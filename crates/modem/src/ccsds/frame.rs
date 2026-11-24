use sdrmm_dsp::coding::fec::{
    ccsds_rs::{Basis, CcsdsReedSolomon},
    conv7::ViterbiK7,
};

use crate::framesync::sync_distance;

use super::link::{
    ASM_BYTES, ASM_CODED_BITS, AsmPattern, CODED_BITS, CODED_BYTES, INTERLEAVE, PN, PairMap,
    Precoding, VCDU_BYTES, asm_patterns_with, conv_code, reed_solomon,
};

const LEAD: usize = 96;
const TAIL: usize = ASM_CODED_BITS;
const WINDOW: usize = LEAD + CODED_BITS + TAIL;
const SEARCH_MISMATCHES: f32 = 4.0;
const TRACK_MISMATCHES: f32 = 14.0;
/// Hard bit errors past which a window is not worth a soft look.
const PREFILTER_MISMATCHES: u32 = 16;
const MAX_MISSES: u8 = 2;
const SLIP_GUARD: usize = 64;
pub const SOFT_CAPACITY: usize = 2 * WINDOW + 16_384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameOutcome {
    pub corrected: u32,
    pub failed: bool,
}

#[derive(Clone, Copy, Debug)]
enum State {
    Search {
        scan: usize,
        window: u64,
        filled: usize,
    },
    Track {
        start: usize,
        map: PairMap,
        misses: u8,
    },
}

impl State {
    fn search_from(scan: usize) -> Self {
        Self::Search {
            scan,
            window: 0,
            filled: 0,
        }
    }

    fn keep_from(&self) -> usize {
        match *self {
            Self::Search { scan, .. } => scan.saturating_sub(ASM_CODED_BITS + LEAD),
            Self::Track { start, .. } => start.saturating_sub(LEAD),
        }
    }

    fn shifted(self, by: usize) -> Self {
        match self {
            Self::Search {
                scan,
                window,
                filled,
            } => Self::Search {
                scan: scan - by,
                window,
                filled,
            },
            Self::Track { start, map, misses } => Self::Track {
                start: start - by,
                map,
                misses,
            },
        }
    }
}

/// Finds CADUs in a stream of soft coded symbols, Viterbi-decodes, derandomizes and
/// RS-corrects each, whatever rotation or symbol slip the demodulator left.
pub struct Deframer {
    soft: Vec<i16>,
    state: State,
    patterns: [AsmPattern; 8],
    viterbi: ViterbiK7,
    coded: Vec<i16>,
    decoded: Vec<bool>,
    cadu: [u8; CODED_BYTES],
    rs: CcsdsReedSolomon,
    precoding: Precoding,
    confirmed: bool,
}

impl Deframer {
    #[must_use]
    pub fn new(basis: Basis) -> Self {
        let mut viterbi = ViterbiK7::new(conv_code());
        let mut decoded = Vec::with_capacity(WINDOW / 2 + 1);
        viterbi.decode(&vec![0; WINDOW], &mut decoded);
        decoded.clear();
        Self {
            soft: Vec::with_capacity(SOFT_CAPACITY),
            state: State::search_from(0),
            patterns: asm_patterns_with(Precoding::Nrzl),
            viterbi,
            coded: Vec::with_capacity(WINDOW),
            decoded,
            cadu: [0; CODED_BYTES],
            rs: reed_solomon(basis),
            precoding: Precoding::Nrzl,
            confirmed: false,
        }
    }

    /// The same deframer for a link that NRZ-M codes ahead of the convolutional encoder.
    #[must_use]
    pub fn with_precoding(mut self, precoding: Precoding) -> Self {
        self.precoding = precoding;
        self.patterns = asm_patterns_with(precoding);
        self
    }

    pub fn reset(&mut self) {
        self.soft.clear();
        self.state = State::search_from(0);
        self.confirmed = false;
    }

    #[must_use]
    pub fn vcdu(&self) -> &[u8] {
        &self.cadu[..VCDU_BYTES]
    }

