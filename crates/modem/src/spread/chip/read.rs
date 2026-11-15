use num_complex::Complex;
use sdrmm_dsp::farrow;

#[must_use]
pub fn sample_at(filtered: &[Complex<f32>], position: f64) -> Complex<f32> {
    let zero = Complex::new(0.0, 0.0);
    if !position.is_finite() || position < 0.0 {
        return zero;
    }
    let base = position.floor() as usize;
    if base >= 1 && base + 2 < filtered.len() {
        return farrow(
            &filtered[base - 1..base + 3],
            (position - base as f64) as f32,
        );
    }
    filtered
        .get(position.round() as usize)
        .copied()
        .unwrap_or(zero)
}

/// Integrates samples, each held over `[k, k + 1)`, across `[from, to)`: the matched filter of
/// a rectangular chip whose length is not a whole number of samples. Outside the samples it
/// reads zero.
#[must_use]
pub fn integrate(samples: &[Complex<f32>], from: f64, to: f64) -> Complex<f32> {
    if !(from.is_finite() && to.is_finite()) || to <= from || to <= 0.0 {
        return Complex::new(0.0, 0.0);
    }
    let first = from.max(0.0) as usize;
    let whole = to as usize;
    let last = whole
        .saturating_add(usize::from((whole as f64) < to))
        .min(samples.len());
    let Some(held) = samples.get(first..last) else {
        return Complex::new(0.0, 0.0);
    };
    let mut sum = Complex::new(0.0, 0.0);
    for (k, &sample) in (first..).zip(held) {
        let overlap = to.min(k as f64 + 1.0) - from.max(k as f64);
        sum += sample * overlap as f32;
    }
    sum
}

/// Integrates `N` back-to-back chips of `length` samples from `start`, each as [`integrate`]
/// does, from one running sum: every sample is added once and every chip edge costs one
/// multiply-add.
#[must_use]
pub fn integrate_chips<const N: usize>(
    samples: &[Complex<f32>],
    start: f64,
    length: f64,
) -> [Complex<f32>; N] {
    let mut running = Running::new(samples, start);
    let mut previous = running.to(start);
    std::array::from_fn(|chip| {
        let edge = running.to(start + (chip + 1) as f64 * length);
        let value = edge - previous;
        previous = edge;
        value
    })
}

/// The integral of held samples from a fixed origin up to edges that never move back.
struct Running<'a> {
    samples: &'a [Complex<f32>],
    origin: f64,
    next: usize,
    sum: Complex<f32>,
}

impl<'a> Running<'a> {
    fn new(samples: &'a [Complex<f32>], start: f64) -> Self {
        let next = if start.is_finite() {
            (start.max(0.0) as usize).min(samples.len())
        } else {
            samples.len()
        };
        Self {
            samples,
            origin: next as f64,
            next,
            sum: Complex::new(0.0, 0.0),
        }
    }

    fn to(&mut self, edge: f64) -> Complex<f32> {
        if !(edge > self.origin && edge.is_finite()) {
            return Complex::new(0.0, 0.0);
        }
        let whole = edge as usize;
        let end = whole.min(self.samples.len());
        if self.next < end {
            self.sum += self.samples[self.next..end].iter().sum::<Complex<f32>>();
            self.next = end;
        }
        let part = (edge - whole as f64) as f32;
        self.sum
            + self
                .samples
                .get(whole)
                .map_or(Complex::new(0.0, 0.0), |&x| x * part)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integration_weighs_each_sample_by_its_overlap() {
        let samples = [1.0f32, 2.0, 4.0, 8.0].map(|v| Complex::new(v, 0.0));
        let at = |from: f64, to: f64| integrate(&samples, from, to).re;
        assert!((at(0.0, 4.0) - 15.0).abs() < 1e-6);
        assert!((at(0.5, 2.25) - (0.5 + 2.0 + 1.0)).abs() < 1e-6);
        assert!((at(1.2, 1.7) - 1.0).abs() < 1e-6);
        assert!((at(-1.0, 0.5) - 0.5).abs() < 1e-6);
        assert!((at(3.5, 9.0) - 4.0).abs() < 1e-6);
        assert_eq!(at(2.0, 2.0), 0.0);
    }

    #[test]
    fn back_to_back_chips_match_one_integration_each() {
        let samples: Vec<Complex<f32>> = (0..40)
            .map(|k| Complex::new((k as f32 * 0.7).sin(), (k as f32 * 0.3).cos()))
            .collect();
        let length = 20.0 / 11.0;
        for start in [-3.1, -0.4, 0.0, 0.25, 3.0, 7.6, 26.3, 33.9, 45.0] {
            let chips = integrate_chips::<8>(&samples, start, length);
            for (chip, &got) in chips.iter().enumerate() {
                let from = start + chip as f64 * length;
                let want = integrate(&samples, from, from + length);
                assert!(
                    (got - want).norm() < 1e-5,
                    "{start} {chip}: {got} vs {want}"
                );
            }
        }
    }
}
