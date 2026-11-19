//! Soft-decision decoding of the JT65 (63,12) Reed-Solomon code by the Franke-Taylor method:
//! hard decisions first, then errors-and-erasures decoding with the least reliable symbols erased
//! at random, keeping the candidate codeword whose tones hold the most power.

use sdrmm_dsp::coding::fec::rs256::ReedSolomon;

pub(crate) const N: usize = 63;
pub(crate) const K: usize = 12;
const PARITY: usize = N - K;
const PROBABILITY_SCALE: f32 = 255.999;
const MAX_HARD: u32 = 41;
const MAX_TOTAL: u32 = 71;
const MAX_RATIO: f32 = 0.70;

/// Erasure odds in percent by the symbol's second-to-first power ratio (row) and its reliability
/// rank in eighths (column), measured by K9AN on HF and EME signals.
const ERASURE_PERCENT: [[u8; 8]; 8] = [
    [4, 9, 11, 13, 14, 14, 15, 15],
    [2, 20, 20, 30, 40, 50, 50, 50],
    [7, 24, 27, 40, 50, 50, 50, 50],
    [13, 25, 35, 46, 52, 70, 50, 50],
    [17, 30, 42, 54, 55, 64, 71, 70],
    [25, 39, 48, 57, 64, 66, 77, 77],
    [32, 45, 54, 63, 66, 75, 78, 83],
    [51, 58, 57, 66, 72, 77, 82, 86],
];

/// The received word in decoder order: best and second-best symbol with their shares of the
/// symbol's power, scaled to 0..=255.
pub(crate) struct Received {
    pub(crate) best: [u8; N],
    pub(crate) best_share: [u32; N],
    pub(crate) second: [u8; N],
    pub(crate) second_share: [u32; N],
}

impl Received {
    /// Ranks each of `powers` (63 symbols of 64 tone powers, in decoder order).
    pub(crate) fn rank(powers: &[[f32; 64]; N]) -> Self {
        let mut received = Self {
            best: [0; N],
            best_share: [0; N],
            second: [0; N],
            second_share: [0; N],
        };
        for (symbol, row) in powers.iter().enumerate() {
            let total = row.iter().sum::<f32>().max(1e-6);
            let (first, top) = argmax(row, usize::MAX);
            let (next, runner) = argmax(row, first);
            received.best[symbol] = first as u8;
            received.second[symbol] = next as u8;
            received.best_share[symbol] = (PROBABILITY_SCALE * top / total) as u32;
            received.second_share[symbol] = (PROBABILITY_SCALE * runner / total) as u32;
        }
        received
    }
}

fn argmax(row: &[f32; 64], skip: usize) -> (usize, f32) {
    row.iter()
        .enumerate()
        .filter(|&(index, _)| index != skip)
        .fold((0, f32::MIN), |best, (index, &value)| {
            if value > best.1 { (index, value) } else { best }
        })
}

/// A codeword the decoder settled on, in decoder order, with how far it lies from what was
/// received: symbols that differ from the best guess, those plus a soft share where they also
/// differ from the second, and the power ratio of the runner-up candidate to it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Decoded {
    pub(crate) codeword: [u8; N],
    pub(crate) hard: u32,
    pub(crate) total: u32,
    pub(crate) ratio: f32,
}

pub(crate) struct SoftDecoder {
    code: ReedSolomon,
}

impl SoftDecoder {
    pub(crate) fn new() -> Self {
        Self {
            code: ReedSolomon::with_field(6, 0x43, 3, PARITY),
        }
    }

    pub(crate) fn encode(&self, data: &[u8; K]) -> [u8; N] {
        let mut out = Vec::with_capacity(N);
        self.code.encode(data, &mut out);
        let mut codeword = [0u8; N];
        codeword.copy_from_slice(&out[..N]);
        codeword
    }

    /// Decodes `received`, `power` scoring a candidate codeword by the mean power its tones hold.
    /// `None` when nothing decodes or the best candidate is too far from what was heard.
    pub(crate) fn decode(
        &self,
        received: &Received,
        trials: usize,
        power: &mut dyn FnMut(&[u8; N]) -> f32,
    ) -> Option<Decoded> {
        let mut hard = received.best;
        if self.code.decode(&mut hard).is_some() {
            let distance = differing(&hard, &received.best);
            return (distance <= MAX_HARD).then_some(Decoded {
                codeword: hard,
                hard: distance,
                total: 0,
                ratio: 0.0,
            });
        }
        let best = self.stochastic(received, trials, power)?;
        (best.hard <= MAX_HARD && best.total <= MAX_TOTAL && best.ratio <= MAX_RATIO)
            .then_some(best)
    }

