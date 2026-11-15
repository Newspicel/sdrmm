use num_complex::Complex;

use super::error::OfdmError;

/// The seven-stage register of the pilot polarity sequence, all ones at the start.
const M_SEQUENCE_SEED: u8 = 0x7f;
const M_SEQUENCE_PERIOD: usize = 127;
const M_SEQUENCE_TAP: u8 = 3;
const M_SEQUENCE_TOP_BIT: u8 = 6;

#[derive(Clone, Debug, PartialEq)]
pub struct PilotPattern {
    base: Vec<Complex<f32>>,
    polarity: Vec<f32>,
}

impl PilotPattern {
    pub fn new(base: Vec<Complex<f32>>, polarity: Vec<f32>) -> Result<Self, OfdmError> {
        if base.is_empty() || polarity.is_empty() {
            return Err(OfdmError::EmptyPilotPattern);
        }
        Ok(Self { base, polarity })
    }

    pub fn m_sequence(base: Vec<Complex<f32>>) -> Result<Self, OfdmError> {
        let mut state = M_SEQUENCE_SEED;
        let polarity = (0..M_SEQUENCE_PERIOD)
            .map(|_| {
                let out = state & 1;
                let feedback = (state ^ (state >> M_SEQUENCE_TAP)) & 1;
                state = (state >> 1) | (feedback << M_SEQUENCE_TOP_BIT);
                if out == 1 { 1.0 } else { -1.0 }
            })
            .collect();
        Self::new(base, polarity)
    }

    #[must_use]
    pub fn value(&self, index: usize, symbol: usize) -> Complex<f32> {
        self.base[index] * self.polarity[symbol % self.polarity.len()]
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.base.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.base.is_empty()
    }

    #[must_use]
    pub fn polarity(&self) -> &[f32] {
        &self.polarity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ofdm::OfdmParams;

    #[test]
    fn the_pilot_polarity_is_a_balanced_maximal_length_sequence() {
        let p = OfdmParams::wifi_like();
        let polarity = p.pilot_pattern().polarity();
        assert_eq!(polarity.len(), 127);
        let sum: f32 = polarity.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6, "polarity sum {sum}");
        for pilot in 0..p.pilot_pattern().len() {
            assert_eq!(
                p.pilot_pattern().value(pilot, 0),
                p.pilot_pattern().value(pilot, 127)
            );
        }
    }

    #[test]
    fn an_empty_pattern_is_rejected() {
        assert_eq!(
            PilotPattern::new(Vec::new(), vec![1.0]),
            Err(OfdmError::EmptyPilotPattern)
        );
    }
}
