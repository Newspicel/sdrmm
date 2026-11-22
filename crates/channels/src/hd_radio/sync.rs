use std::f32::consts::{PI, TAU};

use num_complex::Complex;
use sdrmm_dsp::Soft;

use super::{
    acquire::Tracking,
    layout::{
        BLOCK, CENTRE, CP, DATA_CARRIERS, FFT, LOWER_EDGE, MAIN_BITS, MAIN_PARTITIONS,
        MAX_PARTITIONS, PARTITION, UPPER_EDGE,
    },
};

/// The reference subcarrier ID of the subcarrier at the middle of the sideband's table.
const MIDDLE_REFERENCE: usize = 30;
const LOOP_BANDWIDTH: f32 = 0.05;
const DAMPING: f32 = std::f32::consts::FRAC_1_SQRT_2;
const MAX_LOOP_FREQUENCY: f32 = 0.5;
/// References that must agree before a block's control word is trusted.
const MIN_REFERENCES: usize = 4;
const MIN_CFO_VOTES: usize = 3;
/// Blocks to wait after moving the carriers before measuring again.
const CFO_WAIT: u8 = 8;
const MAX_SOFT: f32 = 127.0;

/// The differentially coded sync and parity pattern every reference block opens with.
const SYNC: [i8; BLOCK] = [
    -1, 1, -1, -1, -1, 1, 1, 0, 1, -1, 0, 0, 0, -1, -1, 0, 0, 0, 0, 0, -1, 1, -1, 0, 0, 0, 0, 0, 0,
    0, 0, -1,
];

/// Partitions per sideband for each primary service mode's backwards compatibility class.
const fn partitions(psmi: u8) -> usize {
    match compatibility(psmi) {
        2 => 11,
        3 => 12,
        5 | 6 | 11 => 14,
        _ => MAIN_PARTITIONS,
    }
}

