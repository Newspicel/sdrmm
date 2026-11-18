/// The mean of the last `len` samples, one sample at a time. The running sum is kept in
/// `f64` so a long stream does not drift.
#[derive(Clone, Debug)]
pub struct MovingAverage {
    ring: Vec<f32>,
    position: usize,
    sum: f64,
}

impl MovingAverage {
    /// A window of at least one sample.
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            ring: vec![0.0; len.max(1)],
            position: 0,
            sum: 0.0,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    /// Takes one sample and returns the mean of the window, counting not yet filled slots as
    /// zero.
    pub fn push(&mut self, sample: f32) -> f32 {
        self.sum += f64::from(sample) - f64::from(self.ring[self.position]);
        self.ring[self.position] = sample;
        self.position = (self.position + 1) % self.ring.len();
        (self.sum / self.ring.len() as f64) as f32
    }

    pub fn reset(&mut self) {
        self.ring.fill(0.0);
        self.position = 0;
        self.sum = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mean_fills_then_follows_the_window() {
        let mut average = MovingAverage::new(4);
        let means: Vec<f32> = [4.0, 4.0, 4.0, 4.0, 8.0, 8.0]
            .map(|x| average.push(x))
            .to_vec();
        assert_eq!(means, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        average.reset();
        assert_eq!(average.push(2.0), 0.5);
        assert_eq!(MovingAverage::new(0).len(), 1);
    }
}
