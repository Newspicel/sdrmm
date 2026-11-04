use num_complex::Complex;

pub(super) struct Fir {
    taps: Vec<f32>,
    history: Vec<Complex<f32>>,
    head: usize,
    decimation: usize,
    phase: usize,
}

impl Fir {
    pub(super) fn new(taps: Vec<f32>, decimation: usize) -> Self {
        let length = taps.len();
        Self {
            taps,
            history: vec![Complex::new(0.0, 0.0); 2 * length],
            head: 0,
            decimation: decimation.max(1),
            phase: 0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.history.fill(Complex::new(0.0, 0.0));
        self.head = 0;
        self.phase = 0;
    }

    pub(super) fn filter(&mut self, sample: Complex<f32>) -> Complex<f32> {
        self.push(sample);
        self.output()
    }

    pub(super) fn process(&mut self, input: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        for &sample in input {
            self.push(sample);
            self.phase += 1;
            if self.phase == self.decimation {
                self.phase = 0;
                out.push(self.output());
            }
        }
    }

    fn push(&mut self, sample: Complex<f32>) {
        let length = self.taps.len();
        self.head = if self.head == 0 {
            length - 1
        } else {
            self.head - 1
        };
        self.history[self.head] = sample;
        self.history[self.head + length] = sample;
    }

    fn output(&self) -> Complex<f32> {
        self.history[self.head..self.head + self.taps.len()]
            .iter()
            .zip(&self.taps)
            .fold(Complex::new(0.0, 0.0), |sum, (value, &tap)| {
                sum + *value * tap
            })
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_dsp::design_lowpass_blackman_harris;

    use super::*;

    #[test]
    fn lowpass_passes_dc_and_blocks_high_frequencies() {
        let mut fir = Fir::new(design_lowpass_blackman_harris(101, 0.1), 1);
        let mut out = Vec::new();
        fir.process(&vec![Complex::new(1.0, 0.0); 512], &mut out);
        let mean = out[200..].iter().map(|value| value.re).sum::<f32>() / 312.0;
        assert!((mean - 1.0).abs() < 1e-3);
        let tone: Vec<Complex<f32>> = (0..512)
            .map(|index| Complex::from_polar(1.0, (index as f32) * 0.4 * std::f32::consts::TAU))
            .collect();
        out.clear();
        fir.process(&tone, &mut out);
        let power = out[200..].iter().map(|value| value.norm_sqr()).sum::<f32>() / 312.0;
        assert!(power < 1e-6);
    }

    #[test]
    fn decimation_keeps_every_second_output() {
        let taps = design_lowpass_blackman_harris(15, 0.05);
        let input: Vec<Complex<f32>> = (0..64)
            .map(|index| Complex::new(index as f32, -(index as f32)))
            .collect();
        let mut full = Fir::new(taps.clone(), 1);
        let mut half = Fir::new(taps, 2);
        let (mut every, mut decimated) = (Vec::new(), Vec::new());
        full.process(&input, &mut every);
        half.process(&input, &mut decimated);
        let expected: Vec<_> = every.iter().skip(1).step_by(2).copied().collect();
        assert_eq!(decimated, expected);
    }
}
