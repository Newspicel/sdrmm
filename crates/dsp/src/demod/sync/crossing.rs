/// An NRZ bit clock that times itself on where between two samples the signal crosses zero.
///
/// Unlike [`super::BitSync`], which nudges its phase a fixed share toward mid-bit on any sign
/// change, this one interpolates the crossing, pulls the clock by `gain` of how far it sits
/// from a bit edge, and returns the signal interpolated at the exact mid-bit instant. That
/// suits slow, smooth waveforms with few samples per bit.
#[derive(Clone, Debug)]
pub struct CrossingClock {
    step: f64,
    gain: f64,
    phase: f64,
    previous: f32,
}

impl CrossingClock {
    #[must_use]
    pub fn new(sample_rate: f64, baud: f64, gain: f64) -> Self {
        Self {
            step: baud / sample_rate,
            gain,
            phase: 0.0,
            previous: 0.0,
        }
    }

    /// Takes one sample; returns the mid-bit value when a bit centre passed.
    pub fn push(&mut self, sample: f32) -> Option<f32> {
        let start = self.phase;
        let mut next = start + self.step;
        if (self.previous < 0.0) != (sample < 0.0) {
            let fraction = f64::from(self.previous / (self.previous - sample));
            let crossing = start + self.step * fraction;
            next -= self.gain * (crossing - crossing.round());
        }
        let symbol = (start < 0.5 && next >= 0.5).then(|| {
            let fraction = ((0.5 - start) / (next - start)) as f32;
            self.previous + (sample - self.previous) * fraction
        });
        self.phase = next.rem_euclid(1.0);
        self.previous = sample;
        symbol
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    #[test]
    fn symbols_survive_a_rate_error() {
        let baud = 4_800.0;
        let mut clock = CrossingClock::new(RATE, baud, 0.12);
        let samples_per_bit = RATE / (baud * 1.002);
        let bits: Vec<bool> = (0..2_000).map(|k| (k * 7 + k / 3) % 5 < 2).collect();
        let total = (bits.len() as f64 * samples_per_bit) as usize;
        let mut decided = Vec::new();
        for n in 0..total {
            let index = ((n as f64 / samples_per_bit) as usize).min(bits.len() - 1);
            if let Some(symbol) = clock.push(if bits[index] { 1.0 } else { -1.0 }) {
                decided.push(symbol > 0.0);
            }
        }
        let tail = &decided[decided.len() - 1_000..];
        let found = (0..bits.len() - 1_000).any(|offset| {
            bits[offset..offset + 1_000]
                .iter()
                .zip(tail)
                .all(|(a, b)| a == b)
        });
        assert!(found, "the decided bits do not follow the sent ones");
    }

    #[test]
    fn the_mid_bit_value_is_interpolated() {
        let mut clock = CrossingClock::new(RATE, RATE * 0.4, 0.0);
        assert_eq!(clock.push(0.0), None);
        let centre = clock.push(4.0).expect("a bit centre between the samples");
        assert!((centre - 1.0).abs() < 1e-5, "{centre}");
        assert_eq!(clock.push(4.0), None);
    }
}
