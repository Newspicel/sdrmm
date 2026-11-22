mod demap;
mod plateau;
pub mod tables;

use std::f32::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::{
    ConvCode, Soft, ViterbiK7, depuncture,
    math::{complex::cross_sum, fft::FftPair},
};

use self::{
    plateau::{PLATEAU_SAMPLES, PROBE, Plateau, Probe, WINDOW},
    tables::{
        CONV_POLYS, CP, DATA_CARRIERS, EDGE, FFT, Modulation, OCCUPIED, PILOT_CARRIERS,
        PILOT_VALUES, POLARITY_LEN, RATES, Rate, SERVICE_BITS, SHORT, SYMBOL, bin, data_carriers,
        interleave_table, long_training, occupied_carriers, polarity, rate_for,
    },
};
use super::{
    Frame, Grid, MAX_MPDU_BYTES, RATE_HZ, Sink, WifiPhy, byte, fcs_ok, level_dbfs, points::Cloud,
    scrambler::descramble,
};

pub use self::demap::map as map_point;

const LTF_SEARCH: usize = 320;
const BACKOFF: usize = 3;
const SIGNAL_BITS: usize = 24;
const RATE_BITS: usize = 4;
const LENGTH_BITS: usize = 12;
/// SIGNAL bits the even parity covers: rate, reserved bit and length.
const PARITY_BITS: usize = RATE_BITS + 1 + LENGTH_BITS + 1;
const LONG_TRAINING_REPEATS: usize = 2;
/// The outer pilot pair spans three times the inner pair's carriers.
const PILOT_SPAN_RATIO: i32 = 3;
const MIN_LENGTH: usize = 14;
const LOOKAHEAD: usize = WINDOW + SHORT + LTF_SEARCH + 2 * FFT + SYMBOL;
const MAX_HISTORY: usize = 600_000;
const PEEK_BITS: usize = SERVICE_BITS + 16 + 96;
const MAX_CODED_BITS: usize = 288;
const MAX_SYMBOLS: usize = 1_366;
const MAX_DATA_BITS: usize = 24 * MAX_SYMBOLS;
const POINT_RATE_HZ: f64 = RATE_HZ / SYMBOL as f64 * DATA_CARRIERS as f64;
const MAX_CODED_STREAM: usize = 66_000;

#[derive(Clone, Copy)]
enum State {
    Search,
    Pending(usize),
}

enum Outcome {
    Frame(Rate, usize),
    Skip(usize),
    Wait(usize),
    Failed,
}

struct Scratch {
    derotated: [Complex<f32>; LTF_SEARCH + 2 * FFT],
    scores: [f32; LTF_SEARCH + FFT],
    received: [f32; MAX_CODED_BITS],
    coded: Vec<Soft>,
    full: Vec<Soft>,
    bits: Vec<bool>,
    bytes: Vec<u8>,
    tables: Vec<Vec<usize>>,
    cloud: Cloud,
}

impl Scratch {
    fn new() -> Self {
        Self {
            derotated: [Complex::new(0.0, 0.0); LTF_SEARCH + 2 * FFT],
            scores: [0.0; LTF_SEARCH + FFT],
            received: [0.0; MAX_CODED_BITS],
            coded: Vec::with_capacity(MAX_CODED_STREAM),
            full: Vec::with_capacity(2 * MAX_DATA_BITS + MAX_CODED_BITS),
            bits: Vec::with_capacity(MAX_DATA_BITS + MAX_CODED_BITS),
            bytes: Vec::with_capacity(MAX_MPDU_BYTES),
            tables: RATES.iter().map(|&rate| interleave_table(rate)).collect(),
            cloud: Cloud::across_carriers(MAX_SYMBOLS * DATA_CARRIERS),
        }
    }
}

pub struct Ofdm {
    history: Vec<Complex<f32>>,
    plateau: Plateau,
    probe: Probe,
    consumed: u64,
    cursor: usize,
    ready_at: usize,
    state: State,
    fft: FftPair,
    reference: [Complex<f32>; FFT],
    polarity: [f32; POLARITY_LEN],
    viterbi: ViterbiK7,
    scratch: Scratch,
    rejected: u32,
}