/// NRSC-5 table 6-4: the mode an older receiver may treat each service mode as.
const fn compatibility(psmi: u8) -> u8 {
    const TABLE: [u8; 16] = [0, 1, 2, 3, 1, 5, 6, 5, 6, 1, 2, 11, 1, 5, 6, 5];
    if psmi == 0 {
        0
    } else {
        TABLE[(psmi % 16) as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Searching,
    Coarse,
    Locked,
}

/// What a block taught the tracker.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Outcome {
    /// The block's index in its P1 frame, when its soft bits are in `Sync::soft`.
    pub block: Option<usize>,
    /// Whole subcarriers to move by and symbols to delay, found by the reference search.
    pub shift: Option<(i32, usize)>,
}

/// Tracks the reference subcarriers block by block: their phase, the sync pattern that marks
/// where a block starts, the block count and service mode they carry, and from them the
/// equalised data subcarriers as soft bits.
pub struct Sync {
    pub bins: Vec<Complex<f32>>,
    phases: Vec<f32>,
    loop_phase: Vec<f32>,
    loop_frequency: Vec<f32>,
    alpha: f32,
    beta: f32,
    pub state: State,
    pub psmi: u8,
    pub block: usize,
    pub tracking: Option<Tracking>,
    cfo_wait: u8,
    pub mer_db: f32,
    pub soft: Vec<Soft>,
}

impl Sync {
    pub fn new() -> Self {
        let denominator = 1.0 + 2.0 * DAMPING * LOOP_BANDWIDTH + LOOP_BANDWIDTH * LOOP_BANDWIDTH;
        Self {
            bins: vec![Complex::default(); FFT * BLOCK],
            phases: vec![0.0; FFT * BLOCK],
            loop_phase: vec![0.0; FFT],
            loop_frequency: vec![0.0; FFT],
            alpha: 4.0 * DAMPING * LOOP_BANDWIDTH / denominator,
            beta: 4.0 * LOOP_BANDWIDTH * LOOP_BANDWIDTH / denominator,
            state: State::Searching,
            psmi: 1,
            block: 0,
            tracking: None,
            cfo_wait: 0,
            mer_db: 0.0,
            soft: vec![0; MAIN_BITS],
        }
    }

    pub fn reset(&mut self) {
        self.loop_phase.fill(0.0);
        self.loop_frequency.fill(0.0);
        self.state = State::Searching;
        self.psmi = 1;
        self.block = 0;
        self.tracking = None;
        self.cfo_wait = 0;
        self.mer_db = 0.0;
    }

    /// Turns each tracked subcarrier's phase for a block that starts `samples` later.
    pub fn retime(&mut self, samples: i32) {
        for i in 0..=MAX_PARTITIONS * PARTITION {
            for carrier in [LOWER_EDGE + i, UPPER_EDGE - i] {
                self.loop_phase[carrier] -=
                    samples as f32 * (carrier as f32 - CENTRE as f32) * TAU / FFT as f32;
            }
        }
    }

    fn references(&self) -> impl Iterator<Item = (usize, usize)> + use<> {
        let count = partitions(self.psmi);
        (0..=count).flat_map(|i| {
            let id = (MIDDLE_REFERENCE - i) & 3;
            [
                (LOWER_EDGE + i * PARTITION, id),
                (UPPER_EDGE - i * PARTITION, id),
            ]
        })
    }

    /// Processes the block now in `bins`.
    pub fn process(&mut self) -> Outcome {
        let mut outcome = Outcome::default();
        for (carrier, _) in self.references() {
            self.track_reference(carrier, 0);
        }
        if self.state == State::Coarse {
            if self.find_control() {
                self.state = State::Locked;
            } else if self.cfo_wait == 0 {
                outcome.shift = self.detect_cfo();
            } else {
                self.cfo_wait -= 1;
            }
        }
        if self.state == State::Locked {
            self.equalise_and_track();
            self.demodulate();
            outcome.block = Some(self.block);
            self.block = (self.block + 1) % super::layout::BLOCKS;
        } else {
            self.tracking = None;
        }
        outcome
    }

    fn at(carrier: usize, symbol: usize) -> usize {
        carrier * BLOCK + symbol
    }

    /// A Costas loop along one reference subcarrier's block, then the half turn its sync
    /// pattern resolves.
    fn track_reference(&mut self, carrier: usize, cfo: i32) {
        let step = TAU * cfo as f32 * CP as f32 / FFT as f32;
        for symbol in 0..BLOCK {
            let at = Self::at(carrier, symbol);
            let phase = self.loop_phase[carrier];
            let error = (self.bins[at] * self.bins[at] * Complex::from_polar(1.0, -2.0 * phase))
                .arg()
                * 0.5;
            self.phases[at] = phase;
            self.bins[at] *= Complex::from_polar(1.0, -phase);
            let frequency = (self.loop_frequency[carrier] + self.beta * error)
                .clamp(-MAX_LOOP_FREQUENCY, MAX_LOOP_FREQUENCY);
            self.loop_frequency[carrier] = frequency;
            self.loop_phase[carrier] = wrap(phase + frequency + step + self.alpha * error);
        }
        let agreement: f32 = (0..BLOCK)
            .map(|symbol| self.bins[Self::at(carrier, symbol)].re * f32::from(SYNC[symbol]))
            .sum();
        if agreement < 0.0 {
            for symbol in 0..BLOCK {
                let at = Self::at(carrier, symbol);
                self.phases[at] += PI;
                self.bins[at] = -self.bins[at];
            }
            self.loop_phase[carrier] += PI;
        }
    }

    fn untrack_reference(&mut self, carrier: usize) {
        for symbol in 0..BLOCK {
            let at = Self::at(carrier, symbol);
            self.bins[at] *= Complex::from_polar(1.0, self.phases[at]);
        }
    }

    fn hard_bits(&self, carrier: usize) -> [u8; BLOCK] {
        std::array::from_fn(|symbol| u8::from(self.bins[Self::at(carrier, symbol)].re > 0.0))
    }

    /// Reads the block count and service mode from every reference whose sync pattern holds,
    /// and locks once a majority agree.
    fn find_control(&mut self) -> bool {
        let mut good = 0usize;
        let mut counts = [0usize; 16];
        let mut modes = [0usize; 64];
        for (carrier, id) in self.references() {
            let bits = self.hard_bits(carrier);
            if !matches(&needle(id), &bits, 0) {
                continue;
            }
            let data = differential(&bits);
            let count = data[16..20].iter().fold(0, |acc, &bit| acc << 1 | bit);
            let mode = data[25..31].iter().fold(0, |acc, &bit| acc << 1 | bit);
            good += 1;
            counts[usize::from(count)] += 1;
            modes[usize::from(mode)] += 1;
        }
        if good < MIN_REFERENCES {
            return false;
        }
        let majority = |votes: &[usize]| votes.iter().position(|&n| n > good / 2);
        match (majority(&counts), majority(&modes[..16])) {
            (Some(count), Some(mode)) => {
                self.block = count;
                self.psmi = mode as u8;
                true
            }
            _ => false,
        }
    }

    /// Tries each whole-subcarrier offset until enough references show the sync pattern at
    /// one shared position, which is also where the block starts.
    fn detect_cfo(&mut self) -> Option<(i32, usize)> {
        let reach = 2 * PARTITION as i32;
        for cfo in -reach..reach {
            let mut votes = [0usize; BLOCK];
            for i in 0..=MAIN_PARTITIONS {
                let id = (MIDDLE_REFERENCE - i) & 3;
                for edge in [
                    LOWER_EDGE as i32 + (i * PARTITION) as i32,
                    UPPER_EDGE as i32 - (i * PARTITION) as i32,
                ] {
                    let carrier = (edge + cfo) as usize;
                    self.track_reference(carrier, cfo);
                    if let Some(offset) = self.find_reference(carrier, id) {
                        votes[offset] += 1;
                    }
                    self.untrack_reference(carrier);
                }
            }
            let (offset, &count) = votes
                .iter()
                .enumerate()
                .max_by_key(|&(offset, &count)| (count, std::cmp::Reverse(offset)))?;
            if count >= MIN_CFO_VOTES {
                self.cfo_wait = CFO_WAIT;
                return Some((cfo, (BLOCK - offset) % BLOCK));
            }
        }
        None
    }

    fn find_reference(&self, carrier: usize, id: usize) -> Option<usize> {
        let pattern = needle(id);
        let mut bits = self.hard_bits(carrier);
        (0..BLOCK)
            .find(|&offset| matches(&pattern, &bits, offset))
            .or_else(|| {
                for bit in &mut bits {
                    *bit ^= 1;
                }
                (0..BLOCK).find(|&offset| matches(&pattern, &bits, offset))
            })
    }

    /// Equalises each partition's data subcarriers between its two references, and measures
    /// the timing and frequency drift the references show.
    fn equalise_and_track(&mut self) {
        let count = partitions(self.psmi);
        let mut samples = 0.0f32;
        for p in 0..count {
            let lower = LOWER_EDGE + p * PARTITION;
            self.equalise(lower, lower + PARTITION);
            let upper = UPPER_EDGE - p * PARTITION;
            self.equalise(upper - PARTITION, upper);
            samples += half_turn(
                self.phases[Self::at(lower, 0)],
                self.phases[Self::at(lower + PARTITION, 0)],
            );
            samples += half_turn(
                self.phases[Self::at(upper - PARTITION, 0)],
                self.phases[Self::at(upper, 0)],
            );
        }
        samples = samples / (count * 2) as f32 * FFT as f32 / PARTITION as f32 / TAU;
        let (mut angle, mut sum_xy, mut sum_x2) = (0.0f32, 0.0f32, 0.0f32);
        for (carrier, _) in self.references() {
            let x = carrier as f32 - CENTRE as f32;
            let y = self.loop_frequency[carrier];
            angle += y;
            sum_xy += x * y;
            sum_x2 += x * x;
        }
        samples -= sum_xy / sum_x2 * FFT as f32 / TAU * BLOCK as f32;
        angle /= ((count + 1) * 2) as f32;
        for (carrier, _) in self.references() {
            self.loop_frequency[carrier] -= angle;
        }
        self.tracking = Some(Tracking {
            samples: samples.round() as i32,
            angle,
        });
    }

    fn magnitude(&self, carrier: usize) -> f32 {
        (0..BLOCK)
            .map(|symbol| self.bins[Self::at(carrier, symbol)].re.abs())
            .sum::<f32>()
            / BLOCK as f32
    }

    fn equalise(&mut self, lower: usize, upper: usize) {
        let (low, high) = (self.magnitude(lower), self.magnitude(upper));
        let width = PARTITION as f32;
        for symbol in 0..BLOCK {
            let up = Complex::from_polar(high, self.phases[Self::at(upper, symbol)]);
            let down = Complex::from_polar(low, self.phases[Self::at(lower, symbol)]);
            for k in 1..PARTITION {
                let weight = up * k as f32 + down * (width - k as f32);
                self.bins[Self::at(lower + k, symbol)] *= Complex::new(width, width) / weight;
            }
        }
    }

    /// Slices the primary main data subcarriers to soft bits, scaled by each sideband's
    /// modulation error, in the order the P1 deinterleaver reads them.
    fn demodulate(&mut self) {
        let count = partitions(self.psmi);
        let error = |sync: &Self, start: usize| -> f32 {
            let mut total = 0.0;
            for symbol in 0..BLOCK {
                for p in 0..count {
                    for k in 1..PARTITION {
                        let c = sync.bins[Self::at(start + p * PARTITION + k, symbol)];
                        let ideal = Complex::new(1f32.copysign(c.re), 1f32.copysign(c.im));
                        total += (ideal - c).norm_sqr();
                    }
                }
            }
            total
        };
        let lower_error = error(self, LOWER_EDGE);
        let upper_error = error(self, UPPER_EDGE - count * PARTITION);
        let signal = 2.0 * BLOCK as f32 * (count * DATA_CARRIERS) as f32;
        let gain = |error: f32| (signal / error.max(f32::EPSILON) * 10.0).clamp(1.0, MAX_SOFT);
        let (lower_gain, upper_gain) = (gain(lower_error), gain(upper_error));
        self.mer_db = 10.0 * (2.0 * signal / (lower_error + upper_error).max(f32::EPSILON)).log10();
        let mut out = 0;
        for symbol in 0..BLOCK {
            for (start, gain) in [
                (LOWER_EDGE, lower_gain),
                (UPPER_EDGE - MAIN_PARTITIONS * PARTITION, upper_gain),
            ] {
                for p in 0..MAIN_PARTITIONS {
                    for k in 1..PARTITION {
                        let c = self.bins[Self::at(start + p * PARTITION + k, symbol)];
                        self.soft[out] = soft(c.re, gain);
                        self.soft[out + 1] = soft(c.im, gain);
                        out += 2;
                    }
                }
            }
        }
    }
}

impl Sync {
    /// The first `symbols` symbols of the block's equalised data subcarriers, every partition
    /// in use, scaled from the ±1 ± j grid onto the unit circle.
    pub fn points(&self, symbols: usize) -> impl Iterator<Item = Complex<f32>> + '_ {
        let count = partitions(self.psmi);
        (0..symbols.min(BLOCK)).flat_map(move |symbol| {
            [LOWER_EDGE, UPPER_EDGE - count * PARTITION]
                .into_iter()
                .flat_map(move |start| {
                    (0..count * PARTITION)
                        .filter(|offset| offset % PARTITION != 0)
                        .map(move |offset| {
                            self.bins[Self::at(start + offset, symbol)]
                                * std::f32::consts::FRAC_1_SQRT_2
                        })
                })
        })
    }

    /// Data subcarriers each symbol carries in the partitions now in use.
    pub fn data_carriers(&self) -> usize {
        2 * partitions(self.psmi) * DATA_CARRIERS
    }
}

