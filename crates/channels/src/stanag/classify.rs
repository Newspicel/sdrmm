use std::f64::consts::TAU;

use sdrmm_wire::StanagModem;

use super::{
    RATE_HZ,
    lags::{Lag, Lags, Peak},
    spectrum::Comb,
};

const FRAME_TOLERANCE: f64 = 0.012;
const SHORT_TOLERANCE: f64 = 0.03;
const TONE_TOLERANCE: f64 = 0.04;

const FRAME_4285: Lag = Lag::symbols(256.0, 2_400.0, FRAME_TOLERANCE);
const SYNC_4285: Lag = Lag::symbols(31.0, 2_400.0, FRAME_TOLERANCE);
const FRAME_4529: Lag = Lag::symbols(256.0, 1_200.0, FRAME_TOLERANCE);
const SYNC_4529: Lag = Lag::symbols(31.0, 1_200.0, FRAME_TOLERANCE);
const BLOCK_110A: Lag = Lag::symbols(480.0, 2_400.0, FRAME_TOLERANCE);
const FRAME_4539: Lag = Lag::symbols(288.0, 2_400.0, FRAME_TOLERANCE);
const PROBE_4539: Lag = Lag::symbols(16.0, 2_400.0, SHORT_TOLERANCE);
const PREAMBLE_4539: Lag = Lag::symbols(184.0, 2_400.0, FRAME_TOLERANCE);
const TONES_39: Lag = Lag::period(56.25, TONE_TOLERANCE);
const TONES_16: Lag = Lag::period(110.0, TONE_TOLERANCE);

const SINGLE_CARRIER_TYPICAL: f32 = 0.3;
const FRAME_4539_TYPICAL: f32 = 0.08;
const PROBE_4539_TYPICAL: f32 = 0.06;
const PREAMBLE_4539_TYPICAL: f32 = 0.2;
const PROBE_4539_CORROBORATION_Z: f32 = 3.5;
const FRAME_4539_ALONE_Z: f32 = 15.0;
const TONES_39_TYPICAL: f32 = 0.12;
const TONES_16_TYPICAL: f32 = 0.2;
const SHARED_FRAME_RATIO: f32 = 0.3;
const ALE_CONTRAST_DB: f32 = 8.0;
const ALE_EVENNESS_DB: f32 = -12.0;
const ALE_TYPICAL_DB: f32 = 16.0;
const HOLD_STRENGTH: f32 = 0.025;
const HOLD_Z: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Verdict {
    pub modem: StanagModem,
    pub confidence: f32,
}

struct Peaks {
    frame_4285: Peak,
    frame_4529: Peak,
    block_110a: Peak,
    frame_4539: Peak,
    probe_4539: Peak,
    preamble_4539: Peak,
    tones_39: Peak,
    tones_16: Peak,
}

impl Peaks {
    fn measure(lags: &mut Lags) -> Self {
        Self {
            frame_4285: lags.peak(FRAME_4285),
            frame_4529: lags.peak(FRAME_4529),
            block_110a: lags.peak(BLOCK_110A),
            frame_4539: lags.peak(FRAME_4539),
            probe_4539: lags.peak(PROBE_4539),
            preamble_4539: lags.peak(PREAMBLE_4539),
            tones_39: lags.peak(TONES_39),
            tones_16: lags.peak(TONES_16),
        }
    }
}

fn score(peak: &Peak, typical: f32) -> Option<f32> {
    peak.passes().then(|| peak.strength / typical)
}

fn stanag_4529(peaks: &Peaks) -> Option<f32> {
    (peaks.frame_4285.strength < SHARED_FRAME_RATIO * peaks.frame_4529.strength)
        .then(|| score(&peaks.frame_4529, SINGLE_CARRIER_TYPICAL))
        .flatten()
}

fn stanag_4539(peaks: &Peaks) -> Option<f32> {
    let probe = &peaks.probe_4539;
    let frame = &peaks.frame_4539;
    let framed = frame.passes()
        && (probe.z >= PROBE_4539_CORROBORATION_Z || frame.z >= FRAME_4539_ALONE_Z)
        && !peaks.frame_4285.passes();
    if !(probe.passes() || framed) {
        return None;
    }
    [
        framed.then(|| frame.strength / FRAME_4539_TYPICAL),
        score(probe, PROBE_4539_TYPICAL),
        score(&peaks.preamble_4539, PREAMBLE_4539_TYPICAL),
    ]
    .into_iter()
    .flatten()
    .reduce(f32::max)
}

