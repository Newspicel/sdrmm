pub(crate) mod frame;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_dsp::{BitSync, Decimator, FmDemod, Nco, RealDecimator, SyncDetector, design_lowpass};
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent, DecoderFamily, EotMessage,
    EotParams, EotReport,
};

use crate::{ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_input_rate};
use frame::{Decoded, HEAD, HEAD_BLOCK_BITS, HEAD_COPIES, REAR, REAR_BITS};

const RATE: f64 = 48_000.0;
pub(crate) const BAUD: f64 = 1_200.0;
pub(crate) const MARK_HZ: f64 = 1_200.0;
pub(crate) const SPACE_HZ: f64 = 1_800.0;
pub(crate) const DEVIATION_HZ: f64 = 3_000.0;
const CHANNEL_TAPS: usize = 65;
const AUDIO_DECIMATION: usize = 4;
const AUDIO_RATE: f64 = RATE / AUDIO_DECIMATION as f64;
const AUDIO_TAPS: usize = 31;
const AUDIO_CUTOFF_HZ: f64 = 2_600.0;
const CLIP: f32 = 1.2;
const TONE_TAPS: usize = 31;
const TONE_CUTOFF_HZ: f64 = 1_000.0;
const SAMPLES_PER_BIT: usize = (AUDIO_RATE / BAUD) as usize;
const REAR_TOLERANCE: u32 = 2;
const HEAD_TOLERANCE: u32 = 3;
const DEDUP_SAMPLES: u64 = 2 * RATE as u64;
const REPEAT_BITS: u64 = 144;
const HISTORY_BITS: usize = 256;
const MIN_BANDWIDTH_HZ: f64 = 6_000.0;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "eot".to_owned(),
    name: "End-of-Train".to_owned(),
    summary: "Railroad EOT telemetry and HOT commands".to_owned(),
    family: DecoderFamily::Utility,
    bandwidth_hz: 12_500.0,
    input_rate_hz: RATE,
    has_audio: false,
    decoder_kind: Some("eot".to_owned()),
    ..ChannelDescriptor::default()
});

struct Demod {
    fm: FmDemod,
    audio: RealDecimator,
    mixer: Nco,
    tone: Decimator,
    discriminator: FmDemod,
    matched: RealDecimator,
    sync: BitSync,
    discriminated: Vec<f32>,
    decimated: Vec<f32>,
    mixed: Vec<Complex<f32>>,
    filtered: Vec<Complex<f32>>,
    frequency: Vec<f32>,
    smoothed: Vec<f32>,
}

impl Demod {
    fn new() -> Self {
        let centre = (MARK_HZ + SPACE_HZ) / 2.0;
        Self {
            fm: FmDemod::new(RATE, DEVIATION_HZ),
            audio: RealDecimator::new(
                &design_lowpass(AUDIO_TAPS, AUDIO_CUTOFF_HZ / RATE),
                AUDIO_DECIMATION,
            ),
            mixer: Nco::new(-centre as f32, AUDIO_RATE as f32),
            tone: Decimator::new(&design_lowpass(TONE_TAPS, TONE_CUTOFF_HZ / AUDIO_RATE), 1),
            discriminator: FmDemod::new(AUDIO_RATE, (SPACE_HZ - MARK_HZ) / 2.0),
            matched: RealDecimator::new(&[1.0 / SAMPLES_PER_BIT as f32; SAMPLES_PER_BIT], 1),
            sync: BitSync::new(AUDIO_RATE, BAUD),
            discriminated: Vec::new(),
            decimated: Vec::new(),
            mixed: Vec::new(),
            filtered: Vec::new(),
            frequency: Vec::new(),
            smoothed: Vec::new(),
        }
    }

    fn reset(&mut self) {
        self.audio.reset();
        self.tone.reset();
        self.matched.reset();
        self.sync.reset();
    }

    fn symbols(&mut self, iq: &[Complex<f32>], out: &mut Vec<f32>) {
        self.fm.process(iq, &mut self.discriminated);
        self.discriminated
            .iter_mut()
            .for_each(|sample| *sample = sample.clamp(-CLIP, CLIP));
        self.audio.process(&self.discriminated, &mut self.decimated);
        self.mixed.clear();
        let mixer = &mut self.mixer;
        self.mixed.extend(
            self.decimated
                .iter()
                .map(|&sample| Complex::new(sample, 0.0) * mixer.next_sample()),
        );
        self.tone.process(&self.mixed, &mut self.filtered);
        self.discriminator
            .process(&self.filtered, &mut self.frequency);
        self.matched.process(&self.frequency, &mut self.smoothed);
        out.clear();
        for &sample in &self.smoothed {
            if let Some(symbol) = self.sync.push_soft(sample) {
                out.push(-symbol);
            }
        }
    }
}