struct Lock {
    origin: usize,
    cfo: f32,
    signal: usize,
    channel: [Complex<f32>; FFT],
    channel_inv: [Complex<f32>; FFT],
    channel_power: [f32; FFT],
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
        for carrier in -EDGE..=EDGE {
            reference[bin(carrier)] = Complex::new(long_training(carrier), 0.0);
        }
        fft.inverse(&mut reference);
        Self {
            history: Vec::with_capacity(MAX_HISTORY),
            plateau: Plateau::default(),
            probe: Probe::default(),
            consumed: 0,
            cursor: 0,
            ready_at: 0,
            state: State::Search,
            fft,
            reference,
            polarity: polarity(),
            viterbi: ViterbiK7::with_capacity(
                ConvCode::new(&CONV_POLYS),
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
        self.ready_at = 0;
        self.state = State::Search;
        self.plateau = Plateau::default();
        self.probe = Probe::default();
    }

    pub fn process(&mut self, iq: &[Complex<f32>], sink: &mut impl Sink) {
        self.history.extend_from_slice(iq);
        loop {
            match self.state {
                State::Pending(_) if self.history.len() < self.ready_at => break,
                State::Pending(start) => match self.demodulate(start, sink) {
                    Outcome::Wait(needed) => {
                        self.ready_at = needed;
                        break;
                    }
                    Outcome::Frame(rate, end) => {
                        sink.frame(Frame {
                            mpdu: &self.scratch.bytes,
                            phy: WifiPhy::Ofdm {
                                mbps: rate.mbps as u8,
                            },
                            level_dbfs: level_dbfs(&self.history[start..end]),
                            sample: self.consumed + start as u64,
                        });
                        self.scratch.cloud.hand(sink);
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
        self.ready_at = 0;
        if at > self.cursor {
            self.cursor = at;
            self.plateau.primed = false;
        }
        self.state = State::Search;
        self.plateau.run = 0;
        self.plateau.holding = holding;
    }

    /// Slides the plateau detector on. A fresh run has to pass every sample of its first
    /// `PLATEAU_SAMPLES`, so one failing grid probe among them lets the search jump past it.
    fn search(&mut self) -> bool {
        let mut plateau = self.plateau;
        let mut cursor = self.cursor;
        let mut found = None;
        while cursor + LOOKAHEAD <= self.history.len() {
            if plateau.run == 0 {
                let absolute = self.consumed + cursor as u64;
                let grid = absolute.next_multiple_of(PROBE as u64);
                let probe = cursor + (grid - absolute) as usize;
                if !self.probe.on(&self.history, probe, grid) {
                    cursor = probe + 1;
                    plateau.primed = false;
                    plateau.holding = false;
                    continue;
                }
            }
            let at = cursor;
            plateau.slide(&self.history, at);
            cursor += 1;
            if plateau.step() {
                found = Some((at + 1).saturating_sub(PLATEAU_SAMPLES));
                break;
            }
        }
        self.plateau = plateau;
        self.cursor = cursor;
        let Some(start) = found else {
            return false;
        };
        self.state = State::Pending(start);
        true
    }

    fn lock(&mut self, origin: usize) -> Option<Lock> {
        let coarse = cross_sum(
            &self.history[origin..origin + WINDOW],
            &self.history[origin + SHORT..origin + SHORT + WINDOW],
        );
        let coarse_cfo = -coarse.arg() as f32 / (TAU * SHORT as f32);
        let step = Complex::from_polar(1.0f32, -TAU * coarse_cfo);
        let mut rot = Complex::new(1.0f32, 0.0);
        for (dst, &sample) in self
            .scratch
            .derotated
            .iter_mut()
            .zip(&self.history[origin..origin + LTF_SEARCH + 2 * FFT])
        {
            *dst = sample * rot;
            rot *= step;
        }
        for offset in 0..LTF_SEARCH + FFT {
            self.scratch.scores[offset] = cross_sum(
                &self.scratch.derotated[offset..offset + FFT],
                &self.reference,
            )
            .norm() as f32;
        }
        let scores = &self.scratch.scores;
        let first = (0..LTF_SEARCH).max_by(|&a, &b| {
            (scores[a] + scores[a + FFT]).total_cmp(&(scores[b] + scores[b + FFT]))
        })?;
        let ltf = origin + first;
        let fine = cross_sum(
            &self.scratch.derotated[first..first + FFT],
            &self.scratch.derotated[first + FFT..first + 2 * FFT],
        );
        let cfo = coarse_cfo - fine.arg() as f32 / (TAU * FFT as f32);
        let mut channel = [Complex::new(0.0, 0.0); FFT];
        for repeat in 0..LONG_TRAINING_REPEATS {
            let spectrum = self.spectrum(origin, cfo, ltf + repeat * FFT - BACKOFF);
            for carrier in occupied_carriers() {
                channel[bin(carrier)] += spectrum[bin(carrier)] * 0.5 * long_training(carrier);
            }
        }
        let mut channel_inv = [Complex::new(0.0, 0.0); FFT];
        let mut channel_power = [0.0f32; FFT];
        for k in 0..FFT {
            let p = channel[k].norm_sqr();
            channel_power[k] = p;
            if p > 0.0 {
                channel_inv[k] = channel[k].conj() / p;
            }
        }
        let gain = occupied_carriers()
            .map(|carrier| channel_power[bin(carrier)])
            .sum::<f32>()
            / OCCUPIED as f32;
        (gain > 0.0).then_some(Lock {
            origin,
            cfo,
            signal: ltf + LONG_TRAINING_REPEATS * FFT,
            channel,
            channel_inv,
            channel_power,
            gain,
        })
    }

    fn spectrum(&mut self, origin: usize, cfo: f32, at: usize) -> [Complex<f32>; FFT] {
        let mut buffer = [Complex::new(0.0f32, 0.0); FFT];
        let phase0 = -TAU * cfo * (at as f32 - origin as f32);
        let mut rot = Complex::from_polar(1.0f32, phase0);
        let step = Complex::from_polar(1.0f32, -TAU * cfo);
        for (dst, &sample) in buffer.iter_mut().zip(&self.history[at..at + FFT]) {
            *dst = sample * rot;
            rot *= step;
        }
        self.fft.forward(&mut buffer);
        buffer
    }

    fn equalized(&mut self, lock: &Lock, index: usize) -> [Complex<f32>; FFT] {
        let at = lock.signal + index * SYMBOL + CP - BACKOFF;
        let mut points = self.spectrum(lock.origin, lock.cfo, at);
        for (point, &h_inv) in points.iter_mut().zip(&lock.channel_inv) {
            *point *= h_inv;
        }
        let sign = self.polarity[index % self.polarity.len()];
        let pilot = |carrier: i32, value: f32| {
            points[bin(carrier)] * (value * sign) * lock.channel_power[bin(carrier)]
        };
        let pilots: [Complex<f32>; 4] =
            std::array::from_fn(|i| pilot(PILOT_CARRIERS[i], PILOT_VALUES[i]));
        let common = pilots.iter().sum::<Complex<f32>>().arg();
        let unit = |z: Complex<f32>| z / z.norm().max(f32::EPSILON);
        let outer = unit(pilots[3] * pilots[0].conj());
        let inner = unit(pilots[2] * pilots[1].conj()).powi(PILOT_SPAN_RATIO);
        let slope = (outer + inner).arg() / (PILOT_CARRIERS[3] - PILOT_CARRIERS[0]) as f32;
        let mut rot = Complex::from_polar(1.0f32, -(common - slope * EDGE as f32));
        let step = Complex::from_polar(1.0f32, -slope);
        for carrier in -EDGE..=EDGE {
            points[bin(carrier)] *= rot;
            rot *= step;
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
        if index > 0 {
            scratch
                .cloud
                .extend(data_carriers().map(|carrier| points[bin(carrier)]));
        }
    }

    fn decode(&mut self, lock: &Lock, rate: Rate, symbols: usize, first: usize, already: usize) {
        let table = RATES
            .iter()
            .position(|candidate| candidate.code == rate.code)
            .unwrap_or(0);
        if already == 0 {
            self.scratch.coded.clear();
            self.scratch
                .cloud
                .start(grid(rate.modulation), POINT_RATE_HZ);
        }
        for index in first + already..first + symbols {
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
        self.decode(lock, RATES[0], 1, 0, 0);
        let bits = self.scratch.bits.get(..SIGNAL_BITS)?;
        let parity = bits[..PARITY_BITS].iter().filter(|&&bit| bit).count() % 2 == 0;
        let tail = bits[PARITY_BITS..].iter().all(|&bit| !bit);
        let code = bits[..RATE_BITS]
            .iter()
            .fold(0u8, |acc, &bit| acc << 1 | u8::from(bit));
        let length = bits[RATE_BITS + 1..RATE_BITS + 1 + LENGTH_BITS]
            .iter()
            .enumerate()
            .fold(0usize, |acc, (bit, &value)| acc | usize::from(value) << bit);
        let rate = rate_for(code)?;
        (parity && tail && !bits[RATE_BITS] && length >= MIN_LENGTH).then_some((rate, length))
    }

    fn demodulate(&mut self, start: usize, sink: &impl Sink) -> Outcome {
        if start + LOOKAHEAD > self.history.len() {
            return Outcome::Wait(start + LOOKAHEAD);
        }
        let Some(lock) = self.lock(start) else {
            return Outcome::Failed;
        };
        if lock.signal + SYMBOL > self.history.len() {
            return Outcome::Wait(lock.signal + SYMBOL);
        }
        let Some((rate, length)) = self.signal(&lock) else {
            return Outcome::Failed;
        };
        let symbols = rate.symbols(length);
        let end = lock.signal + SYMBOL * (1 + symbols);
        if end > self.history.len() {
            return Outcome::Wait(end);
        }
        let peek = PEEK_BITS.div_ceil(rate.data_bits).min(symbols);
        self.decode(&lock, rate, peek, 1, 0);
        descramble(&mut self.scratch.bits);
        let frame_control = self
            .scratch
            .bits
            .get(SERVICE_BITS..SERVICE_BITS + 8)
            .map(byte);
        if !frame_control.is_some_and(|control| sink.accepts(control)) {
            return Outcome::Skip(end);
        }
        if peek < symbols {
            self.decode(&lock, rate, symbols, 1, peek);
            descramble(&mut self.scratch.bits);
        }
        let scratch = &mut self.scratch;
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
        scratch.cloud.heard_at(f64::from(lock.cfo) * RATE_HZ);
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
        self.ready_at = self.ready_at.saturating_sub(drop);
        if let State::Pending(start) = &mut self.state {
            *start = start.saturating_sub(drop);
        }
    }
}

fn grid(modulation: Modulation) -> Grid {
    match modulation {
        Modulation::Bpsk => Grid::Bpsk,
        Modulation::Qpsk => Grid::Qpsk,
        Modulation::Qam16 => Grid::Qam16,
        Modulation::Qam64 => Grid::Qam64,
    }
}

#[cfg(test)]
mod tests;
