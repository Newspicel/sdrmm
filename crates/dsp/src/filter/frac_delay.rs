/// A delay of any non-negative number of samples, the fraction by linear interpolation
/// between the two nearest whole delays.
#[derive(Clone, Debug)]
pub struct FracDelay {
    ring: Vec<f32>,
    position: usize,
    whole: usize,
    fraction: f32,
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
            fraction: (delay - whole as f64) as f32,
        }
    }

    pub fn push(&mut self, sample: f32) -> f32 {
        let length = self.ring.len();
        self.ring[self.position] = sample;
        let a = self.ring[(self.position + length - self.whole) % length];
        let b = self.ring[(self.position + length - self.whole - 1) % length];
        self.position = (self.position + 1) % length;
        a * (1.0 - self.fraction) + b * self.fraction
    }

    pub fn reset(&mut self) {
        self.ring.fill(0.0);
        self.position = 0;
    }
}

#[cfg(test)]
mod tests {
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
}