    fn stochastic(
        &self,
        received: &Received,
        trials: usize,
        power: &mut dyn FnMut(&[u8; N]) -> f32,
    ) -> Option<Decoded> {
        let thresholds = erasure_thresholds(received);
        let share_sum: u32 = received.best_share.iter().sum();
        if share_sum == 0 {
            return None;
        }
        let order = least_reliable_first(received);
        let mut seed = 1u32;
        let mut erasures = [0usize; PARITY];
        let (mut first, mut second) = (0.0f32, 0.0f32);
        let mut best: Option<Decoded> = None;
        for _ in 0..trials {
            let mut erased = 0;
            for (rank, &symbol) in order.iter().enumerate() {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let draw = (seed / 65_536) % 32_768 * 100 / 32_768;
                if draw < thresholds[rank] && erased < PARITY {
                    erasures[erased] = symbol;
                    erased += 1;
                }
            }
            let mut word = received.best;
            if self
                .code
                .decode_with_erasures(&mut word, &erasures[..erased])
                .is_none()
            {
                continue;
            }
            let (hard, soft) = distances(&word, received, share_sum);
            let score = power(&word);
            if score > first {
                second = first;
                first = score;
                best = Some(Decoded {
                    codeword: word,
                    hard,
                    total: hard + soft,
                    ratio: 0.0,
                });
            } else if score > second && score != first {
                second = score;
            }
            if best.is_some_and(|found| found.hard <= MAX_HARD && found.total <= MAX_TOTAL) {
                break;
            }
        }
        best.map(|found| Decoded {
            ratio: if first > 0.0 { second / first } else { 1.0 },
            ..found
        })
    }
}

fn differing(a: &[u8; N], b: &[u8; N]) -> u32 {
    a.iter().zip(b).filter(|(x, y)| x != y).count() as u32
}

fn distances(word: &[u8; N], received: &Received, share_sum: u32) -> (u32, u32) {
    let mut hard = 0;
    let mut soft = 0;
    for symbol in 0..N {
        if word[symbol] != received.best[symbol] {
            hard += 1;
            if word[symbol] != received.second[symbol] {
                soft += received.best_share[symbol];
            }
        }
    }
    (hard, N as u32 * soft / share_sum)
}

fn least_reliable_first(received: &Received) -> [usize; N] {
    let mut order: [usize; N] = std::array::from_fn(|index| index);
    order.sort_by(|&a, &b| received.best_share[a].cmp(&received.best_share[b]));
    order
}

fn erasure_thresholds(received: &Received) -> [u32; N] {
    let order = least_reliable_first(received);
    std::array::from_fn(|rank| {
        let symbol = order[rank];
        let ratio =
            received.second_share[symbol] as f32 / (received.best_share[symbol] as f32 + 0.01);
        let row = ((7.999 * ratio) as usize).min(7);
        let column = (N - 1 - rank) / 8;
        (1.3 * f32::from(ERASURE_PERCENT[row][column])) as u32
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(codeword: &[u8; N], strength: f32) -> [[f32; 64]; N] {
        std::array::from_fn(|symbol| {
            std::array::from_fn(|tone| {
                let noise = ((symbol * 31 + tone * 17) % 23) as f32 / 23.0;
                if usize::from(codeword[symbol]) == tone {
                    strength + noise
                } else {
                    noise
                }
            })
        })
    }

    #[test]
    fn hard_decisions_correct_up_to_twenty_five_errors() {
        let decoder = SoftDecoder::new();
        let sent = decoder.encode(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        let mut powers = clean(&sent, 10.0);
        for symbol in (0..N).step_by(2).take(25) {
            powers[symbol].swap(
                usize::from(sent[symbol]),
                (usize::from(sent[symbol]) + 7) % 64,
            );
        }
        let received = Received::rank(&powers);
        let decoded = decoder.decode(&received, 0, &mut |_| 1.0).expect("decodes");
        assert_eq!(decoded.codeword, sent);
        assert_eq!(decoded.hard, 25);
    }

    #[test]
    fn erasures_reach_past_the_hard_decision_limit() {
        let decoder = SoftDecoder::new();
        let sent = decoder.encode(&[63, 0, 21, 42, 7, 14, 28, 56, 35, 49, 3, 60]);
        let mut powers = clean(&sent, 10.0);
        for symbol in (0..N).step_by(2).take(30) {
            let wrong = (usize::from(sent[symbol]) + 5) % 64;
            powers[symbol][wrong] = 10.5;
            powers[symbol][usize::from(sent[symbol])] = 10.0;
        }
        let received = Received::rank(&powers);
        let mut hard = received.best;
        assert!(decoder.code.decode(&mut hard).is_none());
        let truth = sent;
        let decoded = decoder
            .decode(&received, 10_000, &mut |word| {
                100.0 - differing(word, &truth) as f32
            })
            .expect("decodes");
        assert_eq!(decoded.codeword, sent);
    }
}
