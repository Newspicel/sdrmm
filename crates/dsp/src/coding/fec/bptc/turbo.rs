use crate::coding::fec::conv::Soft;

pub(super) const MAX_PASSES: usize = 5;

const MAX_COMPONENT_BITS: usize = 16;

pub(super) const CHASE_FLIPS: u32 = 4;

const CANDIDATES: usize = 1 << CHASE_FLIPS;

const MAX_SETTLE_REPAIRS: u32 = 4;

/// Pyndiah's weights for channel values scaled to a mean magnitude of one: how much of the other
/// dimension's extrinsic reliability each half pass trusts, and the reliability a bit gets when
/// no tested codeword disagrees with its decision.
pub(super) const ALPHA: [f32; 2 * MAX_PASSES] = [0.0, 0.2, 0.3, 0.5, 0.7, 0.9, 1.0, 1.0, 1.0, 1.0];
pub(super) const BETA: [f32; 2 * MAX_PASSES] = [0.2, 0.4, 0.6, 0.8, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];

/// What `word` costs against `input`: the reliability of every bit it contradicts.
fn discrepancy(input: &[f32], word: &[bool]) -> f32 {
    input
        .iter()
        .zip(word)
        .filter(|&(&y, &c)| (y > 0.0) != c)
        .map(|(&y, _)| y.abs())
        .sum()
}

/// Soft-in soft-out decode of one component word by Chase search over its `flips` least reliable
/// bits. Each bit's extrinsic reliability is the cost gap to the best tested codeword that
/// disagrees with it, or `beta` when none does. False when no test pattern decodes.
pub(super) fn siso(
    input: &[f32],
    flips: u32,
    beta: f32,
    decode: impl Fn(&mut [bool]) -> Option<u32>,
    extrinsic: &mut [f32],
) -> bool {
    let n = input.len();
    debug_assert!(n <= MAX_COMPONENT_BITS && flips <= CHASE_FLIPS && (flips as usize) <= n);
    let base: [bool; MAX_COMPONENT_BITS] = std::array::from_fn(|i| i < n && input[i] > 0.0);
    let mut order: [usize; MAX_COMPONENT_BITS] = std::array::from_fn(|i| i);
    order[..n].sort_unstable_by(|&a, &b| input[a].abs().total_cmp(&input[b].abs()));
    let mut words = [[false; MAX_COMPONENT_BITS]; CANDIDATES];
    let mut costs = [0.0f32; CANDIDATES];
    let mut found = 0;
    for pattern in 0..1u32 << flips {
        let mut word = base;
        for (j, &i) in order.iter().enumerate().take(flips as usize) {
            if pattern >> j & 1 == 1 {
                word[i] = !word[i];
            }
        }
        if decode(&mut word[..n]).is_some() {
            costs[found] = discrepancy(input, &word[..n]);
            words[found] = word;
            found += 1;
        }
    }
    let Some(best) = (0..found).min_by(|&a, &b| costs[a].total_cmp(&costs[b])) else {
        extrinsic.fill(0.0);
        return false;
    };
    for (j, slot) in extrinsic.iter_mut().enumerate() {
        let sign = if words[best][j] { 1.0 } else { -1.0 };
        let rival = (0..found)
            .filter(|&k| words[k][j] != words[best][j])
            .map(|k| costs[k])
            .min_by(f32::total_cmp);
        *slot = rival.map_or(beta * sign, |cost| (cost - costs[best]) * sign - input[j]);
    }
    true
}

/// One half pass over the component words `lines` of a product code: each word reads its channel
/// values plus `alpha` of the extrinsic the other dimension left, and `component` writes back its
/// own given `beta`.
pub(super) fn half_pass<const N: usize, const L: usize>(
    channel: &[f32; N],
    extrinsic: &mut [f32; N],
    lines: impl Iterator<Item = [usize; L]>,
    (alpha, beta): (f32, f32),
    mut component: impl FnMut(&[f32], f32, &mut [f32]),
) {
    let mut input = [0.0f32; L];
    let mut out = [0.0f32; L];
    for line in lines {
        for (slot, &a) in input.iter_mut().zip(&line) {
            *slot = channel[a] + alpha * extrinsic[a];
        }
        component(&input, beta, &mut out);
        for (&a, &w) in line.iter().zip(&out) {
            extrinsic[a] = w;
        }
    }
}

/// Soft-in soft-out decode of a single parity check by min-sum: each bit hears the parity of the
/// others, as reliable as the least reliable of them, scaled by `scale`.
pub(super) fn parity_siso(input: &[f32], scale: f32, extrinsic: &mut [f32]) {
    for (j, slot) in extrinsic.iter_mut().enumerate() {
        let (sign, least) = input
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != j)
            .fold((1.0f32, f32::INFINITY), |(sign, least), (_, &y)| {
                (if y < 0.0 { -sign } else { sign }, least.min(y.abs()))
            });
        *slot = scale * sign * least;
    }
}

/// Channel values scaled to a mean magnitude of one, or `None` for an all-erasure block.
pub(super) fn normalized<const N: usize>(soft: &[Soft; N]) -> Option<[f32; N]> {
    let mean = soft.iter().map(|&s| f32::from(s).abs()).sum::<f32>() / N as f32;
    (mean > 0.0).then(|| std::array::from_fn(|i| f32::from(soft[i]) / mean))
}

pub(super) fn decisions<const N: usize>(channel: &[f32; N], extrinsic: &[f32; N]) -> [bool; N] {
    std::array::from_fn(|i| channel[i] + extrinsic[i] > 0.0)
}

/// The settled hard decision and how many channel bits it overrules, unless settling it takes
/// too many repairs or it contradicts the channel by more than `max_cost`.
pub(super) fn conclude<const N: usize>(
    channel: &[f32; N],
    extrinsic: &[f32; N],
    max_cost: f32,
    settle: impl FnOnce(&mut [bool; N]) -> Option<u32>,
) -> Option<([bool; N], u32)> {
    let mut hard = decisions(channel, extrinsic);
    if settle(&mut hard).is_none_or(|repairs| repairs > MAX_SETTLE_REPAIRS)
        || discrepancy(channel, &hard) > max_cost
    {
        return None;
    }
    let overruled = hard
        .iter()
        .zip(channel)
        .filter(|&(&bit, &y)| bit != (y > 0.0))
        .count() as u32;
    Some((hard, overruled))
}
