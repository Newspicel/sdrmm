//! Q65: 65-tone FSK for weak and fading paths, 85 symbols of which 22 sync, carrying a 77-bit
//! message under a QRA code over GF(64). A translation of the WSJT-X decoder (K1JT, IV3NWV et
//! al.) without a-priori or list decoding: every decode is blind.

mod channel;
mod demod;
mod qra;
mod search;
mod tables;
#[cfg(test)]
mod tests;

use num_complex::Complex;

pub(crate) use channel::{DESCRIPTOR, decoded, q65_decoder, search};
pub use channel::{Q65Params, Q65Period, Q65Submode};

use super::ftx::{Found, message::CallBook, message::unpack};
use demod::{HALF_RATE, SymbolSpectra};
use qra::{ALPHABET, CODEWORD_SYMBOLS, MESSAGE_SYMBOLS};
use search::{Candidate, Planner, Spectra};

const SAMPLE_RATE: f32 = 12_000.0;
const SYMBOLS: usize = 85;
/// The sync symbols, numbered from zero.
const SYNC: [usize; 22] = [
    0, 8, 11, 12, 14, 21, 22, 25, 26, 32, 34, 37, 45, 49, 54, 59, 61, 65, 68, 73, 75, 84,
];
const ITERATIONS: usize = 60;
const MAX_DISTANCE: i32 = 5;
const SPREAD_BASE: f32 = 1.72;
const MAX_SPREAD_HZ: f32 = 345.0;
const PAYLOAD_SHIFT: u32 = 1;

/// A Q65 submode: its T/R period and tone spacing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Q65Setup {
    /// 15, 30, 60, 120 or 300.
    pub(crate) period_s: u32,
    /// 0 to 4 for A to E: the tone spacing is the baud rate times `2^submode`.
    pub(crate) submode: u8,
}

impl Q65Setup {
    /// Where a transmission starts in its period.
    pub(crate) fn nominal_start_s(self) -> f32 {
        if self.period_s <= 30 { 0.5 } else { 1.0 }
    }

    /// How long a transmission lasts.
    #[cfg(test)]
    pub(crate) fn transmission_s(self) -> f32 {
        Geometry::of(self).transmission_s()
    }
}

/// The sizes that follow from a [`Q65Setup`].
struct Geometry {
    symbol_samples: usize,
    submode: u8,
}

impl Geometry {
    fn of(setup: Q65Setup) -> Self {
        let symbol_samples = match setup.period_s {
            0..=15 => 1_800,
            16..=30 => 3_600,
            31..=60 => 7_200,
            61..=120 => 16_000,
            _ => 41_472,
        };
        Self {
            symbol_samples,
            submode: setup.submode.min(4),
        }
    }

    fn baud(&self) -> f32 {
        SAMPLE_RATE / self.symbol_samples as f32
    }

    fn tone_bins(&self) -> usize {
        1 << self.submode
    }

    fn row_bins(&self) -> usize {
        ALPHABET * (2 + self.tone_bins())
    }

    fn transmission_s(&self) -> f32 {
        (SYMBOLS * self.symbol_samples) as f32 / SAMPLE_RATE
    }

    fn smoothing_passes(&self) -> usize {
        let passes = self.tone_bins() * self.tone_bins() / 2;
        if passes <= 1 { 0 } else { passes }
    }

    /// The Doppler spreads tried, as powers of 1.72 Hz.
    fn spreads(&self) -> std::ops::RangeInclusive<i32> {
        match self.submode {
            0 => 1..=7,
            1 => 3..=9,
            _ => 8..=14,
        }
    }
}

pub(crate) struct Q65Decoder {
    geometry: Geometry,
    planner: Planner,
    symbols: SymbolSpectra,
    codec: qra::Decoder,
    book: CallBook,
    shifted: Vec<Complex<f32>>,
    energies: Vec<f32>,
}