fn soft(value: f32, gain: f32) -> Soft {
    (value.clamp(-1.0, 1.0) * gain).round() as Soft
}

fn wrap(phase: f32) -> f32 {
    if phase > PI {
        phase - TAU
    } else if phase < -PI {
        phase + TAU
    } else {
        phase
    }
}

fn half_turn(a: f32, b: f32) -> f32 {
    let mut diff = a - b;
    while diff > PI / 2.0 {
        diff -= PI;
    }
    while diff < -PI / 2.0 {
        diff += PI;
    }
    diff
}

/// The reference block's fixed bits as transmitted, before differential coding; `None`
/// where the block carries data.
fn needle(id: usize) -> [Option<u8>; BLOCK] {
    let id_high = (id >> 1) as u8;
    let id_low = id_high ^ (id & 1) as u8;
    let fixed: [i8; BLOCK] = [
        0, 1, 0, 0, 0, 1, 1, -1, 1, 0, -2, -3, -1, 0, 0, -1, -1, -1, -1, -1, 0, 1, 0, -1, -1, -1,
        -1, -1, -1, -1, -1, 0,
    ];
    fixed.map(|bit| match bit {
        -2 => Some(id_high),
        -3 => Some(id_low),
        bit if bit < 0 => None,
        bit => Some(bit as u8),
    })
}

