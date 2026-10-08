mod demap;
pub mod tables;

use std::f32::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::{ConvCode, Soft, ViterbiK7, depuncture, fft::FftPair};

use self::tables::{
    CP, FFT, PILOT_CARRIERS, PILOT_VALUES, RATES, Rate, SERVICE_BITS, SHORT, SYMBOL, bin,
    interleave_table, long_training, polarity, rate_for,
};
use super::{Frame, MAX_MPDU_BYTES, Sink, WifiPhy, byte, fcs_ok, level_dbfs};

pub use self::demap::map as map_point;

const WINDOW: usize = 48;
const PLATEAU: f32 = 0.7;
const PLATEAU_SAMPLES: usize = 24;
const MIN_POWER: f32 = 1e-10;
const LTF_SEARCH: usize = 320;
const BACKOFF: usize = 3;
const SIGNAL_BITS: usize = 24;
const MIN_LENGTH: usize = 14;
const LOOKAHEAD: usize = WINDOW + SHORT + LTF_SEARCH + 2 * FFT + SYMBOL;
const MAX_HISTORY: usize = 600_000;
const PEEK_BITS: usize = SERVICE_BITS + 16 + 96;
const MAX_CODED_BITS: usize = 288;
const MAX_DATA_BITS: usize = 24 * 1_366;
const MAX_CODED_STREAM: usize = 66_000;

#[derive(Clone, Copy)]
enum State {
    Search,
    Pending(usize),
}

enum Outcome {
    Frame(Rate, usize),
    Skip(usize),
    Short,
    Failed,
}

struct Scratch {
    scores: [f32; LTF_SEARCH + FFT],
    received: [f32; MAX_CODED_BITS],
    coded: Vec<Soft>,
    full: Vec<Soft>,
    bits: Vec<bool>,
    bytes: Vec<u8>,
    tables: Vec<Vec<usize>>,
}

impl Scratch {
    fn new() -> Self {
        Self {
            scores: [0.0; LTF_SEARCH + FFT],
            received: [0.0; MAX_CODED_BITS],
            coded: Vec::with_capacity(MAX_CODED_STREAM),
            full: Vec::with_capacity(2 * MAX_DATA_BITS + MAX_CODED_BITS),
            bits: Vec::with_capacity(MAX_DATA_BITS + MAX_CODED_BITS),
            bytes: Vec::with_capacity(MAX_MPDU_BYTES),
            tables: RATES.iter().map(|&rate| interleave_table(rate)).collect(),
        }
    }
}

pub struct Ofdm {
    history: Vec<Complex<f32>>,
    correlation: Complex<f32>,
    energy: f32,
    run: usize,
    holding: bool,
    primed: bool,
    consumed: u64,
    cursor: usize,
    state: State,
    fft: FftPair,
    reference: [Complex<f32>; FFT],
    polarity: [f32; 127],
    viterbi: ViterbiK7,
    scratch: Scratch,
    rejected: u32,
}

struct Lock {
    origin: usize,
    cfo: f32,
    signal: usize,
    channel: [Complex<f32>; FFT],
    gain: f32,
}

impl Default for Ofdm {
    fn default() -> Self {
        Self::new()
    }
}

impl Ofdm {
    #[must_use]
    pub fn new() -> Self {
        let mut fft = FftPair::new(FFT);
        let mut reference = [Complex::new(0.0, 0.0); FFT];
        for carrier in -26..=26 {
            reference[bin(carrier)] = Complex::new(long_training(carrier), 0.0);
        }
        fft.inverse(&mut reference);
        Self {
            history: Vec::with_capacity(MAX_HISTORY),
            correlation: Complex::new(0.0, 0.0),
            energy: 0.0,
            run: 0,
            holding: false,
            primed: false,
            consumed: 0,
            cursor: 0,
            state: State::Search,
            fft,
            reference,
            polarity: polarity(),
            viterbi: ViterbiK7::with_capacity(
                ConvCode::new(&[0o133, 0o171]),
                MAX_DATA_BITS + MAX_CODED_BITS,
            ),
            scratch: Scratch::new(),
            rejected: 0,
        }
    }

