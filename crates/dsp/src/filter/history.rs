/// The last `capacity` samples of a stream, addressed by their absolute index since the start.
/// Reads outside the window return silence.
#[derive(Clone, Debug)]
pub struct SampleHistory {
    buf: Vec<f32>,
    head: u64,
}

impl SampleHistory {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        assert!(capacity.is_power_of_two(), "capacity is a power of two");
        Self {
            buf: vec![0.0; capacity],
            head: 0,
        }
    }

    pub fn push(&mut self, value: f32) {
        let mask = self.buf.len() - 1;
        self.buf[(self.head as usize) & mask] = value;
        self.head += 1;
    }

    /// The index the next sample will take.
    #[must_use]
    pub const fn head(&self) -> u64 {
        self.head
    }

    #[must_use]
    pub fn oldest(&self) -> u64 {
        self.head.saturating_sub(self.buf.len() as u64)
    }

    #[must_use]
    pub fn buffered(&self, from: u64, to: u64) -> bool {
        from >= self.oldest() && to <= self.head
    }

    #[must_use]
    pub fn aged_out(&self, from: u64) -> bool {
        from < self.oldest()
    }

    #[must_use]
    pub fn get(&self, index: u64) -> f32 {
        if index >= self.head || index < self.oldest() {
            return 0.0;
        }
        self.buf[(index as usize) & (self.buf.len() - 1)]
    }

    /// Linear interpolation between the samples around a fractional index.
    #[must_use]
    pub fn interpolate(&self, position: f64) -> f32 {
        let position = position.max(0.0);
        let index = position.floor();
        let frac = (position - index) as f32;
        let index = index as u64;
        let before = self.get(index);
        let after = self.get(index + 1);
        before + (after - before) * frac
    }

    #[must_use]
    pub fn sum(&self, from: u64, to: u64) -> f64 {
        (from..to).map(|index| f64::from(self.get(index))).sum()
    }

    /// The mean over `from..to`, at least one sample wide.
    #[must_use]
    pub fn mean(&self, from: u64, to: u64) -> f32 {
        let to = to.max(from + 1);
        (self.sum(from, to) / (to - from) as f64) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_between_samples_interpolate() {
        let mut history = SampleHistory::new(8);
        history.push(0.0);
        history.push(2.0);
        assert!((history.interpolate(0.25) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn old_samples_age_out_and_read_as_silence() {
        let mut history = SampleHistory::new(16);
        for index in 0..26 {
            history.push(index as f32);
        }
        assert!(history.aged_out(5));
        assert_eq!(history.get(5), 0.0);
        assert_eq!(history.get(26), 0.0);
        assert!(history.buffered(10, history.head()));
        assert_eq!(history.mean(10, 12), 10.5);
        assert_eq!(history.mean(20, 20), 20.0);
        assert_eq!(history.sum(24, 26), 49.0);
    }
}
