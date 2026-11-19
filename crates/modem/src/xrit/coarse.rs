use num_complex::Complex;
use sdrmm_dsp::Nco;
use sdrmm_dsp::math::fft::Transform;

/// Samples each search squares and transforms.
const SEARCH_LEN: usize = 1 << 14;
/// How far over the mean bin the doubled carrier must stand to be taken.
const MIN_PEAK_OVER_MEAN: f32 = 12.0;

/// Finds a BPSK carrier from the line its square puts at twice its offset, and mixes it to zero
/// so the phase loop only has to hold what is left. A loop narrow enough for weak signals cannot
/// pull in an LNB's kilohertz on its own.
pub struct CoarseTune {
    transform: Transform<f32>,
    squared: Vec<Complex<f32>>,
    mixed: Vec<Complex<f32>>,
    nco: Nco,
    turns: f64,
    searching: bool,
}

impl CoarseTune {
    #[must_use]
    pub fn new() -> Self {
        Self {
            transform: Transform::forward(SEARCH_LEN),
            squared: Vec::with_capacity(SEARCH_LEN),
            mixed: Vec::new(),
            nco: Nco::new(0.0, 1.0),
            turns: 0.0,
            searching: true,
        }
    }

    /// The offset now mixed out, in cycles a sample.
    #[cfg(test)]
    #[must_use]
    pub fn turns(&self) -> f64 {
        self.turns
    }

    pub fn reset(&mut self) {
        self.squared.clear();
        self.searching = true;
    }

    /// Searches again from the next samples, as when the frames stop.
    pub fn search(&mut self) {
        if !self.searching {
            self.squared.clear();
            self.searching = true;
        }
    }

    /// Holds the offset found, as when frames decode.
    pub fn hold(&mut self) {
        self.searching = false;
    }

    /// `iq` with the carrier offset taken out.
    pub fn process(&mut self, iq: &[Complex<f32>]) -> &[Complex<f32>] {
        if self.searching {
            self.observe(iq);
        }
        self.mixed.clear();
        self.mixed.resize(iq.len(), Complex::default());
        self.nco.mix_into(iq, &mut self.mixed);
        &self.mixed
    }

    fn observe(&mut self, iq: &[Complex<f32>]) {
        for &sample in iq {
            self.squared.push(sample * sample);
            if self.squared.len() == SEARCH_LEN {
                self.estimate();
                self.squared.clear();
            }
        }
    }

    fn estimate(&mut self) {
        self.transform.process(&mut self.squared);
        let mean = self.squared.iter().map(|bin| bin.norm()).sum::<f32>() / SEARCH_LEN as f32;
        let Some((peak, power)) = self
            .squared
            .iter()
            .enumerate()
            .map(|(index, bin)| (index, bin.norm()))
            .max_by(|a, b| a.1.total_cmp(&b.1))
        else {
            return;
        };
        if power < MIN_PEAK_OVER_MEAN * mean {
            return;
        }
        let signed = if peak > SEARCH_LEN / 2 {
            peak as f64 - SEARCH_LEN as f64
        } else {
            peak as f64
        };
        self.turns = signed / SEARCH_LEN as f64 / 2.0;
        self.nco.set_turns(-self.turns);
    }
}

impl Default for CoarseTune {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bpsk_carrier_is_found_and_mixed_out() {
        let offset = 0.013;
        let mut state = 7u32;
        let iq: Vec<Complex<f32>> = (0..40_000)
            .map(|index| {
                if index % 4 == 0 {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                }
                let level = if state & 1 == 0 { 1.0 } else { -1.0 };
                let phase = std::f64::consts::TAU * offset * f64::from(index);
                Complex::from_polar(level, phase as f32)
            })
            .collect();
        let mut tune = CoarseTune::new();
        tune.process(&iq);
        assert!((tune.turns() - offset).abs() < 1e-4, "{}", tune.turns());
    }
}
