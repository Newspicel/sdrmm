//! FST4 and FST4W, after WSJT-X (GPL-3.0) `lib/fst4_decode.f90` and `lib/fst4/`.

mod channel;
mod code;
mod message;
mod metrics;
mod model;
mod narrow;
mod search;
#[cfg(any(test, feature = "synth"))]
mod synth;
mod tables;
#[cfg(test)]
mod tests;

use num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};
use sdrmm_dsp::math::fft::Transform;
use std::sync::Arc;

use super::ftx::message::CallBook;
pub(crate) use channel::{
    DESCRIPTOR, FST4W_DESCRIPTOR, decoded, fst4_decoder, fst4w_decoder, search, setup, shift_hz,
};
pub use channel::{Fst4Params, Fst4Period, Fst4wParams, Fst4wPeriod};
use code::{Code, Decoded, N, Payload};
use message::Beacon;
pub use model::{Fst4Message, Fst4wSpot};
pub(crate) use narrow::Narrow;
#[cfg(any(test, feature = "synth"))]
pub(crate) use synth::waveform;

const SAMPLE_RATE: f32 = 12_000.0;
const SYMBOLS: usize = 160;
const SYNC_A: [u8; 8] = [0, 1, 3, 2, 1, 0, 2, 3];
const SYNC_B: [u8; 8] = [2, 3, 1, 0, 3, 2, 0, 1];
const SYNC_STARTS: [usize; 5] = [0, 38, 76, 114, 152];
const DATA_STARTS: [usize; 4] = [8, 46, 84, 122];
const DATA_SYMBOLS: usize = 30;
const MIN_SYNC: f32 = 1.2;
const MIN_SYNC_SHORT: f32 = 1.15;
const SAME_FREQUENCY_BAUD: f32 = 0.1;
const SAME_START: usize = 2;
const JITTER: [isize; 3] = [0, 1, -1];
const NOISE_BANDWIDTH_HZ: f32 = 2_500.0;
const SNR_REFERENCE: f32 = 1.46;
const SNR_SYMBOL_SAMPLES: f32 = 8_200.0;
const NO_SNR_DB: f32 = -99.9;
const LOWEST_HZ: f32 = 100.0;
const HIGHEST_HZ: f32 = 4_800.0;

/// An FST4 or FST4W setting: the T/R period and whether the 50-bit beacon message is sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fst4Setup {
    pub(crate) period_s: u32,
    pub(crate) wspr: bool,
}

/// Samples per symbol, the decimation to baseband and the analysed span for a period, at the
/// rate of the stream decoded: 12 kHz audio, or that audio narrowed `narrowing` times.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Timing {
    symbol_samples: usize,
    decimation: usize,
    fft_samples: usize,
    snr_calibration: f32,
    narrowing: usize,
}

impl Timing {
    fn of(period_s: u32) -> Option<Self> {
        let (symbol_samples, decimation, fft_samples, snr_calibration) = match period_s {
            15 => (720, 18, 180_000, 800.0),
            30 => (1_680, 42, 359_856, 600.0),
            60 => (3_888, 108, 720_000, 430.0),
            120 => (8_200, 205, 1_440_000, 390.0),
            300 => (21_504, 512, 3_594_240, 340.0),
            900 => (66_560, 1_664, 10_782_720, 320.0),
            1_800 => (134_400, 3_360, 21_591_360, 320.0),
            _ => return None,
        };
        Some(Self {
            symbol_samples,
            decimation,
            fft_samples,
            snr_calibration,
            narrowing: 1,
        })
    }

    /// The timing of a stream `by` times slower, when every span divides evenly.
    fn narrowed(self, by: usize) -> Option<Self> {
        let even = |samples: usize| samples.is_multiple_of(by).then_some(samples / by);
        Some(Self {
            symbol_samples: even(self.symbol_samples)?,
            decimation: even(self.decimation)?,
            fft_samples: even(self.fft_samples)?,
            narrowing: self.narrowing * by,
            ..self
        })
    }

    fn rate(&self) -> f32 {
        SAMPLE_RATE / self.narrowing as f32
    }

    fn baud(&self) -> f32 {
        self.rate() / self.symbol_samples as f32
    }

    fn baseband_rate(&self) -> f32 {
        self.rate() / self.decimation as f32
    }

    fn baseband_symbol(&self) -> usize {
        self.symbol_samples / self.decimation
    }

    fn baseband_samples(&self) -> usize {
        self.fft_samples / self.decimation
    }

    /// The span of the stream that carries flat audio.
    fn usable_hz(&self) -> (f32, f32) {
        if self.narrowing == 1 {
            (LOWEST_HZ, HIGHEST_HZ)
        } else {
            narrow::usable_hz(self.rate())
        }
    }
}