enum Frame {
    Search,
    Rear { polarity: f32, start: u64 },
    Head { polarity: f32, start: u64 },
}

struct Syncs {
    rear: SyncDetector,
    rear_inverted: SyncDetector,
    head: SyncDetector,
    head_inverted: SyncDetector,
}

impl Syncs {
    fn new() -> Self {
        let rear_mask = (1 << frame::REAR_SYNC_BITS) - 1;
        let head_mask = (1 << frame::HEAD_SYNC_BITS) - 1;
        Self {
            rear: SyncDetector::new(frame::REAR_SYNC, frame::REAR_SYNC_BITS, REAR_TOLERANCE),
            rear_inverted: SyncDetector::new(
                !frame::REAR_SYNC & rear_mask,
                frame::REAR_SYNC_BITS,
                REAR_TOLERANCE,
            ),
            head: SyncDetector::new(frame::HEAD_SYNC, frame::HEAD_SYNC_BITS, HEAD_TOLERANCE),
            head_inverted: SyncDetector::new(
                !frame::HEAD_SYNC & head_mask,
                frame::HEAD_SYNC_BITS,
                HEAD_TOLERANCE,
            ),
        }
    }

    fn push(&mut self, bit: bool) -> Option<Frame> {
        let rear = self.rear.push(bit);
        let rear_inverted = self.rear_inverted.push(bit);
        let head = self.head.push(bit);
        let head_inverted = self.head_inverted.push(bit);
        if head || head_inverted {
            Some(Frame::Head {
                polarity: if head { 1.0 } else { -1.0 },
                start: 0,
            })
        } else if rear || rear_inverted {
            Some(Frame::Rear {
                polarity: if rear { 1.0 } else { -1.0 },
                start: 0,
            })
        } else {
            None
        }
    }

    fn reset(&mut self) {
        self.rear.reset();
        self.rear_inverted.reset();
        self.head.reset();
        self.head_inverted.reset();
    }
}

pub struct EotChannel {
    demod: Demod,
    syncs: Syncs,
    frame: Frame,
    symbols: Vec<f32>,
    rejected: u32,
    clock: u64,
    bit_clock: u64,
    history: [f32; HISTORY_BITS],
    repeats: [Option<(u64, f32)>; 2],
    last: Option<(u64, u32, EotReport)>,
}

fn params(settings: &ChannelSettings) -> Result<&EotParams, ChannelError> {
    match &settings.params {
        ChannelParams::Eot(params) => Ok(params),
        other => Err(ChannelError::InvalidSettings(format!(
            "eot channel got {} params",
            other.type_id()
        ))),
    }
}

fn check_params(params: &EotParams) -> Result<(), ChannelError> {
    if params.bandwidth_hz.is_finite()
        && params.bandwidth_hz >= MIN_BANDWIDTH_HZ
        && params.bandwidth_hz < RATE
    {
        Ok(())
    } else {
        Err(ChannelError::InvalidSettings(format!(
            "eot bandwidth must be in [{MIN_BANDWIDTH_HZ}, {RATE}) Hz, got {}",
            params.bandwidth_hz
        )))
    }
}

pub(crate) fn occupied_band(params: &EotParams) -> (f64, f64) {
    let half = params.bandwidth_hz / 2.0;
    (-half, half)
}

pub(crate) fn channel_filter(params: &EotParams) -> Result<ChannelFilter, ChannelError> {
    check_params(params)?;
    let half = params.bandwidth_hz / 2.0;
    Ok(ChannelFilter::Symmetric(Decimator::new(
        &design_lowpass(CHANNEL_TAPS, half / RATE),
        1,
    )))
}

impl EotChannel {
    fn symbol(&mut self, symbol: f32, out: &mut ChannelOutputs) {
        self.history[self.bit_clock as usize % HISTORY_BITS] = symbol;
        self.bit_clock += 1;
        let synced = self.syncs.push(symbol > 0.0);
        self.finish_repeats(out);
        match &mut self.frame {
            Frame::Search => {
                if let Some(mut frame) = synced {
                    if let Frame::Rear { start, .. } | Frame::Head { start, .. } = &mut frame {
                        *start = self.bit_clock;
                    }
                    self.frame = frame;
                }
            }
            Frame::Rear { polarity, start } => {
                if self.bit_clock == *start + REAR_BITS as u64 {
                    let (polarity, start) = (*polarity, *start);
                    self.frame = Frame::Search;
                    self.finish_rear(start, polarity, out);
                }
            }
            Frame::Head { polarity, start } => {
                if self.bit_clock == *start + (HEAD_COPIES * HEAD_BLOCK_BITS) as u64 {
                    let (polarity, start) = (*polarity, *start);
                    self.frame = Frame::Search;
                    let decoded = self.head(start, polarity);
                    self.emit(decoded, out);
                }
            }
        }
    }

