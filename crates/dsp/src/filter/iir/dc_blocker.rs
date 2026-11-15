use super::STRIDE;

#[derive(Clone, Debug, Default)]
pub struct DcBlocker {
    x1: f32,
    y1: f32,
}

impl DcBlocker {
    const POLE: f32 = 0.995;
    const POWERS: [f32; STRIDE + 1] = [
        1.0,
        Self::POLE,
        Self::POLE * Self::POLE,
        Self::POLE * Self::POLE * Self::POLE,
        Self::POLE * Self::POLE * Self::POLE * Self::POLE,
    ];

    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        if !(self.x1.is_finite() && self.y1.is_finite()) {
            self.x1 = 0.0;
            self.y1 = 0.0;
        }
        let (strides, tail) = samples.as_chunks_mut::<STRIDE>();
        for stride in strides {
            self.stride(stride);
        }
        for s in tail {
            let y = *s - self.x1 + Self::POLE * self.y1;
            self.x1 = *s;
            self.y1 = y;
            *s = y;
        }
    }

    fn stride(&mut self, stride: &mut [f32; STRIDE]) {
        let [x0, x1, x2, x3] = *stride;
        let [_, a1, a2, a3, a4] = Self::POWERS;
        let [d0, d1, d2, d3] = [x0 - self.x1, x1 - x0, x2 - x1, x3 - x2];
        let y = self.y1;
        let y0 = d0 + a1 * y;
        let y1 = d1 + a1 * d0 + a2 * y;
        let y2 = d2 + a1 * d1 + a2 * d0 + a3 * y;
        let y3 = d3 + a1 * d2 + a2 * d1 + a3 * d0 + a4 * y;
        *stride = [y0, y1, y2, y3];
        self.x1 = x3;
        self.y1 = y3;
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_1_SQRT_2;

    use super::*;
    use crate::testutil::real_tone;
    use crate::testutil::rms_r;

    #[test]
    fn strided_real_dc_blocking_matches_the_recursion_for_any_split() {
        let input: Vec<f32> = (0..1_003)
            .map(|n| ((n * 7919) % 4096) as f32 / 2048.0 - 0.7)
            .collect();
        let mut x1 = 0.0f64;
        let mut y1 = 0.0f64;
        let reference: Vec<f64> = input
            .iter()
            .map(|&x| {
                let y = f64::from(x) - x1 + f64::from(DcBlocker::POLE) * y1;
                x1 = f64::from(x);
                y1 = y;
                y
            })
            .collect();
        for split in [1, 3, 4, 5, 64, 1_003] {
            let mut blocker = DcBlocker::new();
            let mut output = input.clone();
            for chunk in output.chunks_mut(split) {
                blocker.process(chunk);
            }
            for (n, (&got, &want)) in output.iter().zip(&reference).enumerate() {
                assert!(
                    (f64::from(got) - want).abs() < 1e-4,
                    "split {split}, sample {n}: {got} vs {want}"
                );
            }
        }
    }

    #[test]
    fn dc_blocker_recovers_after_non_finite_sample() {
        let mut blocker = DcBlocker::new();
        let mut poisoned = vec![0.5f32; 480];
        poisoned[7] = f32::INFINITY;
        blocker.process(&mut poisoned);

        let mut tone = real_tone(1_000.0 / 48_000.0, 48_000);
        blocker.process(&mut tone);
        assert!(tone.iter().all(|v| v.is_finite()), "state still poisoned");
        let gain = rms_r(&tone[4_800..]) / FRAC_1_SQRT_2;
        assert!((0.891..1.122).contains(&gain), "post-recovery gain {gain}");
    }
    #[test]
    fn dc_blocker_kills_dc_and_passes_1_khz() {
        let mut blocker = DcBlocker::new();
        let mut dc = vec![1.0f32; 48_000];
        blocker.process(&mut dc);
        let tail = dc[dc.len() - 1].abs();
        assert!(tail < 0.01, "dc residue {tail}");

        let mut blocker = DcBlocker::new();
        let mut tone = real_tone(1_000.0 / 48_000.0, 48_000);
        blocker.process(&mut tone);
        let gain = rms_r(&tone[4_800..]) / FRAC_1_SQRT_2;
        assert!((0.891..1.122).contains(&gain), "1 kHz gain {gain}");
    }
}
