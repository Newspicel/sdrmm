use num_complex::Complex;

#[derive(Clone, Debug, Default)]
pub struct DifferentialDetector {
    previous: Option<Complex<f32>>,
}

impl DifferentialDetector {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process(&mut self, symbols: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        for &current in symbols {
            if let Some(previous) = self.previous {
                out.push(product(previous, current));
            }
            self.previous = Some(current);
        }
    }

    pub fn reset(&mut self) {
        self.previous = None;
    }
}

pub fn differential_detect(symbols: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
    out.extend(symbols.windows(2).map(|pair| product(pair[0], pair[1])));
}

/// The current symbol turned back by the previous one's phase, keeping the current amplitude.
/// An origin symbol casts no vote.
fn product(previous: Complex<f32>, current: Complex<f32>) -> Complex<f32> {
    let magnitude = previous.norm();
    if magnitude > 0.0 {
        current * previous.conj() / magnitude
    } else {
        Complex::new(0.0, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_modem_test_support::ber::rng::Rng;

    use std::f64::consts::{FRAC_PI_2, TAU};

    use super::*;
    use crate::{
        constellation::tables,
        linear::rotation::phasor,
        symbolcode::{DifferentialSymbolDecoder, DifferentialSymbolEncoder},
    };

    #[test]
    fn an_unknown_carrier_phase_costs_nothing() {
        let table = tables::psk(4).unwrap();
        let mut rng = Rng::new(0xd9c);
        let data: Vec<u32> = (0..500).map(|_| (rng.next_u64() % 4) as u32).collect();
        let mut encoder = DifferentialSymbolEncoder::new(4);
        let sent: Vec<u32> = std::iter::once(0)
            .chain(data.iter().map(|&s| encoder.encode(s)))
            .collect();
        let point = |index: u32| {
            let i = table
                .labels()
                .iter()
                .position(|&l| l == tables::gray(index))
                .unwrap();
            table.points()[i]
        };
        for phase_turns in [0.0f64, 0.1, 0.25, 0.5, 0.77] {
            let rot = phasor(TAU * phase_turns);
            let wave: Vec<Complex<f32>> = sent.iter().map(|&s| point(s) * rot).collect();
            let mut products = Vec::new();
            differential_detect(&wave, &mut products);
            let steps: Vec<u32> = products
                .iter()
                .map(|&z| {
                    let label = table.hard_slice(z);
                    (0..4).find(|&i| tables::gray(i) == label).unwrap()
                })
                .collect();
            let mut decoder = DifferentialSymbolDecoder::new(4);
            let via_decoder: Vec<u32> = decoder.decode_all(&sent)[1..].to_vec();
            assert_eq!(steps, data, "phase {phase_turns}: steps");
            assert_eq!(via_decoder, data, "phase {phase_turns}: decoder pairing");
        }
    }

    #[test]
    fn pi4_dqpsk_differences_land_on_the_rotated_qpsk_table() {
        let differences = tables::psk_rotated(4, tables::PI_4_ROTATION).unwrap();
        let mut rng = Rng::new(0x914);
        let mut phase = 0.0f64;
        let mut wave = vec![Complex::new(1.0f32, 0.0)];
        let mut steps = Vec::new();
        for _ in 0..200 {
            let data = (rng.next_u64() % 4) as u32;
            steps.push(data);
            phase += FRAC_PI_2 * f64::from(data) + tables::PI_4_ROTATION;
            wave.push(phasor(phase));
        }
        let mut products = Vec::new();
        differential_detect(&wave, &mut products);
        assert_eq!(products.len(), steps.len());
        for (&z, &data) in products.iter().zip(&steps) {
            assert!(
                (z - differences.nearest(z)).norm() < 1e-5,
                "product {z} is not on the difference table"
            );
            let sent = phasor(FRAC_PI_2 * f64::from(data) + tables::PI_4_ROTATION);
            assert!((z - sent).norm() < 1e-5, "product {z} is not step {data}");
        }
    }

    #[test]
    fn the_product_carries_the_current_symbols_amplitude() {
        let a = Complex::new(2.0f32, 0.0);
        let b = Complex::new(0.0f32, 3.0);
        let mut out = Vec::new();
        differential_detect(&[a, b], &mut out);
        assert!((out[0].norm() - 3.0).abs() < 1e-6, "{}", out[0]);
        assert!((out[0].arg() - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    }

    #[test]
    fn streaming_matches_the_block_form_and_an_origin_casts_no_vote() {
        let mut rng = Rng::new(0x5719);
        let wave: Vec<Complex<f32>> = (0..300)
            .map(|_| phasor(TAU * (rng.next_u64() % 8) as f64 / 8.0))
            .collect();
        let mut whole = Vec::new();
        differential_detect(&wave, &mut whole);
        let mut detector = DifferentialDetector::new();
        let mut split = Vec::new();
        for chunk in wave.chunks(29) {
            detector.process(chunk, &mut split);
        }
        assert_eq!(whole, split);

        let mut out = Vec::new();
        differential_detect(&[Complex::new(0.0, 0.0), Complex::new(1.0, 1.0)], &mut out);
        assert_eq!(out, [Complex::new(0.0, 0.0)]);
    }
}
