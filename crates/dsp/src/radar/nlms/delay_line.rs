use num_complex::Complex;

type C32 = Complex<f32>;

#[derive(Clone, Debug)]
pub(super) struct DelayLine {
    samples: Vec<C32>,
    head: usize,
}

impl DelayLine {
    pub(super) fn new(len: usize) -> Self {
        Self {
            samples: vec![C32::default(); len],
            head: 0,
        }
    }

    pub(super) fn push(&mut self, value: C32) -> C32 {
        if self.samples.is_empty() {
            return value;
        }
        let out = std::mem::replace(&mut self.samples[self.head], value);
        self.head = (self.head + 1) % self.samples.len();
        out
    }

    pub(super) fn clear(&mut self) {
        self.samples.fill(C32::default());
        self.head = 0;
    }
}