/// How many times slower than 12 kHz a period's stream runs, so its window stays near 1.4
/// million samples.
pub(crate) fn narrowing(period_s: u32) -> usize {
    match period_s {
        300 => 4,
        900 => 8,
        1_800 => 16,
        _ => 1,
    }
}

impl Fst4Setup {
    /// FST4 runs 15 s to 1800 s periods, FST4W 120 s and longer.
    pub(crate) fn is_supported(self) -> bool {
        Timing::of(self.period_s).is_some() && (!self.wspr || self.period_s >= 120)
    }

    fn payload(self) -> Payload {
        if self.wspr {
            Payload::Wspr
        } else {
            Payload::Standard
        }
    }
}

/// Where a transmission starts in its period: half a second in for 15 s periods, else one.
pub(crate) fn nominal_start_s(setup: Fst4Setup) -> f32 {
    if setup.period_s == 15 { 0.5 } else { 1.0 }
}

/// How long a transmission lasts: 160 symbols.
#[cfg(test)]
pub(crate) fn tx_seconds(setup: Fst4Setup) -> f32 {
    Timing::of(setup.period_s).map_or(0.0, |timing| {
        (SYMBOLS * timing.symbol_samples) as f32 / SAMPLE_RATE
    })
}

/// A decoded transmission. `beacon` is set for FST4W.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Fst4Found {
    pub(crate) text: String,
    pub(crate) snr_db: f32,
    pub(crate) frequency_hz: f32,
    pub(crate) start_s: f32,
    pub(crate) hard_errors: u32,
    /// Decoded with the first call taken as CQ, WSJT-X's `a1`.
    pub(crate) cq_assumed: bool,
    pub(crate) beacon: Option<Beacon>,
}

/// Decodes one window of a period's stream: 12 kHz audio, or for long periods that audio
/// narrowed by [`Narrow`] to [`narrowing`] times fewer samples.
pub(crate) struct Fst4Decoder {
    setup: Fst4Setup,
    timing: Option<Timing>,
    code: Code,
    book: CallBook,
    forward: Option<Arc<dyn RealToComplex<f32>>>,
    inverse: Option<Transform>,
    input: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    baseband: Vec<Complex<f32>>,
}

impl Fst4Decoder {
    pub(crate) fn new(setup: Fst4Setup) -> Self {
        let timing = setup
            .is_supported()
            .then(|| Timing::of(setup.period_s)?.narrowed(narrowing(setup.period_s)))
            .flatten();
        Self {
            setup,
            timing,
            code: Code::new(setup.payload()),
            book: CallBook::default(),
            forward: timing
                .map(|timing| RealFftPlanner::new().plan_fft_forward(timing.fft_samples)),
            inverse: timing.map(|timing| Transform::inverse(timing.baseband_samples())),
            input: Vec::new(),
            spectrum: Vec::new(),
            baseband: Vec::new(),
        }
    }

    /// Decodes one window of the stream, finding transmissions that start anywhere the whole
    /// frame still fits, with lowest tones from `low_hz` to `high_hz` in the stream.
    pub(crate) fn decode(
        &mut self,
        samples: &[f32],
        low_hz: f32,
        high_hz: f32,
        max_candidates: usize,
    ) -> Vec<Fst4Found> {
        let Some(timing) = self.timing else {
            return Vec::new();
        };
        if !self.transform(samples, &timing) {
            return Vec::new();
        }
        let min_sync = if self.setup.period_s == 15 {
            MIN_SYNC_SHORT
        } else {
            MIN_SYNC
        };
        let found = search::candidates(
            &self.spectrum,
            &timing,
            low_hz,
            high_hz,
            min_sync,
            max_candidates,
        );
        let synced = self.synchronize(&timing, &found);
        let mut decodes: Vec<Fst4Found> = Vec::new();
        for (candidate, synced) in synced {
            if let Some(decode) = self.try_candidate(&timing, candidate, synced)
                && !decodes.iter().any(|known| known.text == decode.text)
            {
                decodes.push(decode);
            }
        }
        decodes
    }

    fn transform(&mut self, samples: &[f32], timing: &Timing) -> bool {
        let Some(forward) = &self.forward else {
            return false;
        };
        self.input.clear();
        self.input.extend(samples.iter().take(timing.fft_samples));
        self.input.resize(timing.fft_samples, 0.0);
        self.spectrum
            .resize(timing.fft_samples / 2 + 1, Complex::default());
        forward.process(&mut self.input, &mut self.spectrum).is_ok()
    }

