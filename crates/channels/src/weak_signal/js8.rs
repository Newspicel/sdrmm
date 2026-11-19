mod code;
mod model;
mod varicode;

use std::sync::LazyLock;

use sdrmm_decoder::registry::Native;
use sdrmm_wire::{
    ChannelDescriptor, DecoderEvent, EventKind, HzRange, KindSpec, Modulation, SignalSignature,
    Verification,
};

use super::ftx::{Engine, Found, Search};
use super::{
    Decoded, INPUT_RATE_HZ, Job, Js8Channel, WsjtParams, descriptor, grid_timing, search_band,
    search_filter, wsjt_config, wsjt_form,
};
pub use model::Js8Message;
use varicode::Frame;

#[cfg(test)]
pub(crate) use varicode::pack;

const PASSES: usize = 3;
/// Normal windows start every hop; a Slow window is searched every this many hops.
const SLOW_EVERY: u64 = 2;
const HOP_SAMPLES: u64 = 24_000;

pub(crate) static JS8_DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| {
    descriptor(
        "js8",
        "JS8",
        "JS8Call keyboard chat",
        EventKind::named("js8"),
        Verification::OnAir,
    )
});

#[derive(Clone, Copy)]
enum Speed {
    Normal,
    Slow,
}

impl Speed {
    fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Slow => "Slow",
        }
    }

    fn period_s(self) -> f64 {
        match self {
            Self::Normal => 15.0,
            Self::Slow => 30.0,
        }
    }
}

const NOMINAL_START_S: f64 = 0.5;

/// Decodes JS8 Normal from each window and JS8 Slow from the last 30 seconds, which it keeps by
/// stitching the windows together.
pub(crate) struct Js8Decoder {
    normal: Engine,
    slow: Engine,
    history: Vec<f32>,
    history_end: Option<u64>,
}

impl Js8Decoder {
    pub(crate) fn new() -> Self {
        Self {
            normal: Engine::new(&code::NORMAL),
            slow: Engine::new(&code::SLOW),
            history: Vec::with_capacity(code::SLOW.slot_samples + code::NORMAL.slot_samples),
            history_end: None,
        }
    }

    pub(crate) fn decode(&mut self, job: &Job) -> Vec<Decoded> {
        let search = Search {
            low_hz: job.audio_low_hz,
            high_hz: job.audio_high_hz,
            max_candidates: job.max_candidates,
            passes: PASSES,
            osd_order: 0,
        };
        let mut accept = |payload: u128, _osd: bool| Some(Frame::read(payload).raw);
        let mut decoded: Vec<Decoded> = self
            .normal
            .decode(&job.samples, &search, &[], &mut accept)
            .into_iter()
            .map(|found| event(job, Speed::Normal, &found, 0.0))
            .collect();
        self.remember(job);
        let end = job.window_start + job.samples.len() as u64;
        if self.history.len() == code::SLOW.slot_samples
            && (end / HOP_SAMPLES).is_multiple_of(SLOW_EVERY)
        {
            let lead = (code::SLOW.slot_samples - job.samples.len()) as f32 / INPUT_RATE_HZ as f32;
            decoded.extend(
                self.slow
                    .decode(&self.history, &search, &[], &mut accept)
                    .into_iter()
                    .map(|found| event(job, Speed::Slow, &found, -lead)),
            );
        }
        decoded
    }

    fn remember(&mut self, job: &Job) {
        let end = job.window_start + job.samples.len() as u64;
        match self.history_end {
            Some(known) if (job.window_start..=end).contains(&known) => {
                let fresh = (known - job.window_start) as usize;
                self.history.extend_from_slice(&job.samples[fresh..]);
            }
            _ => {
                self.history.clear();
                self.history.extend_from_slice(&job.samples);
            }
        }
        self.history_end = Some(end);
        let excess = self.history.len().saturating_sub(code::SLOW.slot_samples);
        self.history.drain(..excess);
    }
}

/// The event for a frame found in a window that starts `shift_s` seconds off the job's.
fn event(job: &Job, speed: Speed, found: &Found, shift_s: f32) -> Decoded {
    let frame = Frame::read(found.payload);
    let start_s = found.start_s + shift_s;
    let window = job.window_time.and_then(|time| {
        let shift = std::time::Duration::from_secs_f32(shift_s.abs());
        if shift_s < 0.0 {
            time.checked_sub(shift)
        } else {
            time.checked_add(shift)
        }
    });
    let (time_offset_s, slot) =
        grid_timing(window, found.start_s, NOMINAL_START_S, speed.period_s());
    let message = Js8Message {
        text: frame.text.clone().unwrap_or_else(|| frame.raw.clone()),
        frame_type: frame
            .kind
            .map_or("Unknown", varicode::FrameType::label)
            .to_owned(),
        first: frame.first,
        last: frame.last,
        speed: speed.label().to_owned(),
        callsign: frame.callsign,
        to: frame.to,
        grid: frame.grid,
        snr_db: found.snr_db,
        audio_hz: found.frequency_hz,
        time_offset_s,
        hard_errors: found.hard_errors,
        slot,
        frame: frame.raw,
    };
    Decoded {
        event: DecoderEvent::of(&message),
        text: message.frame,
        audio_hz: message.audio_hz,
        window_offset_s: start_s - NOMINAL_START_S as f32,
    }
}

const JS8_SPOTS: &[HzRange] = &[
    HzRange::spot_khz(1_842.0, 3.0),
    HzRange::spot_khz(3_578.0, 3.0),
    HzRange::spot_khz(7_078.0, 3.0),
    HzRange::spot_khz(10_130.0, 3.0),
    HzRange::spot_khz(14_078.0, 3.0),
    HzRange::spot_khz(18_104.0, 3.0),
    HzRange::spot_khz(21_078.0, 3.0),
    HzRange::spot_khz(24_922.0, 3.0),
    HzRange::spot_khz(28_078.0, 3.0),
    HzRange::spot_khz(50_318.0, 3.0),
];

fn js8_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "JS8",
            &[Modulation::Fsk8],
            HzRange::new(20.0, 90.0),
            "eight tones at 6.25 or 3.125 baud on a JS8 calling frequency",
        )
        .on(JS8_SPOTS),
    ]
}

fn js8_kinds() -> Vec<KindSpec> {
    vec![model::js8_kind()]
}

pub(crate) fn js8_decoder() -> Native<WsjtParams> {
    Native::new::<Js8Channel>(|params| search_band(wsjt_config(params)))
        .filter(|params| search_filter(wsjt_config(params)))
        .home(14_078_000.0)
        .signatures(js8_signatures)
        .form(wsjt_form)
        .kinds(js8_kinds)
}

/// A 15-second window holding one JS8 Normal frame at `audio_hz`, starting half a second in.
#[cfg(test)]
pub(crate) fn normal_slot(payload: u128, audio_hz: f64) -> Vec<num_complex::Complex<f32>> {
    let rate = INPUT_RATE_HZ as usize;
    let wave = super::ftx::waveform(&code::NORMAL, payload, audio_hz);
    let mut iq = vec![num_complex::Complex::new(0.0, 0.0); code::NORMAL.slot_samples];
    for (target, sample) in iq[rate / 2..].iter_mut().zip(wave) {
        target.re = 0.6 * sample;
    }
    iq
}

#[cfg(test)]
mod tests;