    #[must_use]
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    pub fn reset(&mut self) {
        self.history.clear();
        self.consumed = 0;
        self.cursor = 0;
        self.state = State::Search;
        self.primed = false;
        self.run = 0;
        self.holding = false;
    }

    pub fn process(&mut self, iq: &[Complex<f32>], sink: &mut impl Sink) {
        self.history.extend_from_slice(iq);
        loop {
            match self.state {
                State::Pending(start) => match self.demodulate(start, sink) {
                    Outcome::Short => break,
                    Outcome::Frame(rate, end) => {
                        sink.frame(Frame {
                            mpdu: &self.scratch.bytes,
                            phy: WifiPhy::Ofdm {
                                mbps: rate.mbps as u8,
                            },
                            level_dbfs: level_dbfs(&self.history[start..end]),
                            sample: self.consumed + start as u64,
                        });
                        self.resume(end, false);
                    }
                    Outcome::Skip(end) => self.resume(end, false),
                    Outcome::Failed => self.resume(start + 1, true),
                },
                State::Search => {
                    if !self.search() {
                        break;
                    }
                }
            }
        }
        self.trim();
    }

    fn resume(&mut self, at: usize, holding: bool) {
        if at > self.cursor {
            self.cursor = at;
            self.primed = false;
        }
        self.state = State::Search;
        self.run = 0;
        self.holding = holding;
    }

    fn search(&mut self) -> bool {
        while self.cursor + LOOKAHEAD <= self.history.len() {
            let at = self.cursor;
            self.slide(at);
            self.cursor += 1;
            let plateau = self.energy > MIN_POWER * WINDOW as f32
                && self.correlation.norm() > PLATEAU * self.energy;
            if !plateau {
                self.run = 0;
                self.holding = false;
                continue;
            }
            if self.holding {
                continue;
            }
            self.run += 1;
            if self.run == PLATEAU_SAMPLES {
                self.state = State::Pending((at + 1).saturating_sub(PLATEAU_SAMPLES));
                return true;
            }
        }
        false
    }

    fn slide(&mut self, at: usize) {
        let h = &self.history;
        if self.primed {
            let (leaving, entering) = (at - 1, at + WINDOW - 1);
            self.correlation +=
                h[entering] * h[entering + SHORT].conj() - h[leaving] * h[leaving + SHORT].conj();
            self.energy += h[entering + SHORT].norm_sqr() - h[leaving + SHORT].norm_sqr();
        } else {
            self.correlation = (0..WINDOW)
                .map(|k| h[at + k] * h[at + k + SHORT].conj())
                .sum();
            self.energy = (0..WINDOW).map(|k| h[at + k + SHORT].norm_sqr()).sum();
            self.primed = true;
        }
    }

    fn sample(&self, origin: usize, cfo: f32, at: usize) -> Complex<f32> {
        let phase = -TAU * cfo * (at as f32 - origin as f32);
        self.history[at] * Complex::from_polar(1.0, phase)
    }