fn matches(pattern: &[Option<u8>; BLOCK], bits: &[u8; BLOCK], offset: usize) -> bool {
    pattern
        .iter()
        .enumerate()
        .all(|(i, want)| want.is_none_or(|want| bits[(offset + i) % BLOCK] == want))
}

fn differential(bits: &[u8; BLOCK]) -> [u8; BLOCK] {
    let mut previous = 0;
    bits.map(|bit| {
        let data = bit ^ previous;
        previous = bit;
        data
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reference_block_is_found_at_its_rotation() {
        let pattern = needle(2);
        let block: [u8; BLOCK] = std::array::from_fn(|i| pattern[i].unwrap_or(1));
        let rotated: [u8; BLOCK] = std::array::from_fn(|i| block[(i + BLOCK - 5) % BLOCK]);
        assert!(matches(&pattern, &block, 0));
        assert!(matches(&pattern, &rotated, 5));
        assert!(!matches(&needle(1), &block, 0));
    }

    #[test]
    fn the_control_word_is_differentially_decoded() {
        assert_eq!(
            differential(&[
                1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0
            ])[..4],
            [1, 0, 1, 1]
        );
    }

    #[test]
    fn hybrid_modes_use_ten_partitions_and_extended_ones_more() {
        assert_eq!(partitions(1), 10);
        assert_eq!(partitions(2), 11);
        assert_eq!(partitions(3), 12);
        assert_eq!(partitions(5), 14);
    }
}
