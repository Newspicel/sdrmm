use num_complex::Complex;

type C64 = Complex<f64>;

/// Rendered samples from index `base` on, read as zero outside what is held.
#[derive(Clone, Debug)]
pub(super) struct History {
    pub(super) base: i64,
    pub(super) data: Vec<C64>,
}

impl History {
    pub(super) const fn starting_at(base: i64) -> Self {
        Self {
            base,
            data: Vec::new(),
        }
    }

    pub(super) fn end(&self) -> i64 {
        self.base + self.data.len() as i64
    }

    pub(super) fn get(&self, index: i64) -> C64 {
        usize::try_from(index - self.base)
            .ok()
            .and_then(|offset| self.data.get(offset))
            .copied()
            .unwrap_or_default()
    }

    /// Sample `at` through a fractional-delay filter whose zero delay sits `centre` taps in.
    pub(super) fn filtered(&self, at: i64, taps: &[f64], centre: i64) -> C64 {
        taps.iter()
            .enumerate()
            .map(|(k, &tap)| self.get(at - k as i64 + centre) * tap)
            .sum()
    }

    pub(super) fn trim(&mut self, keep_from: i64) {
        let drop = (keep_from - self.base).clamp(0, self.data.len() as i64) as usize;
        if drop > 0 {
            self.data.drain(..drop);
            self.base += drop as i64;
        }
    }
}