    fn lock(&mut self, origin: usize) -> Option<Lock> {
        let coarse: Complex<f32> = (0..WINDOW)
            .map(|k| self.history[origin + k] * self.history[origin + k + SHORT].conj())
            .sum();
        let coarse_cfo = -coarse.arg() / (TAU * SHORT as f32);
        for offset in 0..LTF_SEARCH + FFT {
            self.scratch.scores[offset] = (0..FFT)
                .map(|k| {
                    self.sample(origin, coarse_cfo, origin + offset + k) * self.reference[k].conj()
                })
                .sum::<Complex<f32>>()
                .norm();
        }
        let scores = &self.scratch.scores;
        let first = (0..LTF_SEARCH).max_by(|&a, &b| {
            (scores[a] + scores[a + FFT]).total_cmp(&(scores[b] + scores[b + FFT]))
        })?;
        let ltf = origin + first;
        let fine: Complex<f32> = (0..FFT)
            .map(|k| {
                self.sample(origin, coarse_cfo, ltf + k)
                    * self.sample(origin, coarse_cfo, ltf + FFT + k).conj()
            })
            .sum();
        let cfo = coarse_cfo - fine.arg() / (TAU * FFT as f32);
        let mut channel = [Complex::new(0.0, 0.0); FFT];
        for repeat in 0..2 {
            let spectrum = self.spectrum(origin, cfo, ltf + repeat * FFT - BACKOFF);
            for carrier in (-26..=26).filter(|&carrier| carrier != 0) {
                channel[bin(carrier)] += spectrum[bin(carrier)] * 0.5 * long_training(carrier);
            }
        }
        let gain = (-26..=26)
            .filter(|&carrier| carrier != 0)
            .map(|carrier| channel[bin(carrier)].norm_sqr())
            .sum::<f32>()
            / 52.0;
        (gain > 0.0).then_some(Lock {
            origin,
            cfo,
            signal: ltf + 2 * FFT,
            channel,
            gain,
        })
    }

    fn spectrum(&mut self, origin: usize, cfo: f32, at: usize) -> [Complex<f32>; FFT] {
        let mut buffer: [Complex<f32>; FFT] =
            std::array::from_fn(|k| self.sample(origin, cfo, at + k));
        self.fft.forward(&mut buffer);
        buffer
    }

    fn equalized(&mut self, lock: &Lock, index: usize) -> [Complex<f32>; FFT] {
        let at = lock.signal + index * SYMBOL + CP - BACKOFF;
        let mut points = self.spectrum(lock.origin, lock.cfo, at);
        for (point, h) in points.iter_mut().zip(&lock.channel) {
            *point = if h.norm_sqr() > 0.0 {
                *point / h
            } else {
                Complex::default()
            };
        }
        let sign = self.polarity[index % self.polarity.len()];
        let pilot = |carrier: i32, value: f32| {
            points[bin(carrier)] * (value * sign) * lock.channel[bin(carrier)].norm_sqr()
        };
        let pilots: [Complex<f32>; 4] =
            std::array::from_fn(|i| pilot(PILOT_CARRIERS[i], PILOT_VALUES[i]));
        let common = pilots.iter().sum::<Complex<f32>>().arg();
        let unit = |z: Complex<f32>| z / z.norm().max(f32::EPSILON);
        let outer = unit(pilots[3] * pilots[0].conj());
        let inner = unit(pilots[2] * pilots[1].conj()).powi(3);
        let slope = (outer + inner).arg() / 42.0;
        for carrier in -26..=26 {
            points[bin(carrier)] *= Complex::from_polar(1.0, -(common + slope * carrier as f32));
        }
        points
    }

    fn soft_symbol(&mut self, lock: &Lock, index: usize, rate: Rate, table: usize) {
        let points = self.equalized(lock, index);
        let coded = rate.coded_bits();
        demap::demap(
            &points,
            &lock.channel,
            lock.gain,
            rate.modulation,
            &mut self.scratch.received[..coded],
        );
        let scratch = &mut self.scratch;
        scratch.coded.extend(
            scratch.tables[table]
                .iter()
                .map(|&position| demap::soft(scratch.received[position])),
        );
    }

    fn decode(&mut self, lock: &Lock, rate: Rate, symbols: usize, first: usize) {
        let table = RATES
            .iter()
            .position(|candidate| candidate.code == rate.code)
            .unwrap_or(0);
        self.scratch.coded.clear();
        for index in first..first + symbols {
            self.soft_symbol(lock, index, rate, table);
        }
        let scratch = &mut self.scratch;
        scratch.full.clear();
        depuncture(&scratch.coded, rate.coding.puncture(), &mut scratch.full);
        scratch.full.truncate(symbols * rate.data_bits * 2);
        scratch.bits.clear();
        self.viterbi.decode(&scratch.full, &mut scratch.bits);
    }