    fn synchronize(
        &mut self,
        timing: &Timing,
        found: &[search::Candidate],
    ) -> Vec<(search::Candidate, search::Synced)> {
        let Some(inverse) = self.inverse.as_mut() else {
            return Vec::new();
        };
        let waves = search::sync_waveforms(timing.baseband_symbol());
        let mut synced: Vec<(search::Candidate, search::Synced)> = Vec::new();
        for candidate in found {
            search::downsample(
                &self.spectrum,
                timing,
                candidate.center_hz,
                inverse,
                &mut self.baseband,
            );
            let Some((offset_hz, start)) = search::sync_search(&self.baseband, timing, &waves)
            else {
                continue;
            };
            let center_hz = candidate.center_hz + offset_hz;
            let repeated = synced.iter().any(|(_, known)| {
                (known.center_hz - center_hz).abs() < SAME_FREQUENCY_BAUD * timing.baud()
                    && known.start.abs_diff(start) <= SAME_START
            });
            if !repeated {
                synced.push((*candidate, search::Synced { center_hz, start }));
            }
        }
        synced
    }

    fn try_candidate(
        &mut self,
        timing: &Timing,
        candidate: search::Candidate,
        synced: search::Synced,
    ) -> Option<Fst4Found> {
        let inverse = self.inverse.as_mut()?;
        search::downsample(
            &self.spectrum,
            timing,
            synced.center_hz,
            inverse,
            &mut self.baseband,
        );
        let per_symbol = timing.baseband_symbol();
        let references = metrics::references(per_symbol);
        for jitter in JITTER {
            let Some(start) = synced.start.checked_add_signed(jitter) else {
                continue;
            };
            let Some(frame) = self.baseband.get(start..start + SYMBOLS * per_symbol) else {
                continue;
            };
            let Some(measured) = metrics::measure(frame, &references) else {
                continue;
            };
            let assumed = (!self.setup.wspr).then(|| message::assume_cq(&measured.llrs[3]));
            let tries = measured.llrs.iter().map(|llr| (llr, false));
            for (llr, cq) in tries.chain(assumed.iter().map(|llr| (llr, true))) {
                let Some(decoded) = self.code.decode(llr) else {
                    continue;
                };
                let Some((text, beacon)) = self.read(&decoded, cq) else {
                    continue;
                };
                return Some(Fst4Found {
                    cq_assumed: cq,
                    text,
                    snr_db: snr_db(timing, &decoded, &measured.powers, candidate.noise),
                    frequency_hz: synced.center_hz - 1.5 * timing.baud(),
                    start_s: start as f32 / timing.baseband_rate(),
                    hard_errors: decoded.hard_errors,
                    beacon,
                });
            }
        }
        None
    }

    /// Reads the message. One found by ordered statistics or with CQ assumed must be a plain
    /// standard message, the shape that leaves the fewest chances for a false decode.
    fn read(&mut self, decoded: &Decoded, cq: bool) -> Option<(String, Option<Beacon>)> {
        if self.setup.wspr {
            let beacon = message::beacon(&decoded.message, &mut self.book)?;
            return Some((beacon.text.clone(), Some(beacon)));
        }
        if (decoded.osd || cq)
            && !super::ftx::message::is_plain_standard(message::standard_payload(&decoded.message))
        {
            return None;
        }
        message::standard_text(&decoded.message, &mut self.book).map(|text| (text, None))
    }
}

/// The tones a codeword goes out on: five sync words between four blocks of data.
fn tones(codeword: &[bool; N]) -> [u8; SYMBOLS] {
    let mut tones = [0u8; SYMBOLS];
    for (index, &start) in SYNC_STARTS.iter().enumerate() {
        let word = if index % 2 == 0 { &SYNC_A } else { &SYNC_B };
        tones[start..start + 8].copy_from_slice(word);
    }
    let gray = [0u8, 1, 3, 2];
    for (block, &start) in DATA_STARTS.iter().enumerate() {
        for symbol in 0..DATA_SYMBOLS {
            let bit = 2 * (block * DATA_SYMBOLS + symbol);
            let value = 2 * usize::from(codeword[bit]) + usize::from(codeword[bit + 1]);
            tones[start + symbol] = gray[value];
        }
    }
    tones
}

/// WSJT-X's SNR in 2500 Hz: the decoded tones' power against the noise under the candidate.
fn snr_db(timing: &Timing, decoded: &Decoded, powers: &[[f32; 4]], noise: f32) -> f32 {
    let signal: f32 = tones(&decoded.codeword)
        .iter()
        .zip(powers)
        .map(|(&tone, row)| row[usize::from(tone)])
        .sum();
    let ratio = timing.snr_calibration * signal / noise - 1.0;
    if ratio <= 0.0 || !ratio.is_finite() {
        return NO_SNR_DB;
    }
    10.0 * ratio.log10()
        + 10.0 * (SNR_REFERENCE / NOISE_BANDWIDTH_HZ).log10()
        + 10.0 * (SNR_SYMBOL_SAMPLES / (timing.symbol_samples * timing.narrowing) as f32).log10()
}
