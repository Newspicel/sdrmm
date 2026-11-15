use num_complex::Complex;

use super::{
    BeamError, WeightSet, ZERO,
    lanes::{common_len, snapshot},
};
use crate::math::linalg::MAX_ORDER;

pub struct WeightRamp {
    current: WeightSet,
    start: WeightSet,
    target: WeightSet,
    remaining: u32,
    total: u32,
}

impl WeightRamp {
    #[must_use]
    pub const fn new(n: usize) -> Self {
        Self {
            current: WeightSet::zeros(n),
            start: WeightSet::zeros(n),
            target: WeightSet::zeros(n),
            remaining: 0,
            total: 0,
        }
    }

    pub fn set_target(&mut self, target: &WeightSet, ramp_samples: u32) {
        self.start.clone_from(&self.current);
        self.target.clone_from(target);
        if ramp_samples == 0 || self.start.len() != target.len() {
            self.current.clone_from(target);
            self.remaining = 0;
            self.total = 0;
        } else {
            self.remaining = ramp_samples;
            self.total = ramp_samples;
        }
    }

    pub fn apply(
        &mut self,
        lanes: &[&[Complex<f32>]],
        out: &mut Vec<Complex<f32>>,
    ) -> Result<(), BeamError> {
        let n = self.current.len();
        let len = common_len(lanes, n)?;
        let ramped = len.min(self.remaining as usize);
        let mut x = [ZERO; MAX_ORDER];
        for t in 0..ramped {
            self.step();
            snapshot(lanes, t, &mut x);
            out.push(self.current.response(&x[..n]));
        }
        let from = out.len();
        out.resize(from + len - ramped, ZERO);
        let steady = &mut out[from..];
        for (lane, &weight) in lanes.iter().zip(self.current.as_slice()) {
            for (value, sample) in steady.iter_mut().zip(&lane[ramped..len]) {
                *value += sample * weight;
            }
        }
        Ok(())
    }

    #[must_use]
    pub const fn current(&self) -> &WeightSet {
        &self.current
    }

    #[must_use]
    pub const fn ramping(&self) -> bool {
        self.remaining > 0
    }

    fn step(&mut self) {
        self.remaining -= 1;
        if self.remaining == 0 {
            self.current.clone_from(&self.target);
            return;
        }
        let fraction = 1.0 - self.remaining as f32 / self.total as f32;
        for ((value, start), target) in self
            .current
            .as_mut_slice()
            .iter_mut()
            .zip(self.start.as_slice())
            .zip(self.target.as_slice())
        {
            *value = start + (target - start) * fraction;
        }
    }
}

#[cfg(test)]
mod tests {
    use num_complex::Complex;

    use super::*;

    fn close(a: &WeightSet, b: &[Complex<f32>]) -> bool {
        a.as_slice()
            .iter()
            .zip(b)
            .all(|(x, y)| (x - y).norm() < 1e-6)
    }

    #[test]
    fn weight_ramp_is_linear_and_lands_on_target() {
        let start = WeightSet::unit(2, 0);
        let target = WeightSet::from_textbook(&[Complex::new(0.2, 0.6), Complex::new(-1.0, 0.4)]);
        let mut ramp = WeightRamp::new(2);
        ramp.set_target(&start, 0);
        assert_eq!(ramp.current(), &start);
        ramp.set_target(&target, 100);
        let ones = vec![Complex::new(1.0f32, 0.0); 50];
        let zeros = vec![Complex::new(0.0f32, 0.0); 50];
        let mut out = Vec::with_capacity(200);
        ramp.apply(&[&ones, &zeros], &mut out).unwrap();
        let mean: Vec<Complex<f32>> = start
            .as_slice()
            .iter()
            .zip(target.as_slice())
            .map(|(a, b)| (a + b) * 0.5)
            .collect();
        assert!(close(ramp.current(), &mean));
        assert!((out[49] - mean[0]).norm() < 1e-6);
        let steps: Vec<f32> = out
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).norm())
            .collect();
        assert!(steps.iter().all(|step| (step - steps[0]).abs() < 1e-5));
        ramp.apply(&[&ones, &zeros], &mut out).unwrap();
        assert_eq!(ramp.current(), &target);
        assert!(!ramp.ramping());
        ramp.apply(&[&ones, &zeros], &mut out).unwrap();
        assert_eq!(out.len(), 150);
        assert!((out[149] - target.as_slice()[0]).norm() < 1e-6);
        assert_eq!(ramp.apply(&[&ones], &mut out), Err(BeamError::Lanes(1)));
        assert_eq!(
            ramp.apply(&[&ones, &zeros[..10]], &mut out),
            Err(BeamError::LaneLength)
        );
        assert_eq!(out.len(), 150);
    }
}
