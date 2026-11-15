use std::f64::consts::TAU;

#[derive(Clone, Debug)]
pub struct Goertzel {
    coeff: f32,
    norm: f32,
    block: usize,
    n: usize,
    s1: f32,
    s2: f32,
}

impl Goertzel {
    #[must_use]
    pub fn new(sample_rate: f64, freq_hz: f64, block: usize) -> Self {
        assert!(sample_rate > 0.0, "sample rate must be positive");
        assert!(
            freq_hz > 0.0 && freq_hz < sample_rate / 2.0,
            "freq must lie inside the Nyquist band"
        );
        assert!(block >= 2, "block must hold at least two samples");
        Self {
            coeff: 2.0 * (TAU * freq_hz / sample_rate).cos() as f32,
            norm: (2.0 / block as f64).powi(2) as f32,
            block,
            n: 0,
            s1: 0.0,
            s2: 0.0,
        }
    }

    pub fn push(&mut self, sample: f32) -> Option<f32> {
        let s0 = sample + self.coeff * self.s1 - self.s2;
        self.s2 = self.s1;
        self.s1 = s0;
        self.n += 1;
        if self.n < self.block {
            return None;
        }
        let power = self.s1 * self.s1 + self.s2 * self.s2 - self.coeff * self.s1 * self.s2;
        self.reset();
        Some(power * self.norm)
    }

    pub fn reset(&mut self) {
        self.n = 0;
        self.s1 = 0.0;
        self.s2 = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::real_tone;

    const RATE: f64 = 48_000.0;
    const MARK_HZ: f64 = 1_200.0;

    #[test]
    fn goertzel_separates_its_bin_from_the_next_one() {
        let block = 40;
        let power = |freq: f64| {
            let mut g = Goertzel::new(RATE, MARK_HZ, block);
            let mut last = 0.0;
            for &s in &real_tone(freq / RATE, block * 10) {
                if let Some(p) = g.push(s) {
                    last = p;
                }
            }
            last
        };
        let on_bin = power(MARK_HZ);
        let off_bin = power(2_400.0);
        assert!((on_bin - 1.0).abs() < 0.05, "on-bin power {on_bin}");
        assert!(
            off_bin < on_bin / 1e4,
            "off-bin power {off_bin} vs {on_bin}"
        );
    }

    #[test]
    fn goertzel_emits_once_per_block_and_resets() {
        let mut g = Goertzel::new(RATE, MARK_HZ, 40);
        let emitted = real_tone(MARK_HZ / RATE, 200)
            .iter()
            .filter_map(|&s| g.push(s))
            .count();
        assert_eq!(emitted, 5);
    }
}
