use num_complex::Complex;
use sdrmm_dsp::FmDemod;

use super::{
    bfield::{BField, FIELD_BITS},
    mac::{NO_B_FIELD, a_field_crc_ok, field},
};

pub(crate) const BIT_RATE_HZ: f64 = 1_152_000.0;
pub(crate) const SPS: usize = 2;
pub(crate) const INPUT_RATE_HZ: f64 = BIT_RATE_HZ * SPS as f64;
pub(crate) const OCCUPIED_BANDWIDTH_HZ: f64 = 1_728_000.0;
pub(crate) const DEVIATION_HZ: f64 = 288_000.0;
#[cfg(any(test, feature = "synth"))]
pub(crate) const SLOTS_PER_FRAME: u64 = 24;
pub(crate) const FRAME_SAMPLES: u64 = (INPUT_RATE_HZ / 100.0) as u64;

pub(crate) const RFP_SYNC: u32 = 0xAAAA_E98A;
pub(crate) const PP_SYNC: u32 = 0x5555_1675;

const S_FIELD_BITS: usize = 32;
const A_FIELD_BITS: usize = 64;
const BURST_BITS: usize = S_FIELD_BITS + A_FIELD_BITS;
const BURST_SAMPLES: usize = BURST_BITS * SPS;
const PACKET_SAMPLES: usize = (BURST_BITS + FIELD_BITS) * SPS;
const PEAK_SEARCH: usize = 2;
const LOOKAHEAD: usize = PACKET_SAMPLES + PEAK_SEARCH + 2;
const SYNC_SAMPLES: usize = S_FIELD_BITS * SPS;
const PREAMBLE_SAMPLES: usize = 16 * SPS;
const DETECT_THRESHOLD: f32 = 0.55;
const GATE_THRESHOLD: f64 = 0.35;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Burst {
    pub from_rfp: bool,
    pub a_field: u64,
    pub b_field: Option<BField>,
    pub carrier: Option<u8>,
    pub level_dbfs: f32,
    pub score: f32,
    pub sample: u64,
}

fn reference(word: u32) -> [f32; SYNC_SAMPLES] {
    let mut out = [0.0; SYNC_SAMPLES];
    for (index, slot) in out.iter_mut().enumerate() {
        let bit = (word >> (S_FIELD_BITS - 1 - index / SPS)) & 1;
        *slot = if bit == 1 { 1.0 } else { -1.0 };
    }
    out
}

pub(crate) struct Detector {
    demod: FmDemod,
    freq: Vec<f32>,
    history: Vec<f32>,
    power: Vec<f32>,
    rfp_ref: [f32; SYNC_SAMPLES],
    pp_ref: [f32; SYNC_SAMPLES],
    consumed: u64,
    accept_rfp: bool,
    accept_pp: bool,
    carrier: Option<u8>,
}

impl Detector {
    pub fn new(accept_rfp: bool, accept_pp: bool, carrier: Option<u8>) -> Self {
        Self {
            demod: FmDemod::new(INPUT_RATE_HZ, DEVIATION_HZ),
            freq: Vec::new(),
            history: Vec::new(),
            power: Vec::new(),
            rfp_ref: reference(RFP_SYNC),
            pp_ref: reference(PP_SYNC),
            consumed: 0,
            accept_rfp,
            accept_pp,
            carrier,
        }
    }

    pub fn set_sides(&mut self, accept_rfp: bool, accept_pp: bool) {
        self.accept_rfp = accept_rfp;
        self.accept_pp = accept_pp;
    }

    pub fn process(&mut self, iq: &[Complex<f32>], out: &mut Vec<Burst>) {
        self.demod.process(iq, &mut self.freq);
        self.history.extend_from_slice(&self.freq);
        self.power.extend(iq.iter().map(Complex::norm_sqr));
        self.scan(out);
        self.trim();
    }

    fn scan(&mut self, out: &mut Vec<Burst>) {
        if self.history.len() < LOOKAHEAD {
            return;
        }
        let last = self.history.len() - LOOKAHEAD;
        let mut gate = Gate::seat(&self.history, 0);
        let mut start = 0;
        while start <= last {
            if gate.open()
                && let Some(burst) = self.candidate(start)
            {
                out.push(burst);
                start += BURST_SAMPLES;
                if start <= last {
                    gate = Gate::seat(&self.history, start);
                }
                continue;
            }
            start += 1;
            if start <= last {
                gate.advance(&self.history);
            }
        }
        self.drain(start);
    }

    fn score(&self, start: usize) -> (bool, f32) {
        let window = &self.history[start..start + SYNC_SAMPLES];
        let energy = window.iter().map(|&v| v * v).sum::<f32>();
        if energy <= f32::EPSILON {
            return (true, f32::MIN);
        }
        let norm = (energy * SYNC_SAMPLES as f32).sqrt();
        let rfp = if self.accept_rfp {
            correlate(window, &self.rfp_ref) / norm
        } else {
            f32::MIN
        };
        let pp = if self.accept_pp {
            correlate(window, &self.pp_ref) / norm
        } else {
            f32::MIN
        };
        if rfp >= pp { (true, rfp) } else { (false, pp) }
    }

