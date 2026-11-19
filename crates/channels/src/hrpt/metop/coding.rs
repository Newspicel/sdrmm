//! The AHRPT inner code: CCSDS K=7 at rate 3/4. Each pair of QPSK symbols carries four coded
//! bits, `C1 C2` of the first information bit on the first symbol, then `C1` of the third and
//! `C2` of the second on the I and Q of the next. Which symbol opens a pair and how the
//! constellation sits are both unknown, so a lock tries each and keeps the one whose decoded
//! bits carry two sync markers a frame apart.

use num_complex::Complex;
use sdrmm_dsp::{CONFIDENT, ERASURE, Soft, StreamViterbiK7, ViterbiK7, hamming_distance};
use sdrmm_modem::ccsds::{ASM, CADU_BITS, conv_code};

const TRACEBACK: usize = 96;
/// Symbols a lock is judged on: a little over two frames of decoded bits.
pub const TRIAL_SYMBOLS: usize = 12_288;
/// Symbols the hypotheses are ranked on before the best are decoded in full.
const RANK_SYMBOLS: usize = 2_048;
/// Symbols between checks that the carrier has not jumped a quarter turn.
const CHECK_SYMBOLS: usize = 1_024;
/// How much better a quarter turn must fit before the lock follows it.
const TURN_MARGIN: f32 = 0.05;
/// Hypotheses decoded in full, best ranked first.
const VERIFIED: usize = 2;
/// Mother code softs per symbol pair: six, two of them erased.
const MOTHER_PER_PAIR: usize = 6;
pub const MOTHER_CAPACITY: usize = TRIAL_SYMBOLS / 2 * MOTHER_PER_PAIR + MOTHER_PER_PAIR;
const ASM_ERRORS: u32 = 3;
const LEVEL_SMOOTHING: f32 = 1.0 / 1_024.0;
/// Soft magnitude of a symbol at the mean level, leaving headroom below the clamp.
const NOMINAL: f32 = 48.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Lock {
    rotation: u8,
    /// Whether the next symbol opens a pair.
    first: bool,
}

const HYPOTHESES: [Lock; 4] = [
    Lock {
        rotation: 0,
        first: true,
    },
    Lock {
        rotation: 0,
        first: false,
    },
    Lock {
        rotation: 1,
        first: true,
    },
    Lock {
        rotation: 1,
        first: false,
    },
];

/// The symbol turned by `rotation` quarter turns.
fn rotate([i, q]: [Soft; 2], rotation: u8) -> [Soft; 2] {
    match rotation & 3 {
        0 => [i, q],
        1 => [-q, i],
        2 => [-i, -q],
        _ => [q, -i],
    }
}

/// The mother code softs one symbol fills in, erasing what the puncturing left out.
fn depuncture(soft: [Soft; 2], lock: &mut Lock, out: &mut Vec<Soft>) {
    let [i, q] = rotate(soft, lock.rotation);
    if lock.first {
        out.extend_from_slice(&[i, q]);
    } else {
        out.extend_from_slice(&[ERASURE, q, i, ERASURE]);
    }
    lock.first = !lock.first;
}

/// The polarity of two sync markers a frame apart in `bits`, `true` when inverted.
fn marker_pair(bits: &[bool]) -> Option<bool> {
    let word_at = |at: usize| {
        bits[at..at + 32]
            .iter()
            .fold(0u64, |acc, &bit| acc << 1 | u64::from(bit))
    };
    let matches = |window: u64, inverted: bool| {
        let window = if inverted {
            !window & 0xFFFF_FFFF
        } else {
            window
        };
        hamming_distance(window, u64::from(ASM)) <= ASM_ERRORS
    };
    (0..bits.len().saturating_sub(CADU_BITS + 32)).find_map(|at| {
        [false, true].into_iter().find(|&inverted| {
            matches(word_at(at), inverted) && matches(word_at(at + CADU_BITS), inverted)
        })
    })
}

pub struct Coder {
    lock: Option<Lock>,
    stream: StreamViterbiK7,
    trial: ViterbiK7,
    /// Symbols awaiting a lock, or once locked the latest ones the fit is checked on.
    window: Vec<[Soft; 2]>,
    /// The lock as it stood when the checked symbols began.
    window_lock: Option<Lock>,
    mother: Vec<Soft>,
    scratch: Vec<Soft>,
    decoded: Vec<bool>,
    level: f32,
    turns: u32,
}

