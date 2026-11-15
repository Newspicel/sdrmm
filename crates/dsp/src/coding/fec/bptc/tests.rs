use super::*;
use crate::{
    coding::fec::conv::{CONFIDENT, Soft, soft},
    math::rng::Rng,
    testutil::XorShift32,
};

const SOFT_PER_AMPLITUDE: f64 = CONFIDENT as f64 / 2.0;

fn pattern<const N: usize>(seed: u32) -> [bool; N] {
    let mut rng = XorShift32(seed | 1);
    std::array::from_fn(|_| rng.next_u32() & 1 == 1)
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

fn soften<const N: usize>(bits: &[bool; N]) -> [Soft; N] {
    std::array::from_fn(|i| soft(bits[i]))
}

fn noise_block<const N: usize>(rng: &mut Rng) -> [Soft; N] {
    std::array::from_fn(|_| {
        let r = rng.bits();
        let mag = (r >> 8 & 63) as i16 + 1;
        if r & 1 == 1 { mag } else { -mag }
    })
}

fn awgn_trial(seed: u32, sigma: f64) -> ([bool; 96], [Soft; 196]) {
    let data: [bool; 96] = pattern(seed);
    let coded = Bptc196::encode(&data);
    let mut rng = Rng::mixed(u64::from(seed));
    let soft_block = std::array::from_fn(|i| {
        let x = if coded[i] { 1.0 } else { -1.0 };
        let y = x + sigma * rng.normal();
        (y * SOFT_PER_AMPLITUDE)
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
    let mut rng = Rng::from_state(0xDEC0DE);
    for _ in 0..20 {
        let block196: [Soft; 196] = noise_block(&mut rng);
        assert_eq!(
            Bptc196::decode_soft(&block196),
            Bptc196::decode_soft(&block196)
        );
        let block128: [Soft; 128] = noise_block(&mut rng);
        assert_eq!(
            Bptc128::decode_soft(&block128),
            Bptc128::decode_soft(&block128)
        );
    }
}

#[test]
fn bptc196_soft_acceptance_of_noise_stays_at_its_measured_rate() {
    let mut rng = Rng::from_state(0x196_196);
    let accepted = (0..10_000)
        .filter(|_| Bptc196::decode_soft(&noise_block::<196>(&mut rng)).is_some())
        .count();
    assert!(accepted <= 5, "accepted {accepted} of 10000 noise blocks");
}

#[test]
fn bptc128_soft_acceptance_of_noise_stays_at_its_measured_rate() {
    let mut rng = Rng::from_state(0x128_128);
    let accepted = (0..10_000)
        .filter(|_| Bptc128::decode_soft(&noise_block::<128>(&mut rng)).is_some())
        .count();
    assert!(accepted <= 5, "accepted {accepted} of 10000 noise blocks");
}

#[derive(Default)]
struct Tally {
    right: u32,
    wrong: u32,
    refused: u32,
}

impl Tally {
    fn score(&mut self, decoded: Option<([bool; 96], u32)>, data: &[bool; 96]) {
        match decoded {
            Some((d, _)) if d == *data => self.right += 1,
            Some(_) => self.wrong += 1,
            None => self.refused += 1,
        }
    }

    fn failed(&self) -> u32 {
        self.wrong + self.refused
    }
}

/// Hard and soft decoding of the same `trials` AWGN blocks at `sigma`.
fn awgn_tallies(sigma: f64, trials: u32) -> (Tally, Tally) {
    let (mut hard, mut soft) = (Tally::default(), Tally::default());
    for seed in 0..trials {
        let (data, soft_block) = awgn_trial(seed, sigma);
        let signs: [bool; 196] = std::array::from_fn(|i| soft_block[i] > 0);
        hard.score(Bptc196::decode(&signs), &data);
        soft.score(Bptc196::decode_soft(&soft_block), &data);
    }
    (hard, soft)
}

/// Measured over 200000 blocks at sigma 0.55: hard decoding fails 36.4%, soft 0.005%. Here that
/// is about 364 hard failures (300 is 4.2 deviations below) against 0.05 soft ones.
#[test]
fn bptc196_soft_decoding_beats_hard_under_awgn() {
    let (hard, soft) = awgn_tallies(0.55, 1_000);
    assert!(
        hard.failed() >= 300,
        "only {} hard failures: noise level no longer stresses the decoder",
        hard.failed()
    );
    assert!(
        soft.failed() * 20 <= hard.failed(),
        "soft lost {} blocks to hard's {}",
        soft.failed(),
        hard.failed()
    );
}

/// Measured over 200000 blocks at sigma 0.6: hard decoding is right 41.5% and wrong 2.2%, soft
/// right 99.8% and wrong 0.02%. Here that is about 131 wrong hard blocks against 1.4 soft
/// ones; even 90 hard ones, 3.6 deviations low, leave room for 9 soft. At sigma 0.7 soft is
/// wrong 0.33% to hard's 2.4%, and four in five of those blocks lie closer to the received
/// values than the sent one: a maximum likelihood decoder would pick them too.
#[test]
fn bptc196_soft_decoding_rarely_accepts_a_wrong_block() {
    let (hard, soft) = awgn_tallies(0.6, 6_000);
    assert!(
        hard.wrong >= 90,
        "only {} wrong hard blocks: noise level no longer stresses the decoder",
        hard.wrong
    );
    assert!(
        soft.right > 2 * hard.right && soft.wrong * 10 <= hard.wrong,
        "soft {} right {} wrong, hard {} right {} wrong",
        soft.right,
        soft.wrong,
        hard.right,
        hard.wrong
    );
}

#[test]
fn bptc128_soft_decoding_finds_more_blocks_and_rarely_a_wrong_one() {
    let mut rng = Rng::from_state(0xabc);
    let (mut hard_right, mut soft_right, mut soft_wrong) = (0, 0, 0);
    for seed in 0..2_000u32 {
        let data: [bool; 77] = pattern(seed * 2 + 1);
        let coded = Bptc128::encode(&data);
        let soft_block: [Soft; 128] = std::array::from_fn(|i| {
            let x = if coded[i] { 1.0 } else { -1.0 };
            ((x + 0.6 * rng.normal()) * 32.0)
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
