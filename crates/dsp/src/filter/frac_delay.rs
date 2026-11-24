use crate::math::fastmath::madd;

#[derive(Clone, Copy, Debug)]
enum Mode {
    Linear { frac: f32 },
    Cubic { c: [f32; 4] },
}

/// A delay of any non-negative number of samples, with linear ([`Self::new`]) or
/// 4-tap cubic Lagrange ([`Self::cubic`]) fractional interpolation.
#[derive(Clone, Debug)]
pub struct FracDelay {
    ring: Vec<f32>,
    position: usize,
    whole: usize,
    mode: Mode,
}

impl FracDelay {
    #[must_use]
    pub fn new(delay: f64) -> Self {
        let delay = delay.max(0.0);
        let whole = delay.floor() as usize;
        Self {
            ring: vec![0.0; delay.ceil() as usize + 2],
            position: 0,
            whole,
            mode: Mode::Linear {
                frac: (delay - whole as f64) as f32,
            },
        }
    }

    /// 4-tap cubic Lagrange fractional delay (`delay >= 1.0` for centered interpolation,
    /// clamped to `>= 1.0` if smaller).
    #[must_use]
    pub fn cubic(delay: f64) -> Self {
        let delay = delay.max(1.0);
        let whole = delay.floor() as usize;
        let mu = delay - whole as f64;
        let c0 = -mu * (mu - 1.0) * (mu - 2.0) / 6.0;
        let c1 = (mu + 1.0) * (mu - 1.0) * (mu - 2.0) / 2.0;
        let c2 = -(mu + 1.0) * mu * (mu - 2.0) / 2.0;
        let c3 = (mu + 1.0) * mu * (mu - 1.0) / 6.0;
        Self {
            ring: vec![0.0; whole + 4],
            position: 0,
            whole,
            mode: Mode::Cubic {
                c: [c0 as f32, c1 as f32, c2 as f32, c3 as f32],
            },
        }
    }

    #[inline]
    pub fn push(&mut self, sample: f32) -> f32 {
        let length = self.ring.len();
        let pos = self.position;
        self.ring[pos] = sample;
        let out = match self.mode {
            Mode::Linear { frac } => {
                let a = self.ring[(pos + length - self.whole) % length];
                let b = self.ring[(pos + length - self.whole - 1) % length];
                a * (1.0 - frac) + b * frac
            }
            Mode::Cubic { c } => {
                let xm1 = self.ring[(pos + length + 1 - self.whole) % length];
                let x0 = self.ring[(pos + length - self.whole) % length];
                let x1 = self.ring[(pos + length - self.whole - 1) % length];
                let x2 = self.ring[(pos + length - self.whole - 2) % length];
                madd(c[0], xm1, madd(c[1], x0, madd(c[2], x1, c[3] * x2)))
            }
        };
        let next = pos + 1;
        self.position = if next == length { 0 } else { next };
        out
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        out.clear();
        out.reserve(input.len());
        for &sample in input {
            out.push(self.push(sample));
        }
    }

    pub fn reset(&mut self) {
        self.ring.fill(0.0);
        self.position = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use super::*;

    #[test]
    fn whole_and_half_delays_land_where_expected() {
        let mut whole = FracDelay::new(2.0);
        let out: Vec<f32> = [1.0, 2.0, 3.0, 4.0].map(|x| whole.push(x)).to_vec();
        assert_eq!(out, [0.0, 0.0, 1.0, 2.0]);
        let mut half = FracDelay::new(0.5);
        let out: Vec<f32> = [2.0, 4.0, 6.0].map(|x| half.push(x)).to_vec();
        assert_eq!(out, [1.0, 3.0, 5.0]);
    }

    #[test]
    fn cubic_delay_tracks_a_fractional_phase_more_accurately_than_linear() {
        let delay = 3.35f64;
        let freq = 0.1;
        let input: Vec<f32> = (0..256)
            .map(|n: i32| (TAU * freq * f64::from(n)).sin() as f32)
            .collect();
        let mut linear = FracDelay::new(delay);
        let mut cubic = FracDelay::cubic(delay);
        let mut lin_out = Vec::new();
        let mut cub_out = Vec::new();
        linear.process(&input, &mut lin_out);
        cubic.process(&input, &mut cub_out);
        let err = |out: &[f32]| {
            out[16..]
                .iter()
                .enumerate()
                .map(|(i, &y)| {
                    let exact = (TAU * freq * ((i + 16) as f64 - delay)).sin() as f32;
                    (y - exact).abs()
                })
                .fold(0.0f32, f32::max)
        };
        let lin_err = err(&lin_out);
        let cub_err = err(&cub_out);
        assert!(cub_err < 0.02, "cubic error {cub_err}");
        assert!(
            cub_err * 5.0 < lin_err,
            "cubic {cub_err} vs linear {lin_err}"
        );
    }
}