    fn signal(&mut self, lock: &Lock) -> Option<(Rate, usize)> {
        self.decode(lock, RATES[0], 1, 0);
        let bits = self.scratch.bits.get(..SIGNAL_BITS)?;
        let parity = bits[..18].iter().filter(|&&bit| bit).count() % 2 == 0;
        let tail = bits[18..].iter().all(|&bit| !bit);
        let code = bits[..4]
            .iter()
            .fold(0u8, |acc, &bit| acc << 1 | u8::from(bit));
        let length = bits[5..17]
            .iter()
            .enumerate()
            .fold(0usize, |acc, (bit, &value)| acc | usize::from(value) << bit);
        let rate = rate_for(code)?;
        (parity && tail && !bits[4] && length >= MIN_LENGTH).then_some((rate, length))
    }

    fn demodulate(&mut self, start: usize, sink: &impl Sink) -> Outcome {
        if start + LOOKAHEAD > self.history.len() {
            return Outcome::Short;
        }
        let Some(lock) = self.lock(start) else {
            return Outcome::Failed;
        };
        if lock.signal + SYMBOL > self.history.len() {
            return Outcome::Short;
        }
        let Some((rate, length)) = self.signal(&lock) else {
            return Outcome::Failed;
        };
        let symbols = rate.symbols(length);
        let end = lock.signal + SYMBOL * (1 + symbols);
        if end > self.history.len() {
            return Outcome::Short;
        }
        let peek = PEEK_BITS.div_ceil(rate.data_bits).min(symbols);
        self.decode(&lock, rate, peek, 1);
        descramble(&mut self.scratch.bits);
        let frame_control = self
            .scratch
            .bits
            .get(SERVICE_BITS..SERVICE_BITS + 8)
            .map(byte);
        if !frame_control.is_some_and(|control| sink.accepts(control)) {
            return Outcome::Skip(end);
        }
        self.decode(&lock, rate, symbols, 1);
        let scratch = &mut self.scratch;
        descramble(&mut scratch.bits);
        scratch.bytes.clear();
        scratch.bytes.extend(
            scratch.bits[SERVICE_BITS.min(scratch.bits.len())..]
                .as_chunks::<8>()
                .0
                .iter()
                .take(length)
                .map(|bits| byte(bits)),
        );
        if scratch.bytes.len() != length || !fcs_ok(&scratch.bytes) {
            self.rejected = self.rejected.saturating_add(1);
            return Outcome::Skip(end);
        }
        Outcome::Frame(rate, end)
    }

    fn trim(&mut self) {
        let keep = match self.state {
            State::Pending(start) => start,
            State::Search => self.cursor.saturating_sub(PLATEAU_SAMPLES + 1),
        };
        let drop = keep.max(self.history.len().saturating_sub(MAX_HISTORY));
        if drop == 0 {
            return;
        }
        self.history.drain(..drop);
        self.consumed += drop as u64;
        self.cursor = self.cursor.saturating_sub(drop);
        if let State::Pending(start) = &mut self.state {
            *start = start.saturating_sub(drop);
        }
    }
}

pub(crate) fn descramble(bits: &mut [bool]) {
    let seed = bits.len().min(7);
    let mut register = bits[..seed]
        .iter()
        .fold(0u8, |acc, &bit| acc << 1 | u8::from(bit));
    bits[..seed].fill(false);
    for bit in bits.iter_mut().skip(seed) {
        let next = (register >> 6 ^ register >> 3) & 1;
        register = (register << 1 | next) & 0x7F;
        *bit ^= next == 1;
    }
}

#[cfg(test)]
mod tests;
