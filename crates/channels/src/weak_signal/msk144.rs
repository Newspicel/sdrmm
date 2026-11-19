//! MSK144, the meteor-scatter mode of WSJT-X: 72 ms frames of offset-QPSK with half-sine
//! pulses (minimum-shift keying at 2 kbit/s), repeated for the whole transmission, carrying a
//! 77-bit message in a (128, 90) LDPC code with a 13-bit CRC.

mod code;
mod frame;
mod search;

use std::f32::consts::PI;
use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_decoder::ChannelError;
use sdrmm_decoder::registry::Native;
use sdrmm_dsp::math::fft::Transform;
use sdrmm_wire::{
    Capability, ChannelDescriptor, DecoderSettings, EventKind, FilterSpec, Hue, HzRange, KindSpec,
    Modulation, NumberSpec, SettingField, SignalSignature, Verification,
};
use serde::{Deserialize, Serialize};

use super::ftx::Found;
use super::{
    Decoded, FILTER_TAPS, INPUT_RATE_HZ, Job, MAX_AUDIO_HZ, MIN_AUDIO_HZ, Mode, Msk144Channel,
    WsjtMessage, descriptor, validate_passband, wsjt_decoded,
};
use frame::{FRAME, FrameDecoder, Message};
use search::{PING_MASKS, PING_STEP_HZ, PING_TOLERANCE_HZ, Search, Synced, ping_window};

const RATE: f64 = 12_000.0;
/// Samples analysed at once, about 0.6 s; each block starts half a block after the last.
pub(crate) const BLOCK: usize = 7_168;
pub(crate) const BLOCK_STEP: usize = BLOCK / 2;
const SPAN: usize = 8 * FRAME;
const ANALYTIC: usize = 8_192;
/// Sync correlation, relative to an ideal frame, that is worth decoding.
const SYNCED: f32 = 1.3;
const FILTER_HALF_WIDTH_HZ: f32 = 1_000.0;
const FILTER_ROLLOFF: f32 = 0.1;
const MIN_SNR_DB: f32 = -8.0;
const MAX_SNR_DB: f32 = 24.0;
/// Frames averaged when no single ping decodes, and where in the block each average sits,
/// in frames.
const AVERAGES: [([bool; 8], f32); 4] = [
    ([true, true, true, true, false, false, false, false], 2.0),
    ([false, false, true, true, true, true, false, false], 4.5),
    ([true, true, true, true, true, false, false, false], 2.5),
    ([true, true, true, true, true, true, true, false], 3.5),
];

pub(crate) struct Msk144Decoder {
    analytic: Analytic,
    search: Search,
    frames: FrameDecoder,
    noise: Option<f32>,
}

impl Msk144Decoder {
    pub(crate) fn new() -> Self {
        Self {
            analytic: Analytic::new(),
            search: Search::new(),
            frames: FrameDecoder::new(),
            noise: None,
        }
    }

    /// Reads one block of [`BLOCK`] samples of 12 kHz USB audio, the carrier expected at
    /// `center_hz` give or take `tolerance_hz`. The start of what it finds is the middle of the
    /// frames that decoded.
    pub(crate) fn decode(
        &mut self,
        block: &[f32],
        center_hz: f32,
        tolerance_hz: f32,
    ) -> Option<Found> {
        let rms = (block.iter().map(|sample| sample * sample).sum::<f32>()
            / block.len().max(1) as f32)
            .sqrt();
        if !rms.is_normal() {
            return None;
        }
        let data = self.analytic.run(block, 1.0 / rms, center_hz);
        let powers: Vec<f32> = data[..SPAN]
            .as_chunks::<FRAME>()
            .0
            .iter()
            .map(|frame| frame.iter().map(Complex::norm_sqr).sum::<f32>() * rms * rms)
            .collect();
        let peak = powers.iter().copied().fold(0.0, f32::max);
        let average = powers.iter().sum::<f32>() / powers.len() as f32;
        let data = &data[..SPAN];
        let found = read_pings(
            &mut self.search,
            &mut self.frames,
            data,
            center_hz,
            tolerance_hz,
        )
        .or_else(|| {
            read_averages(
                &mut self.search,
                &mut self.frames,
                data,
                center_hz,
                tolerance_hz,
            )
        });
        let Some((message, frequency_hz, start_s)) = found else {
            self.noise = Some(match self.noise {
                Some(noise) if average > noise => 0.9 * noise + 0.1 * average,
                _ => average,
            });
            return None;
        };
        let snr_db = self.noise.map_or(0.0, |noise| {
            (10.0 * (peak / noise - 1.0).max(1e-3).log10()).round()
        });
        Some(Found {
            payload: message.payload,
            text: message.text,
            snr_db: snr_db.clamp(MIN_SNR_DB, MAX_SNR_DB),
            frequency_hz,
            start_s,
            hard_errors: message.hard_errors,
        })
    }
}

