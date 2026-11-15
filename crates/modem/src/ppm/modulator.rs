use num_complex::Complex;

use super::waveform::SlotWaveform;

#[derive(Clone, Debug)]
pub struct PpmMod {
    m: usize,
    waveform: SlotWaveform,
    timeline: Vec<bool>,
}

impl PpmMod {
    #[must_use]
    pub fn new(m: usize, samples_per_slot: f64, phase: f64, level: f32) -> Self {
        assert!(
            m >= 2 && m.is_power_of_two(),
            "M must be a power of two of at least 2, got {m}"
        );
        Self {
            m,
            waveform: SlotWaveform::new(samples_per_slot, phase, level),
            timeline: Vec::new(),
        }
    }

    #[must_use]
    pub fn m(&self) -> usize {
        self.m
    }

    #[must_use]
    pub fn bits_per_symbol(&self) -> u32 {
        self.m.trailing_zeros()
    }

    #[must_use]
    pub fn waveform(&self) -> SlotWaveform {
        self.waveform
    }

    pub fn modulate(&mut self, symbols: &[u8], out: &mut Vec<Complex<f32>>) {
        self.timeline.clear();
        self.timeline.resize(symbols.len() * self.m, false);
        for (i, &symbol) in symbols.iter().enumerate() {
            self.timeline[i * self.m + (symbol as usize & (self.m - 1))] = true;
        }
        self.waveform.render(&self.timeline, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_k_keys_slot_k() {
        let mut m = PpmMod::new(4, 2.0, 0.0, 1.0);
        let mut out = Vec::new();
        m.modulate(&[2, 0, 3], &mut out);
        let lit: Vec<usize> = out
            .iter()
            .enumerate()
            .filter(|(_, s)| s.re > 0.5)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(lit, vec![4, 5, 8, 9, 22, 23]);
    }
}