fn thirty_nine_tone(peaks: &Peaks) -> Option<f32> {
    (peaks.tones_16.strength < peaks.tones_39.strength)
        .then(|| score(&peaks.tones_39, TONES_39_TYPICAL))
        .flatten()
}

fn holds(peak: &Peak) -> bool {
    peak.strength >= HOLD_STRENGTH && peak.z >= HOLD_Z
}

fn evidence(peaks: &Peaks, modem: StanagModem) -> [Peak; 3] {
    let only = |peak: Peak| [peak, Peak::default(), Peak::default()];
    match modem {
        StanagModem::Stanag4285 => only(peaks.frame_4285),
        StanagModem::Stanag4529 => only(peaks.frame_4529),
        StanagModem::MilStd110a => only(peaks.block_110a),
        StanagModem::Stanag4539 => [peaks.frame_4539, peaks.probe_4539, peaks.preamble_4539],
        StanagModem::ThirtyNineTone => only(peaks.tones_39),
        StanagModem::SixteenTone => only(peaks.tones_16),
        StanagModem::Ale2g => [Peak::default(); 3],
    }
}

fn held(peaks: &Peaks, tracked: StanagModem) -> Option<Verdict> {
    evidence(peaks, tracked)
        .into_iter()
        .filter(holds)
        .map(|peak| peak.strength)
        .reduce(f32::max)
        .map(|strength| Verdict {
            modem: tracked,
            confidence: strength.min(1.0),
        })
}

fn ale(comb: &Comb) -> Option<f32> {
    (comb.contrast_db >= ALE_CONTRAST_DB && comb.evenness_db >= ALE_EVENNESS_DB)
        .then_some(comb.contrast_db / ALE_TYPICAL_DB)
}

pub(super) fn classify(
    lags: &mut Lags,
    comb: &Comb,
    tracked: Option<StanagModem>,
) -> Option<Verdict> {
    if let Some(score) = ale(comb) {
        return Some(Verdict {
            modem: StanagModem::Ale2g,
            confidence: score.min(1.0),
        });
    }
    let peaks = Peaks::measure(lags);
    [
        (
            StanagModem::Stanag4285,
            score(&peaks.frame_4285, SINGLE_CARRIER_TYPICAL),
        ),
        (StanagModem::Stanag4529, stanag_4529(&peaks)),
        (
            StanagModem::MilStd110a,
            score(&peaks.block_110a, SINGLE_CARRIER_TYPICAL),
        ),
        (StanagModem::Stanag4539, stanag_4539(&peaks)),
        (StanagModem::ThirtyNineTone, thirty_nine_tone(&peaks)),
        (
            StanagModem::SixteenTone,
            score(&peaks.tones_16, TONES_16_TYPICAL),
        ),
    ]
    .into_iter()
    .filter_map(|(modem, score)| score.map(|score| (modem, score)))
    .max_by(|a, b| a.1.total_cmp(&b.1))
    .map(|(modem, score)| Verdict {
        modem,
        confidence: score.min(1.0),
    })
    .or_else(|| tracked.and_then(|modem| held(&peaks, modem)))
}

fn carrier_lags(modem: StanagModem) -> &'static [Lag] {
    match modem {
        StanagModem::Stanag4285 => &[SYNC_4285, FRAME_4285],
        StanagModem::Stanag4529 => &[SYNC_4529, FRAME_4529],
        StanagModem::Stanag4539 => &[PROBE_4539],
        StanagModem::MilStd110a
        | StanagModem::ThirtyNineTone
        | StanagModem::SixteenTone
        | StanagModem::Ale2g => &[],
    }
}

pub(super) fn refine_offset(lags: &mut Lags, modem: StanagModem, coarse_hz: f64) -> f64 {
    let mut offset_hz = coarse_hz;
    for &lag in carrier_lags(modem) {
        let peak = lags.peak(lag);
        if !peak.passes() {
            break;
        }
        let delay = peak.lag as f64 / RATE_HZ;
        let turns = f64::from(peak.value.arg()) / TAU;
        let wraps = (offset_hz * delay - turns).round();
        offset_hz = (turns + wraps) / delay;
    }
    offset_hz
}