    pub fn push(&mut self, soft: &[i16]) {
        let overflow = (self.soft.len() + soft.len()).saturating_sub(SOFT_CAPACITY);
        if overflow > 0 {
            self.discard(overflow);
        }
        let room = SOFT_CAPACITY - self.soft.len();
        let skip = soft.len().saturating_sub(room);
        self.soft.extend_from_slice(&soft[skip..]);
    }

    fn discard(&mut self, count: usize) {
        let count = count.min(self.soft.len());
        self.soft.copy_within(count.., 0);
        self.soft.truncate(self.soft.len() - count);
        self.state = match self.state {
            State::Track { start, .. } if start < count + LEAD => State::search_from(0),
            State::Search { scan, .. } if scan < count => State::search_from(0),
            other => other.shifted(count),
        };
    }

    fn compact(&mut self) {
        let keep = self.state.keep_from();
        if keep >= CODED_BITS / 2 {
            self.soft.copy_within(keep.., 0);
            self.soft.truncate(self.soft.len() - keep);
            self.state = self.state.shifted(keep);
        }
    }

    pub fn next_frame(&mut self) -> Option<FrameOutcome> {
        loop {
            let progressed = match self.state {
                State::Search {
                    scan,
                    window,
                    filled,
                } => self.search(scan, window, filled),
                State::Track { start, map, misses } => {
                    if start + CODED_BITS + TAIL > self.soft.len() {
                        false
                    } else if let Some(outcome) = self.track(start, map, misses) {
                        self.compact();
                        return Some(outcome);
                    } else {
                        true
                    }
                }
            };
            if !progressed {
                self.compact();
                return None;
            }
        }
    }

    fn search(&mut self, mut scan: usize, mut window: u64, mut filled: usize) -> bool {
        while scan < self.soft.len() {
            window = window << 1 | u64::from(self.soft[scan] > 0);
            scan += 1;
            filled += 1;
            if filled < ASM_CODED_BITS {
                continue;
            }
            if let Some(map) = self.matching_map(scan, window) {
                self.state = State::Track {
                    start: scan - ASM_CODED_BITS,
                    map,
                    misses: 0,
                };
                self.confirmed = false;
                return true;
            }
        }
        self.state = State::Search {
            scan,
            window,
            filled,
        };
        false
    }