    fn copy<const N: usize>(&self, start: u64, polarity: f32) -> [f32; N] {
        std::array::from_fn(|k| self.history[(start as usize + k) % HISTORY_BITS] * polarity)
    }

    fn head(&self, start: u64, polarity: f32) -> Option<Decoded> {
        let soft: [[f32; HEAD_BLOCK_BITS]; HEAD_COPIES] = std::array::from_fn(|copy| {
            self.copy(start + (copy * HEAD_BLOCK_BITS) as u64, polarity)
        });
        let summed: [f32; HEAD_BLOCK_BITS] =
            std::array::from_fn(|k| soft.iter().map(|copy| copy[k]).sum());
        let copies = std::array::from_fn(|copy| frame::hard(HEAD, &soft[copy]));
        frame::head(frame::hard(HEAD, &summed), &copies)
    }

    fn paired(&self, start: u64, other: u64, polarity: f32) -> Option<Decoded> {
        let one: [f32; REAR_BITS] = self.copy(start, polarity);
        let two: [f32; REAR_BITS] = self.copy(other, polarity);
        let summed: [f32; REAR_BITS] = std::array::from_fn(|k| one[k] + two[k]);
        frame::rear(frame::hard(REAR, &summed))
    }

    fn finish_rear(&mut self, start: u64, polarity: f32, out: &mut ChannelOutputs) {
        let alone: [f32; REAR_BITS] = self.copy(start, polarity);
        let decoded = frame::rear(frame::hard(REAR, &alone)).or_else(|| {
            start
                .checked_sub(REPEAT_BITS)
                .and_then(|earlier| self.paired(start, earlier, polarity))
        });
        if decoded.is_some() {
            self.emit(decoded, out);
        } else if let Some(slot) = self.repeats.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some((start, polarity));
        } else {
            self.emit(None, out);
        }
    }

    fn finish_repeats(&mut self, out: &mut ChannelOutputs) {
        for slot in 0..self.repeats.len() {
            let Some((start, polarity)) = self.repeats[slot] else {
                continue;
            };
            if self.bit_clock < start + REPEAT_BITS + REAR_BITS as u64 {
                continue;
            }
            self.repeats[slot] = None;
            let decoded = self.paired(start, start + REPEAT_BITS, polarity);
            self.emit(decoded, out);
        }
    }

    fn emit(&mut self, decoded: Option<Decoded>, out: &mut ChannelOutputs) {
        let Some(decoded) = decoded else {
            self.rejected = self.rejected.saturating_add(1);
            return;
        };
        let clock = self.clock;
        let repeat = self.last.as_ref().is_some_and(|(seen, address, report)| {
            clock.saturating_sub(*seen) < DEDUP_SAMPLES
                && *address == decoded.unit_address
                && *report == decoded.report
        });
        self.last = Some((clock, decoded.unit_address, decoded.report.clone()));
        if repeat {
            return;
        }
        out.events.push(DecoderEvent::Eot(EotMessage {
            unit_address: decoded.unit_address,
            report: decoded.report,
            errors_corrected: decoded.errors,
            rejected: self.rejected,
        }));
    }

    fn reset(&mut self) {
        self.demod.reset();
        self.syncs.reset();
        self.frame = Frame::Search;
        self.repeats = [None; 2];
        self.last = None;
    }
}

impl ChannelRx for EotChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        check_params(params(&settings)?)?;
        frame::prepare();
        Ok(Self {
            demod: Demod::new(),
            syncs: Syncs::new(),
            frame: Frame::Search,
            symbols: Vec::new(),
            rejected: 0,
            clock: 0,
            bit_clock: 0,
            history: [0.0; HISTORY_BITS],
            repeats: [None; 2],
            last: None,
        })
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        check_params(params(&settings)?)
    }

    fn retuned(&mut self) {
        self.reset();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let mut symbols = std::mem::take(&mut self.symbols);
        self.demod.symbols(iq, &mut symbols);
        self.clock += iq.len() as u64;
        for &symbol in &symbols {
            self.symbol(symbol, out);
        }
        self.symbols = symbols;
    }
}

#[cfg(test)]
pub(crate) mod tests;