    fn peak(&self, start: usize, from_rfp: bool, score: f32) -> (usize, f32, f32) {
        let side_score = |at: usize| {
            let (rfp, value) = self.score(at);
            if rfp == from_rfp { value } else { f32::MIN }
        };
        let (best, best_score) = (start + 1..=start + PEAK_SEARCH)
            .map(|at| (at, side_score(at)))
            .fold(
                (start, score),
                |acc, next| if next.1 > acc.1 { next } else { acc },
            );
        let before = if best > 0 {
            side_score(best - 1)
        } else {
            best_score
        };
        let after = side_score(best + 1);
        let curvature = before - 2.0 * best_score + after;
        let fraction = if curvature < 0.0 {
            (0.5 * (before - after) / curvature).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        (best, fraction, best_score)
    }

    fn candidate(&self, start: usize) -> Option<Burst> {
        let (from_rfp, score) = self.score(start);
        if score < DETECT_THRESHOLD {
            return None;
        }
        let (start, fraction, score) = self.peak(start, from_rfp, score);
        let offset = self.dc(start);
        let a_field = (0..A_FIELD_BITS).fold(0u64, |acc, index| {
            (acc << 1) | u64::from(self.bit(start, fraction, S_FIELD_BITS + index, offset))
        });
        let b_field = (a_field_crc_ok(a_field) && field(a_field, 4, 3) != NO_B_FIELD).then(|| {
            BField::from_bits(
                (0..FIELD_BITS).map(|index| self.bit(start, fraction, BURST_BITS + index, offset)),
            )
        });
        Some(Burst {
            from_rfp,
            a_field,
            b_field,
            carrier: self.carrier,
            level_dbfs: self.level(start),
            score,
            sample: self.consumed + start as u64,
        })
    }

    fn bit(&self, start: usize, fraction: f32, index: usize, offset: f32) -> bool {
        let position = start as f32 + fraction + 0.5 + (index * SPS) as f32;
        let at = position.floor().max(0.0) as usize;
        let weight = position - at as f32;
        let value = self.history[at] * (1.0 - weight) + self.history[at + 1] * weight;
        value - offset > 0.0
    }

    fn dc(&self, start: usize) -> f32 {
        let window = &self.history[start..start + SYNC_SAMPLES];
        window.iter().sum::<f32>() / SYNC_SAMPLES as f32
    }

    fn level(&self, start: usize) -> f32 {
        let window = &self.power[start..start + BURST_SAMPLES];
        let mean = window.iter().sum::<f32>() / BURST_SAMPLES as f32;
        10.0 * mean.max(1e-20).log10()
    }

    fn drain(&mut self, upto: usize) {
        if upto == 0 {
            return;
        }
        self.history.drain(..upto);
        self.power.drain(..upto);
        self.consumed += upto as u64;
    }

    fn trim(&mut self) {
        const LIMIT: usize = PACKET_SAMPLES * 16;
        if self.history.len() <= LIMIT {
            return;
        }
        let excess = self.history.len() - LIMIT;
        self.drain(excess);
    }
}

struct Gate {
    sum_re: f64,
    sum_im: f64,
    magnitude: f64,
    at: usize,
}

impl Gate {
    fn seat(history: &[f32], at: usize) -> Self {
        let mut sum_re = 0.0;
        let mut sum_im = 0.0;
        let mut magnitude = 0.0;
        for step in 0..PREAMBLE_SAMPLES {
            let x = f64::from(history[at + step]);
            match step % 4 {
                0 => sum_re += x,
                1 => sum_im -= x,
                2 => sum_re -= x,
                _ => sum_im += x,
            }
            magnitude += x.abs();
        }
        Self {
            sum_re,
            sum_im,
            magnitude,
            at,
        }
    }

    fn advance(&mut self, history: &[f32]) {
        let leaving = f64::from(history[self.at]);
        let entering = f64::from(history[self.at + PREAMBLE_SAMPLES]);
        let re = self.sum_re - leaving + entering;
        let im = self.sum_im;
        self.sum_re = -im;
        self.sum_im = re;
        self.magnitude += entering.abs() - leaving.abs();
        self.at += 1;
    }

    fn open(&self) -> bool {
        (self.sum_re * self.sum_re + self.sum_im * self.sum_im).sqrt()
            >= GATE_THRESHOLD * self.magnitude
    }
}

fn correlate(window: &[f32], reference: &[f32]) -> f32 {
    window
        .iter()
        .zip(reference)
        .map(|(&x, &r)| x * r)
        .sum::<f32>()
}