struct Heard {
    message: [u8; MESSAGE_SYMBOLS],
    codeword: [u8; CODEWORD_SYMBOLS],
    start_s: f32,
    frequency_hz: f32,
    hard_errors: u32,
}

impl Q65Decoder {
    pub(crate) fn new(setup: Q65Setup) -> Self {
        let geometry = Geometry::of(setup);
        Self {
            planner: Planner::new(geometry.symbol_samples),
            symbols: SymbolSpectra::new(&geometry),
            geometry,
            codec: qra::Decoder::new(),
            book: CallBook::default(),
            shifted: Vec::new(),
            energies: Vec::new(),
        }
    }

    /// Every message heard in `samples`, one period of 12 kHz real USB audio whose signals may
    /// start anywhere from a second before it to the latest start that still fits.
    pub(crate) fn decode(
        &mut self,
        samples: &[f32],
        low_hz: f32,
        high_hz: f32,
        max_candidates: usize,
    ) -> Vec<Found> {
        let spectra = self.planner.spectra(samples, &self.geometry);
        let window_s = samples.len() as f32 / SAMPLE_RATE;
        let candidates = search::candidates(
            &spectra,
            &self.geometry,
            window_s,
            low_hz,
            high_hz,
            max_candidates,
        );
        if candidates.is_empty() {
            return Vec::new();
        }
        let analytic = demod::analytic(samples);
        let mut found: Vec<Found> = Vec::new();
        for candidate in candidates {
            if found
                .iter()
                .any(|f| self.overlaps(f.frequency_hz, candidate.frequency_hz))
            {
                continue;
            }
            let Some(heard) = self.attempt(&analytic, candidate) else {
                continue;
            };
            if let Some(decode) = self.report(&spectra, &heard)
                && !found.iter().any(|f| f.text == decode.text)
            {
                found.push(decode);
            }
        }
        found
    }

    fn overlaps(&self, decoded_hz: f32, candidate_hz: f32) -> bool {
        let tone = self.geometry.baud() * self.geometry.tone_bins() as f32;
        let offset = candidate_hz - decoded_hz;
        offset > -tone && offset < 65.0 * tone
    }

    /// Decodes one candidate, nudging its frequency by half a tone bin and its start by a
    /// sixteenth of a symbol, and trying Doppler spreads around the submode's typical one.
    fn attempt(&mut self, analytic: &[Complex<f32>], candidate: Candidate) -> Option<Heard> {
        let baud = self.geometry.baud();
        let half_symbol = self.geometry.symbol_samples / 2;
        let spreads = self.geometry.spreads();
        let middle = (spreads.start() + spreads.end()) / 2;
        for frequency_step in [0, -1, 1] {
            let frequency_hz = candidate.frequency_hz + 0.5 * baud * frequency_step as f32;
            demod::shifted(analytic, frequency_hz, &mut self.shifted);
            for time_step in [0, -1, 1] {
                let start = (candidate.start_s * HALF_RATE).round() as isize
                    + time_step * (half_symbol / 16) as isize;
                self.symbols
                    .energies(&self.shifted, &self.geometry, start, &mut self.energies);
                for spread in spreads.clone() {
                    let distance = frequency_step * frequency_step
                        + time_step as i32 * time_step as i32
                        + (spread - middle) * (spread - middle);
                    let spread_hz = SPREAD_BASE.powi(spread);
                    if distance > MAX_DISTANCE || spread_hz > MAX_SPREAD_HZ {
                        continue;
                    }
                    if let Some((message, codeword)) = self.try_spread(spread_hz / baud) {
                        return Some(Heard {
                            message,
                            codeword,
                            start_s: start as f32 / HALF_RATE,
                            frequency_hz,
                            hard_errors: self.hard_errors(&codeword),
                        });
                    }
                }
            }
        }
        None
    }