fn read_pings(
    search: &mut Search,
    frames: &mut FrameDecoder,
    data: &[Complex<f32>],
    center_hz: f32,
    tolerance_hz: f32,
) -> Option<(Message, f32, f32)> {
    for ping in search.pings(data, center_hz, tolerance_hz) {
        let Some(window) = ping_window(data, ping) else {
            continue;
        };
        for mask in &PING_MASKS {
            let carrier_hz = center_hz + ping.offset_hz;
            let Some(synced) =
                search.sync(window, mask, carrier_hz, PING_TOLERANCE_HZ, PING_STEP_HZ)
            else {
                continue;
            };
            if let Some(message) = read_synced(frames, &synced) {
                let start_s = (ping.start + FRAME / 2) as f32 / RATE as f32;
                return Some((message, synced.frequency_hz, start_s));
            }
        }
    }
    None
}

fn read_averages(
    search: &mut Search,
    frames: &mut FrameDecoder,
    data: &[Complex<f32>],
    center_hz: f32,
    tolerance_hz: f32,
) -> Option<(Message, f32, f32)> {
    for (mask, middle) in &AVERAGES {
        let averaged = mask.iter().filter(|&&used| used).count() as f32;
        let Some(synced) = search.sync(data, mask, center_hz, tolerance_hz, 10.0 / averaged) else {
            continue;
        };
        if let Some(message) = read_synced(frames, &synced) {
            let start_s = middle * FRAME as f32 / RATE as f32;
            return Some((message, synced.frequency_hz, start_s));
        }
    }
    None
}

/// Tries each sync peak and a sample either side of it.
fn read_synced(frames: &mut FrameDecoder, synced: &Synced) -> Option<Message> {
    for &peak in &synced.peaks {
        for shift in [peak, peak.saturating_sub(1), (peak + 1).min(FRAME - 1)] {
            let aligned: [Complex<f32>; FRAME] =
                std::array::from_fn(|index| synced.frame[(index + shift) % FRAME]);
            if let Some(message) = frames.decode(&aligned) {
                return Some(message);
            }
        }
    }
    None
}

/// Turns real audio into its analytic signal, band-limited around the carrier.
struct Analytic {
    forward: Transform,
    inverse: Transform,
    buffer: Vec<Complex<f32>>,
    filter: Vec<f32>,
    center_hz: f32,
}

impl Analytic {
    fn new() -> Self {
        Self {
            forward: Transform::forward(ANALYTIC),
            inverse: Transform::inverse(ANALYTIC),
            buffer: vec![Complex::default(); ANALYTIC],
            filter: vec![0.0; ANALYTIC / 2 + 1],
            center_hz: f32::NAN,
        }
    }

    fn run(&mut self, block: &[f32], gain: f32, center_hz: f32) -> &[Complex<f32>] {
        if center_hz != self.center_hz {
            self.shape(center_hz);
        }
        let scale = 2.0 * gain / ANALYTIC as f32;
        self.buffer.fill(Complex::default());
        for (slot, &sample) in self.buffer.iter_mut().zip(block) {
            slot.re = scale * sample;
        }
        self.forward.process(&mut self.buffer);
        for (bin, &weight) in self.buffer.iter_mut().zip(&self.filter) {
            *bin *= weight;
        }
        self.buffer[0] *= 0.5;
        self.buffer[ANALYTIC / 2 + 1..].fill(Complex::default());
        self.inverse.process(&mut self.buffer);
        &self.buffer
    }

    /// A raised-cosine band-pass a little wider than the signal.
    fn shape(&mut self, center_hz: f32) {
        self.center_hz = center_hz;
        let resolution = RATE as f32 / ANALYTIC as f32;
        let flat = (1.0 - FILTER_ROLLOFF) * FILTER_HALF_WIDTH_HZ;
        let stop = (1.0 + FILTER_ROLLOFF) * FILTER_HALF_WIDTH_HZ;
        for (bin, weight) in self.filter.iter_mut().enumerate() {
            let away = (bin as f32 * resolution - center_hz).abs();
            *weight = if away <= flat {
                1.0
            } else if away <= stop {
                f32::midpoint(1.0, (PI * (away - flat) / (stop - flat)).cos())
            } else {
                0.0
            };
        }
    }
}

/// The 12 kHz audio of one message frame repeated `frames` times, unit amplitude, with the
/// carrier at `center_hz`.
#[cfg(any(test, feature = "synth"))]
pub(crate) fn waveform(payload: u128, center_hz: f64, frames: usize) -> Option<Vec<f32>> {
    let codeword = code::encode(payload)?;
    let baseband = frame::baseband(&frame::channel_bits(&codeword));
    let step = std::f64::consts::TAU * center_hz / RATE;
    Some(
        (0..frames * FRAME)
            .map(|index| {
                let phase = step * index as f64;
                let sample = baseband[index % FRAME];
                sample.re * phase.cos() as f32 - sample.im * phase.sin() as f32
            })
            .collect(),
    )
}

