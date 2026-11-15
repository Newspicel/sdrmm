use num_complex::Complex;

type C32 = Complex<f32>;

/// Tap-delay line stored twice over, so the newest `taps` samples always form one slice.
#[derive(Clone, Debug)]
pub(crate) struct Regressor {
    taps: usize,
    history: Vec<C32>,
    head: usize,
}

impl Regressor {
    pub(crate) fn new(taps: usize) -> Self {
        Self {
            taps,
            history: vec![C32::default(); 2 * taps],
            head: 0,
        }
    }

    pub(crate) fn push(&mut self, value: C32) {
        self.head = (self.head + self.taps - 1) % self.taps;
        self.history[self.head] = value;
        self.history[self.head + self.taps] = value;
    }

    pub(crate) fn window(&self) -> &[C32] {
        &self.history[self.head..self.head + self.taps]
    }

    pub(crate) fn clear(&mut self) {
        self.history.fill(C32::default());
        self.head = 0;
    }
}

/// Filter output `w^H x` and the regressor energy `|x|^2`.
pub(crate) fn filter(weights: &[C32], window: &[C32]) -> (C32, f32) {
    weights
        .iter()
        .zip(window)
        .fold((C32::default(), 0.0f32), |(sum, norm), (w, x)| {
            (sum + w.conj() * x, norm + x.norm_sqr())
        })
}
