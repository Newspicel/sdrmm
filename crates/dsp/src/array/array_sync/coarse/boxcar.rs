use num_complex::Complex;

use super::CoarseError;

/// Averages each lane over `factor` samples, all lanes on the same block boundaries.
pub struct Boxcar {
    factor: usize,
    acc: Vec<Complex<f32>>,
    filled: usize,
}

impl Boxcar {
    #[must_use]
    pub fn new(lanes: usize, factor: usize) -> Self {
        Self {
            factor: factor.max(1),
            acc: vec![Complex::default(); lanes],
            filled: 0,
        }
    }

    #[must_use]
    pub const fn factor(&self) -> usize {
        self.factor
    }

    pub fn reset(&mut self) {
        self.acc.fill(Complex::default());
        self.filled = 0;
    }

    pub fn push(
        &mut self,
        lanes: &[&[Complex<f32>]],
        out: &mut [Vec<Complex<f32>>],
    ) -> Result<(), CoarseError> {
        let expected = self.acc.len();
        if let Some(got) = [lanes.len(), out.len()]
            .into_iter()
            .find(|&count| count != expected)
        {
            return Err(CoarseError::LaneCount { expected, got });
        }
        let samples = lanes.first().map_or(0, |lane| lane.len());
        if lanes.iter().any(|lane| lane.len() != samples) {
            return Err(CoarseError::LaneLength);
        }
        let scale = 1.0 / self.factor as f32;
        let mut at = 0;
        while at < samples {
            let take = (self.factor - self.filled).min(samples - at);
            for (acc, lane) in self.acc.iter_mut().zip(lanes) {
                *acc += lane[at..at + take].iter().sum::<Complex<f32>>();
            }
            self.filled += take;
            at += take;
            if self.filled == self.factor {
                for (acc, out) in self.acc.iter_mut().zip(out.iter_mut()) {
                    out.push(*acc * scale);
                    *acc = Complex::default();
                }
                self.filled = 0;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::array_sync::signals::Gaussian;

    #[test]
    fn boxcar_keeps_the_decimated_phase_common_to_all_lanes() {
        let source = Gaussian::new(30).block(64 * 50 + 17);
        let gains = [
            Complex::new(1.0, 0.0),
            Complex::from_polar(0.5, 2.0),
            Complex::from_polar(2.0, -1.0),
        ];
        let lanes: Vec<Vec<Complex<f32>>> = gains
            .iter()
            .map(|gain| source.iter().map(|value| value * gain).collect())
            .collect();
        let mut boxcar = Boxcar::new(3, 64);
        let mut out = vec![Vec::new(); 3];
        let mut at = 0;
        for size in [1, 63, 64, 65, 7, 1000, 2000, 50].into_iter().cycle() {
            let end = (at + size).min(source.len());
            let views: Vec<&[Complex<f32>]> = lanes.iter().map(|lane| &lane[at..end]).collect();
            boxcar.push(&views, &mut out).unwrap();
            at = end;
            if at == source.len() {
                break;
            }
        }
        assert!(out.iter().all(|lane| lane.len() == 50));
        for (index, value) in out[0].iter().enumerate() {
            let expected = source[index * 64..(index + 1) * 64]
                .iter()
                .sum::<Complex<f32>>()
                / 64.0;
            assert!((value - expected).norm() < 1e-5);
            for (lane, gain) in out.iter().zip(gains).skip(1) {
                assert!((lane[index] - value * gain).norm() < 1e-5);
            }
        }
    }

    #[test]
    fn a_boxcar_refuses_mismatched_lanes() {
        let mut boxcar = Boxcar::new(2, 4);
        let long = [Complex::new(1.0, 0.0); 8];
        let short = [Complex::new(1.0, 0.0); 7];
        let mut out = vec![Vec::new(); 2];
        assert_eq!(
            boxcar.push(&[&long], &mut out),
            Err(CoarseError::LaneCount {
                expected: 2,
                got: 1
            })
        );
        assert_eq!(
            boxcar.push(&[&long, &long], &mut out[..1]),
            Err(CoarseError::LaneCount {
                expected: 2,
                got: 1
            })
        );
        assert_eq!(
            boxcar.push(&[&long, &short], &mut out),
            Err(CoarseError::LaneLength)
        );
        assert!(out.iter().all(Vec::is_empty));
        assert_eq!(boxcar.push(&[&long, &long], &mut out), Ok(()));
        assert!(out.iter().all(|lane| lane.len() == 2));
    }
}