fn default_audio_hz() -> f32 {
    1_500.0
}

fn default_tolerance_hz() -> f32 {
    100.0
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Msk144Params {
    #[serde(default = "default_audio_hz")]
    pub audio_hz: f32,
    #[serde(default = "default_tolerance_hz")]
    pub tolerance_hz: f32,
}

impl Default for Msk144Params {
    fn default() -> Self {
        Self {
            audio_hz: default_audio_hz(),
            tolerance_hz: default_tolerance_hz(),
        }
    }
}

impl DecoderSettings for Msk144Params {
    const TYPE_ID: &'static str = "msk144";
}

pub(super) static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| {
    descriptor(
        "msk144",
        "MSK144",
        "MSK144 meteor scatter pings",
        EventKind::named("msk144"),
        Verification::OnAir,
    )
});

/// The carrier search as the slot worker carries it: the tolerance either side of the carrier.
pub(super) fn search(params: Msk144Params) -> (f32, f32, usize) {
    (
        params.audio_hz - params.tolerance_hz,
        params.audio_hz + params.tolerance_hz,
        1,
    )
}

fn band(params: Msk144Params) -> (f64, f64) {
    let reach = f64::from(FILTER_HALF_WIDTH_HZ * (1.0 + FILTER_ROLLOFF) + params.tolerance_hz);
    let center = f64::from(params.audio_hz);
    (
        (center - reach).max(f64::from(MIN_AUDIO_HZ)),
        (center + reach).min(f64::from(MAX_AUDIO_HZ)),
    )
}

fn filter(params: Msk144Params) -> Result<FilterSpec, ChannelError> {
    validate_passband(search(params))?;
    let (low, high) = band(params);
    Ok(FilterSpec::band(FILTER_TAPS, low, high))
}

/// Reads one block of a job and reports the time into the period like WSJT-X.
pub(super) fn decoded(decoder: &mut Msk144Decoder, job: &Job, mode: Mode) -> Vec<Decoded> {
    let center_hz = f32::midpoint(job.audio_low_hz, job.audio_high_hz);
    let tolerance_hz = (job.audio_high_hz - job.audio_low_hz) / 2.0;
    decoder
        .decode(&job.samples, center_hz, tolerance_hz)
        .map(|found| {
            let since_start = job.window_start as f64 / INPUT_RATE_HZ + f64::from(found.start_s);
            let unclocked = (since_start % mode.period_s()) as f32;
            wsjt_decoded(job, mode, found, unclocked)
        })
        .into_iter()
        .collect()
}

const SPOTS: &[HzRange] = &[
    HzRange::spot_khz(50_260.0, 3.0),
    HzRange::spot_khz(50_280.0, 3.0),
    HzRange::spot_khz(144_150.0, 3.0),
    HzRange::spot_khz(144_360.0, 3.0),
    HzRange::spot_khz(222_065.0, 3.0),
    HzRange::spot_khz(432_065.0, 3.0),
];

fn signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "MSK144",
            &[Modulation::Fsk2],
            HzRange::new(1_500.0, 2_600.0),
            "2 kbit/s MSK pings, tones 1 kHz apart",
        )
        .symbol_rate(HzRange::about(2_000.0, 0.05))
        .on(SPOTS),
    ]
}

fn form() -> Vec<SettingField> {
    vec![
        SettingField::new("audio_hz", "Audio", NumberSpec::hz(1_000.0, 2_000.0, 10.0))
            .title("USB audio frequency of the carrier"),
        SettingField::new(
            "tolerance_hz",
            "Tolerance",
            NumberSpec::hz(10.0, 200.0, 10.0),
        )
        .title("How far from that frequency a signal is searched"),
    ]
}

pub(crate) fn kind() -> KindSpec {
    KindSpec::tagged::<WsjtMessage>(EventKind::named("msk144"), "MSK144", "Stations", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station, Capability::Spot])
        .sighting()
        .build()
}

fn kinds() -> Vec<KindSpec> {
    vec![kind()]
}

pub(crate) fn msk144_decoder() -> Native<Msk144Params> {
    let decoder = Native::new::<Msk144Channel>(|params| band(*params))
        .filter(|params| filter(*params))
        .home(50_260_000.0)
        .signatures(signatures)
        .form(form)
        .kinds(kinds);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(super::synth::MSK144_SIGNALS),
        _ => decoder,
    }
}

#[cfg(test)]
pub(super) mod tests;
