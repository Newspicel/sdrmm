mod count;
pub(crate) mod frame;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_dsp::{BitSync, Decimator, FmDemod, RealDecimator, SyncDetector, design_lowpass};
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent, DecoderFamily, EventKind,
    MAX_PER_PAYLOAD_BYTES, MIN_PER_PAYLOAD_BYTES, PerBaud, PerParams,
};

use crate::{ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_input_rate};
use count::Count;

const RATE: f64 = 96_000.0;
const CHANNEL_TAPS: usize = 63;
const SYNC_TOLERANCE: u32 = 2;
const REPORT_SAMPLES: u64 = RATE as u64;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "per".to_owned(),
    name: "PER tester".to_owned(),
    summary: "Packet and bit error rate of a test frame stream".to_owned(),
    family: DecoderFamily::Utility,
    bandwidth_hz: 2.0 * half_band_hz(PerBaud::Baud9600),
    input_rate_hz: RATE,
    has_audio: false,
    emits: vec![EventKind::Per],
    ..ChannelDescriptor::default()
});

#[must_use]
pub(crate) fn deviation_hz(baud: PerBaud) -> f64 {
    baud.rate() / 2.0
}

fn half_band_hz(baud: PerBaud) -> f64 {
    deviation_hz(baud) + baud.rate()
}

fn params(settings: &ChannelSettings) -> Result<PerParams, ChannelError> {
    match &settings.params {
        ChannelParams::Per(params)
            if (MIN_PER_PAYLOAD_BYTES..=MAX_PER_PAYLOAD_BYTES).contains(&params.payload_bytes) =>
        {
            Ok(*params)
        }
        ChannelParams::Per(params) => Err(ChannelError::InvalidSettings(format!(
            "payload must be {MIN_PER_PAYLOAD_BYTES}..={MAX_PER_PAYLOAD_BYTES} bytes, got {}",
            params.payload_bytes
        ))),
        other => Err(ChannelError::InvalidSettings(format!(
            "PER channel got {} params",
            other.type_id()
        ))),
    }
}

pub(crate) fn occupied_band(params: &PerParams) -> (f64, f64) {
    let half = half_band_hz(params.baud);
    (-half, half)
}

pub(crate) fn channel_filter(params: &PerParams) -> ChannelFilter {
    ChannelFilter::Symmetric(Decimator::new(
        &design_lowpass(CHANNEL_TAPS, half_band_hz(params.baud) / RATE),
        1,
    ))
}

struct Demod {
    fm: FmDemod,
    matched: RealDecimator,
    sync: BitSync,
    discriminated: Vec<f32>,
    smoothed: Vec<f32>,
}

impl Demod {
    fn new(baud: PerBaud) -> Self {
        let sps = (RATE / baud.rate()).round() as usize;
        Self {
            fm: FmDemod::new(RATE, deviation_hz(baud)),
            matched: RealDecimator::new(&vec![1.0 / sps as f32; sps], 1),
            sync: BitSync::new(RATE, baud.rate()),
            discriminated: Vec::new(),
            smoothed: Vec::new(),
        }
    }

    fn bits(&mut self, iq: &[Complex<f32>], mut bit: impl FnMut(bool)) {
        self.fm.process(iq, &mut self.discriminated);
        self.matched
            .process(&self.discriminated, &mut self.smoothed);
        for &sample in &self.smoothed {
            if let Some(symbol) = self.sync.push(sample) {
                bit(symbol);
            }
        }
    }
}

struct Deframer {
    sync: SyncDetector,
    inverted: SyncDetector,
    body: Vec<bool>,
    body_len: usize,
    polarity: Option<bool>,
}

impl Deframer {
    fn new(payload_bytes: u16) -> Self {
        let body_len = frame::body_bits(payload_bytes);
        let mask = (1u64 << frame::SYNC_BITS) - 1;
        Self {
            sync: SyncDetector::new(frame::SYNC, frame::SYNC_BITS, SYNC_TOLERANCE),
            inverted: SyncDetector::new(!frame::SYNC & mask, frame::SYNC_BITS, SYNC_TOLERANCE),
            body: Vec::with_capacity(body_len),
            body_len,
            polarity: None,
        }
    }

    fn push(&mut self, bit: bool) -> Option<&[bool]> {
        let Some(inverted) = self.polarity else {
            let normal = self.sync.push(bit);
            let flipped = self.inverted.push(bit);
            if normal || flipped {
                self.polarity = Some(!normal);
                self.body.clear();
            }
            return None;
        };
        self.body.push(bit != inverted);
        if self.body.len() < self.body_len {
            return None;
        }
        self.polarity = None;
        self.sync.reset();
        self.inverted.reset();
        Some(&self.body)
    }
}

pub struct PerChannel {
    params: PerParams,
    demod: Demod,
    deframer: Deframer,
    expected: Vec<bool>,
    count: Count,
    samples: u64,
    reported: Option<u64>,
}

impl PerChannel {
    fn rebuild(&mut self) {
        self.demod = Demod::new(self.params.baud);
        self.deframer = Deframer::new(self.params.payload_bytes);
        self.expected = frame::payload(self.params.payload_bytes);
        self.count.reset();
        self.samples = 0;
        self.reported = None;
    }
}

impl ChannelRx for PerChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let params = params(&settings)?;
        Ok(Self {
            demod: Demod::new(params.baud),
            deframer: Deframer::new(params.payload_bytes),
            expected: frame::payload(params.payload_bytes),
            params,
            count: Count::default(),
            samples: 0,
            reported: None,
        })
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = params(&settings)?;
        if params != self.params {
            self.params = params;
            self.rebuild();
        }
        Ok(())
    }

    fn retuned(&mut self) {
        self.rebuild();
    }

    fn needs_gated_input(&self) -> bool {
        false
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let Self {
            demod,
            deframer,
            expected,
            count,
            ..
        } = self;
        demod.bits(iq, |bit| {
            if let Some(body) = deframer.push(bit) {
                count.frame(frame::check(body, expected));
            }
        });
        let before = self.samples / REPORT_SAMPLES;
        self.samples += iq.len() as u64;
        let frames = self.count.frames();
        if self.samples / REPORT_SAMPLES > before && self.reported != Some(frames) && frames > 0 {
            self.reported = Some(frames);
            out.events.push(DecoderEvent::Per(self.count.report()));
        }
    }

    fn rejected(&self) -> u32 {
        u32::try_from(self.count.errored()).unwrap_or(u32::MAX)
    }
}

#[cfg(test)]
mod tests;
