//! When a soft decision stands.

/// A decision this many times surer than the word's typical one is never overturned: a code
/// that wants it flipped more likely found noise than an error.
const STRONG: f64 = 1.5;

/// Whether a candidate codeword of a code of minimum distance `distance` stands. `bits` yields
/// each bit's reliability, zero for an erasure, and whether the candidate overturns it.
///
/// Reliability is measured against the typical decided bit, the mean, so the test does not care
/// about scale and a weak word as a whole decides as surely as a strong one. The candidate never
/// overturns a decision [`STRONG`] times the typical; within that it stands when twice its
/// overturned decisions plus the erasures stay below `distance`, as a hard decoder takes it, or
/// when it passes Forney's generalized minimum distance test with each reliability over the
/// typical one, capped at one. Each test admits at most one codeword, and on hard input both are
/// the hard decoder's rule. Sums run in `f64`, where equal reliabilities add up exactly.
pub(crate) fn stands(bits: impl Iterator<Item = (f32, bool)> + Clone, distance: u32) -> bool {
    let (mut sum, mut decided, mut len) = (0.0f64, 0u32, 0u32);
    for (reliability, _) in bits.clone() {
        len += 1;
        if reliability > 0.0 {
            sum += f64::from(reliability);
            decided += 1;
        }
    }
    let erasures = len - decided;
    if decided == 0 {
        return erasures < distance;
    }
    let (mut errors, mut metric) = (0u32, 0.0f64);
    for (reliability, overturned) in bits.filter(|&(reliability, _)| reliability > 0.0) {
        let scaled = f64::from(reliability) * f64::from(decided);
        if !overturned {
            metric += scaled.min(sum);
        } else if scaled > STRONG * sum {
            return false;
        } else {
            errors += 1;
            metric -= scaled.min(sum);
        }
    }
    2 * errors + erasures < distance || metric > (f64::from(len) - f64::from(distance)) * sum
}

/// Whether a candidate stands that a hard decoder correcting `radius` errors would vouch for, or
/// that reaches further only through weak bits: it overturns at most `radius` decisions, or
/// decisions whose reliability sums to at most `radius` typical ones, the mean decided bit. On
/// hard input that is the hard decoder's rule. Looser than [`stands`], for decoders a later
/// check backs.
pub(crate) fn within(bits: impl Iterator<Item = (f32, bool)> + Clone, radius: u32) -> bool {
    let (mut sum, mut decided) = (0.0f64, 0u32);
    for (reliability, _) in bits.clone().filter(|&(reliability, _)| reliability > 0.0) {
        sum += f64::from(reliability);
        decided += 1;
    }
    let (mut overturned, mut cost) = (0u32, 0.0f64);
    for (reliability, _) in bits.filter(|&(reliability, flipped)| flipped && reliability > 0.0) {
        overturned += 1;
        cost += f64::from(reliability) * f64::from(decided);
    }
    overturned <= radius || cost <= f64::from(radius) * sum
}

#[cfg(test)]
mod tests {
    use super::*;

    fn judge(reliability: &[f32], overturned: u64, distance: u32) -> bool {
        let bits = reliability
            .iter()
            .enumerate()
            .map(|(bit, &value)| (value, overturned >> bit & 1 == 1));
        stands(bits, distance)
    }

    #[test]
    fn hard_input_follows_the_hard_rule_at_any_scale() {
        for scale in [1.0, 0.1, 37.0, 1e-3] {
            let sure = [scale; 7];
            assert!(judge(&sure, 0b001, 3));
            assert!(!judge(&sure, 0b011, 3));
            assert!(judge(&sure, 0b011, 5));
            assert!(!judge(&sure, 0b111, 5));
        }
    }

    #[test]
    fn erasures_spend_half_what_errors_do() {
        let mut word = [1.0; 7];
        word[..2].fill(0.0);
        assert!(judge(&word, 0, 3));
        assert!(!judge(&word, 0b100, 3));
        word[2] = 0.0;
        assert!(!judge(&word, 0, 3));
        assert!(!judge(&[0.0; 4], 0, 3));
    }

    #[test]
    fn weak_bits_buy_reach_past_the_hard_radius() {
        let mut word = [1.0; 12];
        word[..2].fill(0.1);
        assert!(judge(&word, 0b11, 3));
        assert!(!judge(&[1.0; 12], 0b11, 3));
    }

    fn reach(reliability: &[f32], overturned: u64, radius: u32) -> bool {
        let bits = reliability
            .iter()
            .enumerate()
            .map(|(bit, &value)| (value, overturned >> bit & 1 == 1));
        within(bits, radius)
    }

    #[test]
    fn the_radius_rule_holds_hard_input_to_the_radius_and_spends_weak_bits() {
        for scale in [1.0, 0.1, 37.0] {
            let sure = [scale; 7];
            assert!(reach(&sure, 0b01, 1));
            assert!(!reach(&sure, 0b11, 1));
        }
        let mut word = [1.0; 7];
        word[..3].fill(0.2);
        assert!(reach(&word, 0b111, 1));
        assert!(!reach(&word, 0b1011, 1));
    }

    #[test]
    fn a_decision_far_surer_than_typical_is_never_overturned() {
        let mut word = [1.0; 12];
        word[0] = 2.0;
        assert!(!judge(&word, 0b1, 5));
        assert!(judge(&word, 0b10, 5));
    }
}