    /// The map whose ASM the soft values ending at `end` hold best, within the search limit;
    /// `window` is their hard bits, which rule out the plainly wrong patterns cheaply.
    fn matching_map(&self, end: usize, window: u64) -> Option<PairMap> {
        let soft = &self.soft[end - ASM_CODED_BITS..end];
        self.patterns
            .iter()
            .filter(|pattern| pattern.mismatches(window) <= PREFILTER_MISMATCHES)
            .map(|pattern| {
                (
                    sync_distance(soft, pattern.value, pattern.mask).upright,
                    pattern.map,
                )
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .filter(|&(distance, _)| distance <= SEARCH_MISMATCHES)
            .map(|(_, map)| map)
    }

    fn asm_window(&self, start: usize) -> u64 {
        self.soft[start..start + ASM_CODED_BITS]
            .iter()
            .fold(0u64, |acc, &value| acc << 1 | u64::from(value > 0))
    }

    fn tracked_map(&self, start: usize, map: PairMap) -> Option<PairMap> {
        let end = start + ASM_CODED_BITS;
        let soft = &self.soft[start..end];
        let holds = self
            .patterns
            .iter()
            .find(|pattern| pattern.map == map)
            .is_some_and(|pattern| {
                sync_distance(soft, pattern.value, pattern.mask).upright <= TRACK_MISMATCHES
            });
        if holds {
            Some(map)
        } else {
            self.matching_map(end, self.asm_window(start))
        }
    }

    fn track(&mut self, start: usize, map: PairMap, misses: u8) -> Option<FrameOutcome> {
        let next = start + CODED_BITS;
        if let Some(map) = self.tracked_map(start, map) {
            self.state = State::Track {
                start: next,
                map,
                misses: 0,
            };
            let outcome = self.decode(start, map);
            if outcome.failed && !self.confirmed {
                return None;
            }
            self.confirmed = true;
            return Some(outcome);
        }
        self.state = if misses + 1 >= MAX_MISSES {
            State::search_from(start.saturating_sub(SLIP_GUARD))
        } else {
            State::Track {
                start: next,
                map,
                misses: misses + 1,
            }
        };
        None
    }

    fn decode(&mut self, start: usize, map: PairMap) -> FrameOutcome {
        let lead = start.min(LEAD) & !1;
        let from = start - lead;
        let to = start + CODED_BITS + TAIL;
        self.coded.clear();
        for &[a, b] in self.soft[from..to].as_chunks::<2>().0 {
            let (first, second) = map.apply(a, b);
            self.coded.push(first);
            self.coded.push(second);
        }
        self.decoded.clear();
        self.viterbi.decode(&self.coded, &mut self.decoded);
        let first_bit = lead / 2 + ASM_BYTES * 8;
        let nrzm = self.precoding == Precoding::Nrzm;
        for (index, byte) in self.cadu.iter_mut().enumerate() {
            let at = first_bit + index * 8;
            let bits = (at..at + 8).map(|bit| {
                let line = self.decoded[bit];
                if nrzm {
                    line ^ self.decoded[bit - 1]
                } else {
                    line
                }
            });
            *byte = bits.fold(0u8, |acc, bit| acc << 1 | u8::from(bit)) ^ PN[index];
        }
        let outcome = self.rs.decode_interleaved(&mut self.cadu, INTERLEAVE);
        FrameOutcome {
            corrected: outcome.corrected,
            failed: outcome.failed > 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_dsp::math::rng::Rng;

    use super::*;

    const LEVEL: f64 = 40.0;
    /// Noise that flips about one coded symbol in seven.
    const SIGMA: f64 = 0.9;
    const TRIALS: usize = 400;

    fn symbol(rng: &mut Rng, bit: bool) -> i16 {
        let level = if bit { 1.0 } else { -1.0 };
        ((level + SIGMA * rng.normal()) * LEVEL).round() as i16
    }

    fn hard_window(soft: &[i16]) -> u64 {
        soft.iter()
            .fold(0u64, |acc, &value| acc << 1 | u64::from(value > 0))
    }

    #[test]
    fn soft_search_finds_markers_hard_search_misses_and_no_false_ones() {
        let mut deframer = Deframer::new(Basis::Dual);
        let pattern = deframer.patterns[0];
        let mut rng = Rng::mixed(0x0a5a);
        let (mut soft_hits, mut hard_hits) = (0, 0);
        for _ in 0..TRIALS {
            let marker: Vec<i16> = (0..ASM_CODED_BITS)
                .map(|index| {
                    symbol(
                        &mut rng,
                        pattern.value >> (ASM_CODED_BITS - 1 - index) & 1 == 1,
                    )
                })
                .collect();
            deframer.reset();
            deframer.push(&marker);
            let window = hard_window(&marker);
            soft_hits +=
                u32::from(deframer.matching_map(ASM_CODED_BITS, window) == Some(pattern.map));
            hard_hits += u32::from(pattern.mismatches(window) <= SEARCH_MISMATCHES as u32);
        }
        assert!(
            soft_hits >= 3 * hard_hits && soft_hits * 2 >= TRIALS as u32,
            "soft {soft_hits}, hard {hard_hits} of {TRIALS}"
        );
        let noise: Vec<i16> = (0..SOFT_CAPACITY)
            .map(|_| {
                let bit = rng.bits() & 1 == 1;
                symbol(&mut rng, bit)
            })
            .collect();
        deframer.reset();
        deframer.push(&noise);
        let false_alarms = (ASM_CODED_BITS..noise.len())
            .filter(|&end| {
                let window = hard_window(&noise[end - ASM_CODED_BITS..end]);
                deframer.matching_map(end, window).is_some()
            })
            .count();
        assert_eq!(false_alarms, 0);
    }
}