    fn try_spread(
        &mut self,
        spread_symbols: f32,
    ) -> Option<([u8; MESSAGE_SYMBOLS], [u8; CODEWORD_SYMBOLS])> {
        let mut probabilities = [[0.0f32; ALPHABET]; CODEWORD_SYMBOLS];
        qra::intrinsics(
            &self.energies,
            self.geometry.submode,
            spread_symbols,
            &mut probabilities,
        );
        self.codec.decode(&probabilities, ITERATIONS)
    }

    /// Data symbols whose loudest tone is not the decoded one.
    fn hard_errors(&self, codeword: &[u8; CODEWORD_SYMBOLS]) -> u32 {
        let row = self.geometry.row_bins();
        let spacing = self.geometry.tone_bins();
        codeword
            .iter()
            .enumerate()
            .filter(|&(symbol, &value)| {
                let bins = &self.energies[symbol * row..(symbol + 1) * row];
                let loudest = (0..ALPHABET)
                    .max_by(|&a, &b| {
                        bins[ALPHABET + a * spacing].total_cmp(&bins[ALPHABET + b * spacing])
                    })
                    .unwrap_or(0);
                loudest != usize::from(value)
            })
            .count() as u32
    }

    fn report(&mut self, spectra: &Spectra, heard: &Heard) -> Option<Found> {
        let payload = payload_of(&heard.message);
        let text = unpack(payload, &mut self.book)?;
        let tones = channel_tones(&heard.codeword);
        Some(Found {
            payload,
            text,
            frequency_hz: heard.frequency_hz,
            start_s: heard.start_s,
            snr_db: search::snr_db(
                spectra,
                &self.geometry,
                &tones,
                heard.start_s,
                heard.frequency_hz,
            ),
            hard_errors: heard.hard_errors,
        })
    }
}

/// The 77-bit payload from 13 six-bit symbols, whose last bit is padding.
fn payload_of(message: &[u8; MESSAGE_SYMBOLS]) -> u128 {
    message
        .iter()
        .fold(0u128, |acc, &symbol| (acc << 6) | u128::from(symbol))
        >> PAYLOAD_SHIFT
}

#[cfg(any(test, feature = "synth"))]
fn message_of(payload: u128) -> [u8; MESSAGE_SYMBOLS] {
    let padded = payload << PAYLOAD_SHIFT;
    std::array::from_fn(|index| ((padded >> (6 * (MESSAGE_SYMBOLS - 1 - index))) & 0x3F) as u8)
}

/// The tone of each of the 85 symbols: 0 for sync, one above the symbol's value for data.
fn channel_tones(codeword: &[u8; CODEWORD_SYMBOLS]) -> [u8; SYMBOLS] {
    let mut tones = [0u8; SYMBOLS];
    let mut data = codeword.iter();
    for (symbol, tone) in tones.iter_mut().enumerate() {
        if !SYNC.contains(&symbol) {
            *tone = data.next().map_or(0, |&value| value + 1);
        }
    }
    tones
}

/// The transmission of `payload` with its sync tone at `audio_hz`, unit amplitude, from sample 0.
#[cfg(any(test, feature = "synth"))]
pub(crate) fn waveform(setup: Q65Setup, payload: u128, audio_hz: f64) -> Vec<f32> {
    let geometry = Geometry::of(setup);
    let tones = channel_tones(&qra::encode(&message_of(payload)));
    let spacing = f64::from(geometry.baud()) * geometry.tone_bins() as f64;
    let mut phase = 0.0f64;
    let mut wave = Vec::with_capacity(SYMBOLS * geometry.symbol_samples);
    for &tone in &tones {
        let step =
            std::f64::consts::TAU * (audio_hz + spacing * f64::from(tone)) / f64::from(SAMPLE_RATE);
        for _ in 0..geometry.symbol_samples {
            wave.push(phase.sin() as f32);
            phase = (phase + step) % std::f64::consts::TAU;
        }
    }
    wave
}