impl Coder {
    #[must_use]
    pub fn new() -> Self {
        let code = conv_code();
        Self {
            lock: None,
            stream: StreamViterbiK7::new(code.clone(), TRACEBACK),
            trial: ViterbiK7::with_capacity(code, MOTHER_CAPACITY / 2),
            window: Vec::with_capacity(TRIAL_SYMBOLS),
            window_lock: None,
            mother: Vec::with_capacity(MOTHER_CAPACITY),
            scratch: Vec::with_capacity(MOTHER_CAPACITY),
            decoded: Vec::with_capacity(MOTHER_CAPACITY / 2),
            level: 1.0,
            turns: 0,
        }
    }

    #[cfg(test)]
    #[must_use]
    pub const fn locked(&self) -> bool {
        self.lock.is_some()
    }

    /// Quarter turns of the carrier the lock has followed.
    #[cfg(test)]
    #[must_use]
    pub const fn turns(&self) -> u32 {
        self.turns
    }

    /// Drops the lock; the next symbols are searched afresh.
    pub fn unlock(&mut self) {
        self.lock = None;
        self.window.clear();
    }

    pub fn reset(&mut self) {
        self.unlock();
        self.stream.reset();
        self.level = 1.0;
    }

    fn soften(&mut self, symbol: Complex<f32>) -> [Soft; 2] {
        let magnitude = symbol.re.abs() + symbol.im.abs();
        self.level += (magnitude - self.level) * LEVEL_SMOOTHING;
        let scale = 2.0 * NOMINAL / self.level.max(1e-9);
        let limit = f32::from(CONFIDENT);
        [symbol.re, symbol.im].map(|value| (value * scale).clamp(-limit, limit) as Soft)
    }

    /// Decodes `symbols`, appending the information bits to `bits` once locked.
    pub fn push(&mut self, symbols: &[Complex<f32>], bits: &mut Vec<bool>) {
        self.mother.clear();
        for &symbol in symbols {
            let soft = self.soften(symbol);
            let Some(mut lock) = self.lock else {
                self.window.push(soft);
                if self.window.len() == TRIAL_SYMBOLS {
                    self.acquire(bits);
                }
                continue;
            };
            if self.window.is_empty() {
                self.window_lock = Some(lock);
            }
            self.window.push(soft);
            depuncture(soft, &mut lock, &mut self.mother);
            self.lock = Some(lock);
            if self.mother.len() + MOTHER_PER_PAIR > self.mother.capacity() {
                self.stream.push(&self.mother, bits);
                self.mother.clear();
            }
            if self.window.len() == CHECK_SYMBOLS {
                self.check();
            }
        }
        self.stream.push(&self.mother, bits);
        self.mother.clear();
    }

    /// How well the first `symbols` of the window fit the code under `lock`, near 1 when clean.
    fn score(&mut self, symbols: usize, lock: Lock) -> f32 {
        fill(
            &mut self.scratch,
            &self.window[..symbols.min(self.window.len())],
            lock,
        );
        self.decoded.clear();
        let metric = self.trial.decode(&self.scratch, &mut self.decoded);
        let energy: i64 = self
            .scratch
            .iter()
            .map(|&value| i64::from(value.abs()))
            .sum();
        if energy == 0 {
            return 0.0;
        }
        (f64::from(metric) / energy as f64) as f32
    }

    /// A carrier that jumped a quarter turn shows as the next rotation fitting better than the
    /// locked one; the lock turns with it. A half turn cannot show here and is left to the
    /// framer, which follows a stream that turns over.
    fn check(&mut self) {
        if let (Some(lock), Some(start)) = (self.lock, self.window_lock) {
            let turned = Lock {
                rotation: (start.rotation + 1) & 3,
                ..start
            };
            if self.score(CHECK_SYMBOLS, turned) > self.score(CHECK_SYMBOLS, start) + TURN_MARGIN {
                self.lock = Some(Lock {
                    rotation: turned.rotation,
                    ..lock
                });
                self.turns = self.turns.saturating_add(1);
            }
        }
        self.window.clear();
    }

    fn verified(&mut self, lock: Lock) -> bool {
        fill(&mut self.scratch, &self.window, lock);
        self.decoded.clear();
        self.trial.decode(&self.scratch, &mut self.decoded);
        marker_pair(&self.decoded).is_some()
    }

