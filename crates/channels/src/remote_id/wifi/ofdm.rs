mod demap;
pub(crate) mod tables;

use std::f32::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::{ConvCode, Soft, ViterbiK7, depuncture, fft::FftPair};
use sdrmm_wire::RemoteIdPhy;

use self::tables::{
    CP, FFT, PILOT_CARRIERS, PILOT_VALUES, Rate, SERVICE_BITS, SHORT, SYMBOL, bin,
    interleave_table, long_training, polarity, rate_for,
};
use super::receiver::{Burst, level_dbfs};

#[cfg(any(test, feature = "synth"))]
pub(crate) use self::demap::map as map_point;

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

#[derive(Clone, Copy)]
enum State {
    Search,
    Pending(usize),
}

pub(crate) struct Ofdm {
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
    pub(crate) rejected: u32,
}

enum Outcome {
    Burst(Burst, usize),
    Skip(usize),
    Short,
    Failed,
}

struct Lock {
    origin: usize,
    cfo: f32,
    signal: usize,
    channel: [Complex<f32>; FFT],
    gain: f32,
}

impl Ofdm {
    pub(crate) fn new() -> Self {
        let mut fft = FftPair::new(FFT);
        let mut reference = [Complex::new(0.0, 0.0); FFT];
        for carrier in -26..=26 {
            reference[bin(carrier)] = Complex::new(long_training(carrier), 0.0);
        }
        fft.inverse(&mut reference);
        Self {
            history: Vec::new(),
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
            viterbi: ViterbiK7::new(ConvCode::new(&[0o133, 0o171])),
            rejected: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.history.clear();
        self.consumed = 0;
        self.cursor = 0;
        self.state = State::Search;
        self.primed = false;
        self.run = 0;
        self.holding = false;
    }

    pub(crate) fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<Burst>) {
        self.history.extend_from_slice(iq);
        loop {
            match self.state {
                State::Pending(start) => match self.demodulate(start) {
                    Outcome::Short => break,
                    Outcome::Burst(burst, end) => {
                        out.push(burst);
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
        let scores: Vec<f32> = (0..LTF_SEARCH + FFT)
            .map(|offset| {
                (0..FFT)
                    .map(|k| {
                        self.sample(origin, coarse_cfo, origin + offset + k)
                            * self.reference[k].conj()
                    })
                    .sum::<Complex<f32>>()
                    .norm()
            })
            .collect();
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

    fn soft_symbol(
        &mut self,
        lock: &Lock,
        index: usize,
        rate: Rate,
        table: &[usize],
        out: &mut Vec<Soft>,
    ) {
        let points = self.equalized(lock, index);
        let mut received = vec![0.0f32; rate.coded_bits()];
        demap::demap(
            &points,
            &lock.channel,
            lock.gain,
            rate.modulation,
            &mut received,
        );
        out.extend(
            table
                .iter()
                .map(|&position| demap::soft(received[position])),
        );
    }

    fn decode(&mut self, lock: &Lock, rate: Rate, symbols: usize, first: usize) -> Vec<bool> {
        let table = interleave_table(rate);
        let mut coded = Vec::with_capacity(symbols * rate.coded_bits());
        for index in first..first + symbols {
            self.soft_symbol(lock, index, rate, &table, &mut coded);
        }
        let mut full = Vec::with_capacity(coded.len() * 2);
        depuncture(&coded, rate.coding.puncture(), &mut full);
        full.truncate(symbols * rate.data_bits * 2);
        let mut bits = Vec::with_capacity(symbols * rate.data_bits);
        self.viterbi.decode(&full, &mut bits);
        bits
    }

    fn signal(&mut self, lock: &Lock) -> Option<(Rate, usize)> {
        let bits = self.decode(lock, tables::RATES[0], 1, 0);
        let bits = bits.get(..SIGNAL_BITS)?;
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

    fn demodulate(&mut self, start: usize) -> Outcome {
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
        let frame_control = descramble(&self.decode(&lock, rate, peek, 1))
            .get(SERVICE_BITS..SERVICE_BITS + 8)
            .map(byte);
        if !frame_control.is_some_and(super::wanted_control) {
            return Outcome::Skip(end);
        }
        let bits = descramble(&self.decode(&lock, rate, symbols, 1));
        let bytes: Vec<u8> = bits[SERVICE_BITS..]
            .as_chunks::<8>()
            .0
            .iter()
            .take(length)
            .map(|bits| byte(bits))
            .collect();
        if bytes.len() != length || !super::fcs_ok(&bytes) {
            self.rejected = self.rejected.saturating_add(1);
            return Outcome::Skip(end);
        }
        Outcome::Burst(
            Burst {
                bytes,
                phy: RemoteIdPhy::Ofdm,
                level_dbfs: level_dbfs(&self.history[start..end]),
                sample: self.consumed + start as u64,
            },
            end,
        )
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

fn descramble(bits: &[bool]) -> Vec<bool> {
    let mut sequence: Vec<bool> = bits.iter().take(7).copied().collect();
    let mut out = vec![false; bits.len().min(7)];
    for (n, &bit) in bits.iter().enumerate().skip(7) {
        let next = sequence[n - 7] ^ sequence[n - 4];
        sequence.push(next);
        out.push(bit ^ next);
    }
    out
}

fn byte(bits: &[bool]) -> u8 {
    bits.iter()
        .enumerate()
        .fold(0u8, |acc, (bit, &value)| acc | u8::from(value) << bit)
}

#[cfg(test)]
mod tests;
