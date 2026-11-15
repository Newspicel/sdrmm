#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preamble {
    pub short_stride: usize,
    pub short_repeats: usize,
    pub long_repeats: usize,
    pub long_guard: usize,
}

impl Preamble {
    #[must_use]
    pub fn short_period(&self, fft: usize) -> usize {
        fft / self.short_stride.max(1)
    }

    #[must_use]
    pub fn short_samples(&self, fft: usize) -> usize {
        self.short_repeats * self.short_period(fft)
    }

    #[must_use]
    pub fn samples(&self, fft: usize) -> usize {
        self.short_samples(fft) + self.long_guard + self.long_repeats * fft
    }
}