    /// Tries every hypothesis on the window; a lock decodes the window from its start, and a
    /// miss keeps the later half, where a marker cut off at the end may now pair up.
    fn acquire(&mut self, bits: &mut Vec<bool>) {
        let mut ranked = HYPOTHESES.map(|lock| (0.0f32, lock));
        for (slot, lock) in ranked.iter_mut().zip(HYPOTHESES) {
            *slot = (self.score(RANK_SYMBOLS, lock), lock);
        }
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
        let found = ranked
            .into_iter()
            .take(VERIFIED)
            .map(|(_, lock)| lock)
            .find(|&lock| self.verified(lock));
        let Some(lock) = found else {
            self.window.drain(..TRIAL_SYMBOLS / 2);
            return;
        };
        self.stream.reset();
        fill(&mut self.scratch, &self.window, lock);
        self.stream.push(&self.scratch, bits);
        self.lock = Some(Lock {
            first: lock.first ^ (self.window.len() % 2 == 1),
            ..lock
        });
        self.window.clear();
    }
}

/// The mother code softs of `symbols` under `lock`, in place of what `out` held.
fn fill(out: &mut Vec<Soft>, symbols: &[[Soft; 2]], mut lock: Lock) {
    out.clear();
    for &soft in symbols {
        depuncture(soft, &mut lock, out);
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Rate 3/4 coding of `bits` as QPSK symbols, the first opening a pair.
    #[must_use]
    pub fn encode(bits: &[bool]) -> Vec<Complex<f32>> {
        let mut coded = Vec::with_capacity(2 * bits.len());
        conv_code().encode(bits, &mut coded);
        let level = |bit: bool| if bit { 1.0 } else { -1.0 };
        coded
            .as_chunks::<6>()
            .0
            .iter()
            .flat_map(|&[a1, a2, _, b2, c1, _]| {
                [
                    Complex::new(level(a1), level(a2)),
                    Complex::new(level(c1), level(b2)),
                ]
            })
            .collect()
    }

    fn frames(count: usize) -> Vec<bool> {
        let mut bits = Vec::new();
        for frame in 0..count {
            bits.extend((0..32).rev().map(|shift| ASM >> shift & 1 == 1));
            bits.extend((0..CADU_BITS - 32).map(|k| (k * 7 + frame * 13) % 11 < 5));
        }
        bits
    }

    fn decoded(symbols: &[Complex<f32>]) -> Vec<bool> {
        let mut coder = Coder::new();
        let mut bits = Vec::new();
        for chunk in symbols.chunks(5_000) {
            coder.push(chunk, &mut bits);
        }
        assert!(coder.locked());
        bits
    }

    /// Whether `bits` holds frames 1 and 2 of `sent` intact.
    fn holds(bits: &[bool], sent: &[bool]) -> bool {
        let wanted = &sent[CADU_BITS..3 * CADU_BITS];
        bits.windows(wanted.len()).any(|window| window == wanted)
    }

    #[test]
    fn every_rotation_and_pair_phase_locks() {
        let sent = frames(6);
        let symbols = encode(&sent);
        for quarter in 0..4u8 {
            for skip in 0..2 {
                let turned: Vec<Complex<f32>> = symbols[skip..]
                    .iter()
                    .map(|&symbol| symbol * Complex::new(0.0, 1.0).powi(i32::from(quarter)))
                    .collect();
                let bits = decoded(&turned);
                let inverted: Vec<bool> = bits.iter().map(|&bit| !bit).collect();
                assert!(
                    holds(&bits, &sent) || holds(&inverted, &sent),
                    "rotation {quarter} skip {skip}"
                );
            }
        }
    }

    #[test]
    fn a_quarter_turn_of_the_carrier_is_followed() {
        let sent = frames(8);
        let mut symbols = encode(&sent);
        for symbol in &mut symbols[20_000..] {
            *symbol *= Complex::new(0.0, 1.0);
        }
        let mut coder = Coder::new();
        let mut bits = Vec::new();
        coder.push(&symbols, &mut bits);
        assert_eq!(coder.turns(), 1);
        let later = &sent[5 * CADU_BITS..7 * CADU_BITS];
        let inverted: Vec<bool> = later.iter().map(|&bit| !bit).collect();
        assert!(
            bits.windows(later.len())
                .any(|window| window == later || window == inverted.as_slice())
        );
    }

    #[test]
    fn noise_never_locks() {
        let noise = sdrmm_decoder_test_support::complex_noise(5, 1.0, 3 * TRIAL_SYMBOLS);
        let mut coder = Coder::new();
        let mut bits = Vec::new();
        coder.push(&noise, &mut bits);
        assert!(!coder.locked());
        assert!(bits.is_empty());
    }
}
