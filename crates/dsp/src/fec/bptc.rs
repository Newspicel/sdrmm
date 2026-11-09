use super::{block::ParityCode, conv::Soft};

const MAX_PASSES: usize = 5;

const MAX_COMPONENT_BITS: usize = 16;

const CHASE_FLIPS: u32 = 4;

const CANDIDATES: usize = 1 << CHASE_FLIPS;

const MAX_SETTLE_REPAIRS: u32 = 4;

/// The most a decision may contradict the channel, in mean bit reliabilities. A turbo decoder
/// finds some codeword near almost anything, so a decision overruling more is likelier one it
/// found in noise than the one sent.
const MAX_COST: f32 = 6.0;

/// [`MAX_COST`] for BPTC(128,77), whose single parity columns leave nearer neighbours.
const MAX_COST_128: f32 = 3.5;

/// How far a min-sum parity check's reliability is trusted. Min-sum overstates it, and with
/// only one parity per column an overstated check walks rows into wrong codewords; measured
/// against AWGN.
const PARITY_SCALE: f32 = 0.375;

/// Pyndiah's weights for channel values scaled to a mean magnitude of one: how much of the other
/// dimension's extrinsic reliability each half pass trusts, and the reliability a bit gets when
/// no tested codeword disagrees with its decision.
const ALPHA: [f32; 2 * MAX_PASSES] = [0.0, 0.2, 0.3, 0.5, 0.7, 0.9, 1.0, 1.0, 1.0, 1.0];
const BETA: [f32; 2 * MAX_PASSES] = [0.2, 0.4, 0.6, 0.8, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];

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
fn siso(
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
fn half_pass<const N: usize, const L: usize>(
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
fn parity_siso(input: &[f32], scale: f32, extrinsic: &mut [f32]) {
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
fn normalized<const N: usize>(soft: &[Soft; N]) -> Option<[f32; N]> {
    let mean = soft.iter().map(|&s| f32::from(s).abs()).sum::<f32>() / N as f32;
    (mean > 0.0).then(|| std::array::from_fn(|i| f32::from(soft[i]) / mean))
}

fn decisions<const N: usize>(channel: &[f32; N], extrinsic: &[f32; N]) -> [bool; N] {
    std::array::from_fn(|i| channel[i] + extrinsic[i] > 0.0)
}

pub struct Bptc196;

impl Bptc196 {
    pub const CODED_BITS: usize = 196;
    pub const DATA_BITS: usize = 96;

    fn deinterleave(coded: &[bool; Self::CODED_BITS]) -> [bool; Self::CODED_BITS] {
        let mut matrix = [false; Self::CODED_BITS];
        for (a, slot) in matrix.iter_mut().enumerate() {
            *slot = coded[a * 181 % Self::CODED_BITS];
        }
        matrix
    }

    #[must_use]
    pub fn decode(coded: &[bool; Self::CODED_BITS]) -> Option<([bool; Self::DATA_BITS], u32)> {
        let mut matrix = Self::deinterleave(coded);
        let corrected = Self::settle(&mut matrix)?;
        Some((Self::extract(&matrix), corrected))
    }

    /// Turbo product decoding (Pyndiah): columns and rows pass extrinsic reliabilities to each
    /// other, so a component's wrong guess does not reach the other dimension as a certainty.
    #[must_use]
    pub fn decode_soft(coded: &[Soft; Self::CODED_BITS]) -> Option<([bool; Self::DATA_BITS], u32)> {
        let mut deinterleaved = [0 as Soft; Self::CODED_BITS];
        for (a, slot) in deinterleaved.iter_mut().enumerate() {
            *slot = coded[a * 181 % Self::CODED_BITS];
        }
        let channel = normalized(&deinterleaved)?;
        let mut extrinsic = [0.0f32; Self::CODED_BITS];
        let columns = || (0..15).map(|c| std::array::from_fn::<_, 13, _>(|r| r * 15 + c + 1));
        let rows = || (0..13).map(|r| std::array::from_fn::<_, 15, _>(|k| r * 15 + 1 + k));
        for pass in 0..MAX_PASSES {
            let column_half = (ALPHA[2 * pass], BETA[2 * pass]);
            half_pass(
                &channel,
                &mut extrinsic,
                columns(),
                column_half,
                |y, beta, w| {
                    siso(
                        y,
                        CHASE_FLIPS,
                        beta,
                        |b| ParityCode::HAMMING_13_9.decode(b),
                        w,
                    );
                },
            );
            let row_half = (ALPHA[2 * pass + 1], BETA[2 * pass + 1]);
            half_pass(&channel, &mut extrinsic, rows(), row_half, |y, beta, w| {
                siso(
                    y,
                    CHASE_FLIPS,
                    beta,
                    |b| ParityCode::HAMMING_15_11.decode(b),
                    w,
                );
            });
            if Self::settle(&mut decisions(&channel, &extrinsic)) == Some(0) {
                break;
            }
        }
        let mut hard = decisions(&channel, &extrinsic);
        if Self::settle(&mut hard).is_none_or(|repairs| repairs > MAX_SETTLE_REPAIRS)
            || discrepancy(&channel, &hard) > MAX_COST
        {
            return None;
        }
        let corrected = hard
            .iter()
            .zip(&channel)
            .filter(|&(&bit, &y)| bit != (y > 0.0))
            .count() as u32;
        Some((Self::extract(&hard), corrected))
    }

    fn settle(matrix: &mut [bool; Self::CODED_BITS]) -> Option<u32> {
        let mut corrected = 0;
        for _ in 0..MAX_PASSES {
            let repaired = Self::pass(matrix)?;
            corrected += repaired;
            if repaired == 0 {
                return Some(corrected);
            }
        }
        None
    }

    fn pass(matrix: &mut [bool; Self::CODED_BITS]) -> Option<u32> {
        let mut corrected = 0;
        for c in 0..15 {
            let mut column: [bool; 13] = std::array::from_fn(|r| matrix[r * 15 + c + 1]);
            corrected += ParityCode::HAMMING_13_9.decode(&mut column)?;
            for (r, bit) in column.into_iter().enumerate() {
                matrix[r * 15 + c + 1] = bit;
            }
        }
        for r in 0..9 {
            let start = r * 15 + 1;
            corrected += ParityCode::HAMMING_15_11.decode(&mut matrix[start..start + 15])?;
        }
        Some(corrected)
    }

    fn data_positions() -> impl Iterator<Item = usize> {
        (4..=11).chain((1..9).flat_map(|row| row * 15 + 1..=row * 15 + 11))
    }

    fn extract(matrix: &[bool; Self::CODED_BITS]) -> [bool; Self::DATA_BITS] {
        let mut out = [false; Self::DATA_BITS];
        for (slot, a) in out.iter_mut().zip(Self::data_positions()) {
            *slot = matrix[a];
        }
        out
    }

    #[must_use]
    pub fn encode(data: &[bool; Self::DATA_BITS]) -> [bool; Self::CODED_BITS] {
        let mut matrix = [false; Self::CODED_BITS];
        for (a, &bit) in Self::data_positions().zip(data.iter()) {
            matrix[a] = bit;
        }
        for r in 0..9 {
            let start = r * 15 + 1;
            ParityCode::HAMMING_15_11.encode(&mut matrix[start..start + 15]);
        }
        for c in 0..15 {
            let mut column: [bool; 13] = std::array::from_fn(|r| matrix[r * 15 + c + 1]);
            ParityCode::HAMMING_13_9.encode(&mut column);
            for (r, bit) in column.into_iter().enumerate() {
                matrix[r * 15 + c + 1] = bit;
            }
        }
        let mut coded = [false; Self::CODED_BITS];
        for (a, bit) in matrix.into_iter().enumerate() {
            coded[a * 181 % Self::CODED_BITS] = bit;
        }
        coded
    }
}

pub struct Bptc128;

impl Bptc128 {
    pub const CODED_BITS: usize = 128;
    pub const DATA_BITS: usize = 77;

    fn transpose(index: usize) -> usize {
        if index == 127 { 127 } else { index * 16 % 127 }
    }

    #[must_use]
    pub fn decode(coded: &[bool; Self::CODED_BITS]) -> Option<([bool; Self::DATA_BITS], u32)> {
        let mut matrix = [false; Self::CODED_BITS];
        for (a, &bit) in coded.iter().enumerate() {
            matrix[Self::transpose(a)] = bit;
        }
        let corrected = Self::settle(&mut matrix)?;
        Some((Self::extract(&matrix), corrected))
    }

    /// Turbo product decoding as in [`Bptc196::decode_soft`], with exact min-sum parity
    /// columns.
    #[must_use]
    pub fn decode_soft(coded: &[Soft; Self::CODED_BITS]) -> Option<([bool; Self::DATA_BITS], u32)> {
        let mut transposed = [0 as Soft; Self::CODED_BITS];
        for (a, &s) in coded.iter().enumerate() {
            transposed[Self::transpose(a)] = s;
        }
        let channel = normalized(&transposed)?;
        let mut extrinsic = [0.0f32; Self::CODED_BITS];
        let rows = || (0..8).map(|r| std::array::from_fn::<_, 16, _>(|k| r * 16 + k));
        let columns = || (0..16).map(|c| std::array::from_fn::<_, 8, _>(|r| r * 16 + c));
        for pass in 0..MAX_PASSES {
            let row_half = (ALPHA[2 * pass], BETA[2 * pass]);
            half_pass(&channel, &mut extrinsic, rows(), row_half, |y, beta, w| {
                siso(
                    y,
                    CHASE_FLIPS,
                    beta,
                    |b| ParityCode::HAMMING_16_11.decode(b),
                    w,
                );
            });
            let column_half = (ALPHA[2 * pass + 1], BETA[2 * pass + 1]);
            half_pass(
                &channel,
                &mut extrinsic,
                columns(),
                column_half,
                |y, _, w| {
                    parity_siso(y, PARITY_SCALE, w);
                },
            );
            if Self::settle(&mut decisions(&channel, &extrinsic)) == Some(0) {
                break;
            }
        }
        let mut hard = decisions(&channel, &extrinsic);
        if Self::settle(&mut hard).is_none_or(|repairs| repairs > MAX_SETTLE_REPAIRS)
            || discrepancy(&channel, &hard) > MAX_COST_128
        {
            return None;
        }
        let corrected = hard
            .iter()
            .zip(&channel)
            .filter(|&(&bit, &y)| bit != (y > 0.0))
            .count() as u32;
        Some((Self::extract(&hard), corrected))
    }

    fn settle(matrix: &mut [bool; Self::CODED_BITS]) -> Option<u32> {
        let mut corrected = 0;
        for r in 0..7 {
            corrected += ParityCode::HAMMING_16_11.decode(&mut matrix[r * 16..r * 16 + 16])?;
        }
        for c in 0..16 {
            if (0..8).fold(false, |acc, r| acc ^ matrix[r * 16 + c]) {
                return None;
            }
        }
        Some(corrected)
    }

    fn extract(matrix: &[bool; Self::CODED_BITS]) -> [bool; Self::DATA_BITS] {
        let mut out = [false; Self::DATA_BITS];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = matrix[i / 11 * 16 + i % 11];
        }
        out
    }

    #[must_use]
    pub fn encode(data: &[bool; Self::DATA_BITS]) -> [bool; Self::CODED_BITS] {
        let mut matrix = [false; Self::CODED_BITS];
        for (i, &bit) in data.iter().enumerate() {
            matrix[i / 11 * 16 + i % 11] = bit;
        }
        for r in 0..7 {
            ParityCode::HAMMING_16_11.encode(&mut matrix[r * 16..r * 16 + 16]);
        }
        for c in 0..16 {
            matrix[7 * 16 + c] = (0..7).fold(false, |acc, r| acc ^ matrix[r * 16 + c]);
        }
        let mut coded = [false; Self::CODED_BITS];
        for (a, slot) in coded.iter_mut().enumerate() {
            *slot = matrix[Self::transpose(a)];
        }
        coded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern<const N: usize>(seed: u32) -> [bool; N] {
        let mut state = seed | 1;
        std::array::from_fn(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state & 1 == 1
        })
    }

    #[test]
    fn bptc196_round_trips() {
        let data: [bool; 96] = pattern(7);
        let coded = Bptc196::encode(&data);
        assert_eq!(Bptc196::decode(&coded), Some((data, 0)));
    }

    #[test]
    fn bptc196_repairs_scattered_errors() {
        let data: [bool; 96] = pattern(11);
        let mut coded = Bptc196::encode(&data);
        for bit in [3usize, 44, 91, 150, 195] {
            coded[bit] = !coded[bit];
        }
        let (decoded, corrected) = Bptc196::decode(&coded).expect("repairable");
        assert_eq!(decoded, data);
        assert!(corrected >= 5, "corrected {corrected} of 5 planted errors");
    }

    #[test]
    fn bptc128_round_trips_and_repairs_one_bit_per_row() {
        let data: [bool; 77] = pattern(3);
        let mut coded = Bptc128::encode(&data);
        assert_eq!(Bptc128::decode(&coded), Some((data, 0)));
        coded[5] = !coded[5];
        coded[70] = !coded[70];
        let (decoded, corrected) = Bptc128::decode(&coded).expect("repairable");
        assert_eq!(decoded, data);
        assert_eq!(corrected, 2);
    }

    #[test]
    fn bptc_refuses_a_block_it_cannot_reconcile() {
        let noise: [bool; 196] = pattern(99);
        let mut heavy = Bptc196::encode(&pattern::<96>(5));
        for (i, bit) in heavy.iter_mut().enumerate() {
            if i % 3 == 0 {
                *bit = noise[i];
            }
        }
        assert!(Bptc196::decode(&heavy).is_none_or(|(_, c)| c > 0));
    }

    use crate::fec::conv::{CONFIDENT, soft};

    fn soften<const N: usize>(bits: &[bool; N]) -> [Soft; N] {
        std::array::from_fn(|i| soft(bits[i]))
    }

    struct TestRng(u64);

    impl TestRng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn uniform(&mut self) -> f64 {
            (self.next() >> 11) as f64 / (1u64 << 53) as f64
        }

        fn gauss(&mut self) -> f64 {
            let u1 = 1.0 - self.uniform();
            let u2 = self.uniform();
            (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
        }

        fn noise_block<const N: usize>(&mut self) -> [Soft; N] {
            std::array::from_fn(|_| {
                let r = self.next();
                let mag = (r >> 8 & 63) as i16 + 1;
                if r & 1 == 1 { mag } else { -mag }
            })
        }
    }

    fn awgn_trial(seed: u32, sigma: f64) -> ([bool; 96], [Soft; 196]) {
        let data: [bool; 96] = pattern(seed);
        let coded = Bptc196::encode(&data);
        let mut rng = TestRng(u64::from(seed) * 0x9E37_79B9 + 1);
        let soft_block = std::array::from_fn(|i| {
            let x = if coded[i] { 1.0 } else { -1.0 };
            let y = x + sigma * rng.gauss();
            (y * 32.0)
                .round()
                .clamp(-f64::from(CONFIDENT), f64::from(CONFIDENT)) as Soft
        });
        (data, soft_block)
    }

    #[test]
    fn bptc196_soft_round_trips() {
        let data: [bool; 96] = pattern(21);
        let coded = soften(&Bptc196::encode(&data));
        assert_eq!(Bptc196::decode_soft(&coded), Some((data, 0)));
    }

    #[test]
    fn bptc128_soft_round_trips() {
        let data: [bool; 77] = pattern(23);
        let coded = soften(&Bptc128::encode(&data));
        assert_eq!(Bptc128::decode_soft(&coded), Some((data, 0)));
    }

    #[test]
    fn bptc196_soft_recovers_a_rectangle_hard_decode_cannot() {
        let data: [bool; 96] = pattern(31);
        let mut coded = soften(&Bptc196::encode(&data));
        for m in [34usize, 38, 79, 83] {
            let a = m * 181 % 196;
            coded[a] = if coded[a] > 0 { -8 } else { 8 };
        }
        let hard: [bool; 196] = std::array::from_fn(|i| coded[i] > 0);
        assert!(
            Bptc196::decode(&hard).is_none_or(|(d, _)| d != data),
            "the rectangle must defeat hard decoding for this test to mean anything"
        );
        assert_eq!(Bptc196::decode_soft(&coded), Some((data, 4)));
    }

    #[test]
    fn bptc128_soft_recovers_a_rectangle_hard_decode_cannot() {
        let data: [bool; 77] = pattern(37);
        let mut coded = soften(&Bptc128::encode(&data));
        for m in [18usize, 25, 66, 73] {
            let a = (0..128)
                .find(|&a| Bptc128::transpose(a) == m)
                .unwrap_or_default();
            coded[a] = if coded[a] > 0 { -8 } else { 8 };
        }
        let hard: [bool; 128] = std::array::from_fn(|i| coded[i] > 0);
        assert_eq!(Bptc128::decode(&hard), None);
        assert_eq!(Bptc128::decode_soft(&coded), Some((data, 4)));
    }

    #[test]
    fn bptc196_soft_reports_the_hamming_distance_to_the_input() {
        let data: [bool; 96] = pattern(41);
        let mut coded = soften(&Bptc196::encode(&data));
        for a in [3usize, 44, 91, 150, 195] {
            coded[a] = if coded[a] > 0 { -8 } else { 8 };
        }
        assert_eq!(Bptc196::decode_soft(&coded), Some((data, 5)));
    }

    #[test]
    fn bptc_soft_refuses_an_all_erasure_block() {
        assert_eq!(Bptc196::decode_soft(&[0; 196]), None);
        assert_eq!(Bptc128::decode_soft(&[0; 128]), None);
    }

    #[test]
    fn bptc_soft_decoding_is_deterministic() {
        let mut rng = TestRng(0xDEC0DE);
        for _ in 0..20 {
            let block196: [Soft; 196] = rng.noise_block();
            assert_eq!(
                Bptc196::decode_soft(&block196),
                Bptc196::decode_soft(&block196)
            );
            let block128: [Soft; 128] = rng.noise_block();
            assert_eq!(
                Bptc128::decode_soft(&block128),
                Bptc128::decode_soft(&block128)
            );
        }
    }

    #[test]
    fn bptc196_soft_acceptance_of_noise_stays_at_its_measured_rate() {
        let mut rng = TestRng(0x196_196);
        let accepted = (0..10_000)
            .filter(|_| Bptc196::decode_soft(&rng.noise_block::<196>()).is_some())
            .count();
        assert!(accepted <= 5, "accepted {accepted} of 10000 noise blocks");
    }

    #[test]
    fn bptc128_soft_acceptance_of_noise_stays_at_its_measured_rate() {
        let mut rng = TestRng(0x128_128);
        let accepted = (0..10_000)
            .filter(|_| Bptc128::decode_soft(&rng.noise_block::<128>()).is_some())
            .count();
        assert!(accepted <= 5, "accepted {accepted} of 10000 noise blocks");
    }

    #[test]
    fn bptc196_soft_decoding_beats_hard_under_awgn() {
        let mut hard_failures = 0u32;
        let mut soft_failures = 0u32;
        for seed in 0..400 {
            let (data, soft_block) = awgn_trial(seed, 0.55);
            let hard: [bool; 196] = std::array::from_fn(|i| soft_block[i] > 0);
            if Bptc196::decode(&hard).is_none_or(|(d, _)| d != data) {
                hard_failures += 1;
            }
            if Bptc196::decode_soft(&soft_block).is_none_or(|(d, _)| d != data) {
                soft_failures += 1;
            }
        }
        assert!(
            hard_failures >= 100,
            "only {hard_failures} hard failures: noise level no longer stresses the decoder"
        );
        assert!(
            soft_failures <= 20 && soft_failures * 4 <= hard_failures,
            "soft lost {soft_failures} blocks to hard's {hard_failures}"
        );
    }

    #[test]
    fn bptc196_soft_decoding_rarely_accepts_a_wrong_block() {
        let (mut hard_right, mut hard_wrong, mut soft_right, mut soft_wrong) = (0, 0, 0, 0);
        for seed in 0..2_000 {
            let (data, soft_block) = awgn_trial(seed, 0.7);
            let hard: [bool; 196] = std::array::from_fn(|i| soft_block[i] > 0);
            match Bptc196::decode(&hard) {
                Some((d, _)) if d == data => hard_right += 1,
                Some(_) => hard_wrong += 1,
                None => {}
            }
            match Bptc196::decode_soft(&soft_block) {
                Some((d, _)) if d == data => soft_right += 1,
                Some(_) => soft_wrong += 1,
                None => {}
            }
        }
        assert!(
            hard_wrong >= 20,
            "only {hard_wrong} wrong hard blocks: noise level no longer stresses the decoder"
        );
        assert!(
            soft_right > hard_right && soft_wrong * 10 <= hard_wrong,
            "soft {soft_right} right {soft_wrong} wrong, hard {hard_right} right {hard_wrong} wrong"
        );
    }

    #[test]
    fn bptc128_soft_decoding_finds_more_blocks_and_rarely_a_wrong_one() {
        let mut rng = TestRng(0xabc);
        let (mut hard_right, mut soft_right, mut soft_wrong) = (0, 0, 0);
        for seed in 0..2_000u32 {
            let data: [bool; 77] = pattern(seed * 2 + 1);
            let coded = Bptc128::encode(&data);
            let soft_block: [Soft; 128] = std::array::from_fn(|i| {
                let x = if coded[i] { 1.0 } else { -1.0 };
                ((x + 0.6 * rng.gauss()) * 32.0)
                    .round()
                    .clamp(-f64::from(CONFIDENT), f64::from(CONFIDENT)) as Soft
            });
            let hard: [bool; 128] = std::array::from_fn(|i| soft_block[i] > 0);
            if Bptc128::decode(&hard).is_some_and(|(d, _)| d == data) {
                hard_right += 1;
            }
            match Bptc128::decode_soft(&soft_block) {
                Some((d, _)) if d == data => soft_right += 1,
                Some(_) => soft_wrong += 1,
                None => {}
            }
        }
        assert!(
            soft_right > 2 * hard_right && soft_wrong * 200 <= soft_right,
            "soft {soft_right} right {soft_wrong} wrong, hard {hard_right} right"
        );
    }
}
