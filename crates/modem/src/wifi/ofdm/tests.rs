use num_complex::Complex;
use sdrmm_modem_test_support::ber::rng::Rng;

use super::{LOOKAHEAD, Ofdm, PLATEAU_SAMPLES, Plateau, State, descramble, tables::RATES};
use crate::wifi::{WifiPhy, byte, transmit};

#[test]
fn the_service_field_seeds_the_descrambler() {
    let mut register = 0b101_1101u8;
    let data: Vec<bool> = (0..64).map(|k| k % 3 == 0 && k >= 16).collect();
    let mut scrambled: Vec<bool> = data
        .iter()
        .map(|&bit| {
            let next = (register >> 6 ^ register >> 3) & 1;
            register = (register << 1 | next) & 0x7F;
            bit ^ (next == 1)
        })
        .collect();
    descramble(&mut scrambled);
    assert_eq!(scrambled, data);
}

#[test]
fn bytes_are_least_significant_bit_first() {
    assert_eq!(
        byte(&[true, false, false, false, false, false, false, true]),
        0x81
    );
}

#[test]
fn a_frame_lasts_its_preamble_signal_and_data_symbols() {
    let mpdu = vec![0x80u8; 100];
    let iq = transmit::ofdm(&mpdu, 6);
    assert_eq!(iq.len(), 320 + 80 * (1 + RATES[0].symbols(100)) + 16);
}

fn slid(history: &[Complex<f32>]) -> Option<usize> {
    let mut plateau = Plateau::default();
    (0..=history.len() - LOOKAHEAD).find(|&at| {
        plateau.slide(history, at);
        plateau.step()
    })
}

fn noisy(lead: usize, wave: &[Complex<f32>], seed: u64) -> Vec<Complex<f32>> {
    let mut rng = Rng::new(seed);
    let mut history = vec![Complex::new(0.0, 0.0); lead];
    history.extend_from_slice(wave);
    history.resize(history.len() + 2 * LOOKAHEAD, Complex::new(0.0, 0.0));
    for sample in &mut history {
        let (a, b) = rng.normal_pair();
        *sample += Complex::new(a as f32, b as f32) * 0.05;
    }
    history
}

#[test]
fn the_grid_probe_starts_where_the_plateau_slide_starts() {
    let frame = vec![0x80u8; 60];
    let bursts = [
        transmit::ofdm(&frame, 6),
        transmit::ofdm(&frame, 54),
        transmit::dsss(&frame, WifiPhy::Cck11m, true).unwrap(),
        transmit::dsss(&frame, WifiPhy::Dsss1m, false).unwrap(),
    ];
    for (seed, lead) in [0usize, 1, 13, 23, 24, 25, 517, 4_000]
        .into_iter()
        .enumerate()
    {
        for (kind, burst) in bursts.iter().enumerate() {
            let history = noisy(lead, burst, seed as u64);
            let mut ofdm = Ofdm::new();
            ofdm.history.clone_from(&history);
            let found = ofdm.search().then_some(ofdm.state);
            let want = slid(&history).map(|at| at + 1 - PLATEAU_SAMPLES);
            assert_eq!(want.is_some(), kind < 2, "lead {lead}");
            assert_eq!(
                found.map(|state| match state {
                    State::Pending(start) => start,
                    State::Search => usize::MAX,
                }),
                want,
                "lead {lead}"
            );
        }
    }
}
