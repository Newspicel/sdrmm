use std::ops::{Add, Sub};

const LANES: usize = 4;
const RESTART: usize = 4_096;

/// Sums of every `width` consecutive inputs, `out[n] = input[n] + … + input[n + width - 1]`,
/// at a few operations per output whatever the width.
///
/// Each sum comes from the one `LANES` outputs earlier plus the four inputs entering minus the
/// four leaving, so the recurrence vectorises. An exact restart every `RESTART` outputs bounds
/// the rounding a long recurrence would gather.
#[derive(Clone, Debug)]
pub struct MovingSum<T> {
    width: usize,
    quads: Vec<T>,
}

impl<T> MovingSum<T>
where
    T: Copy + Default + Add<Output = T> + Sub<Output = T>,
{
    #[must_use]
    pub fn new(width: usize) -> Self {
        assert!(width > 0, "a moving sum spans at least one input");
        Self {
            width,
            quads: Vec::new(),
        }
    }

    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Replaces `out` with the `input.len() + 1 - width` window sums, or nothing for a shorter input.
    pub fn process(&mut self, input: &[T], out: &mut Vec<T>) {
        out.clear();
        let Some(count) = (input.len() + 1).checked_sub(self.width) else {
            return;
        };
        self.quads.clear();
        let quads = input.len().saturating_sub(LANES - 1);
        self.quads.extend(
            input[..quads]
                .iter()
                .zip(&input[1..])
                .zip(&input[2..])
                .zip(&input[3..])
                .map(|(((&a, &b), &c), &d)| a + b + c + d),
        );
        for start in (0..count).step_by(RESTART) {
            let end = (start + RESTART).min(count);
            self.block(input, start, end - start, out);
        }
    }

    fn block(&self, input: &[T], start: usize, len: usize, out: &mut Vec<T>) {
        let seed = len.min(LANES);
        let first = out.len();
        out.extend((start..start + seed).map(|at| {
            input[at..at + self.width]
                .iter()
                .fold(T::default(), |sum, &value| sum + value)
        }));
        if len <= LANES {
            return;
        }
        let rest = len - LANES;
        let leaving = &self.quads[start..start + rest];
        let entering = &self.quads[start + self.width..start + self.width + rest];
        out.extend(
            entering
                .iter()
                .zip(leaving)
                .map(|(&enter, &leave)| enter - leave),
        );
        let (chunks, tail) = out[first..].as_chunks_mut::<LANES>();
        let mut previous = chunks[0];
        for chunk in &mut chunks[1..] {
            for (value, &earlier) in chunk.iter_mut().zip(&previous) {
                *value = earlier + *value;
            }
            previous = *chunk;
        }
        for (value, &earlier) in tail.iter_mut().zip(&previous) {
            *value = earlier + *value;
        }
    }
}

#[cfg(test)]
mod tests {
    use num_complex::Complex;

    use super::*;

    fn direct(input: &[f64], width: usize) -> Vec<f64> {
        input.windows(width).map(|w| w.iter().sum()).collect()
    }

    #[test]
    fn every_window_sum_matches_the_direct_sum() {
        let input: Vec<f32> = (0..10_000)
            .map(|k| ((k * 37) % 101) as f32 / 101.0 - 0.4)
            .collect();
        let wide: Vec<f64> = input.iter().map(|&v| f64::from(v)).collect();
        for width in [1, 2, 3, 4, 5, 20, 48, 64, 5_000] {
            let mut sums = MovingSum::new(width);
            let mut out = Vec::new();
            sums.process(&input, &mut out);
            let want = direct(&wide, width);
            assert_eq!(out.len(), want.len(), "width {width}");
            for (n, (&got, &want)) in out.iter().zip(&want).enumerate() {
                let tolerance = 1e-5 * width as f64 + 1e-4;
                assert!(
                    (f64::from(got) - want).abs() < tolerance,
                    "width {width} at {n}: {got} vs {want}"
                );
            }
        }
    }

    #[test]
    fn complex_sums_and_short_inputs() {
        let input: Vec<Complex<f32>> = (0..300)
            .map(|k| Complex::new(k as f32, -(k as f32) * 0.5))
            .collect();
        let mut sums = MovingSum::new(48);
        let mut out = Vec::new();
        sums.process(&input, &mut out);
        assert_eq!(out.len(), 253);
        for (n, got) in out.iter().enumerate() {
            let want: Complex<f32> = input[n..n + 48].iter().sum();
            assert!((got - want).norm() < 1e-2, "{n}: {got} vs {want}");
        }
        sums.process(&input[..47], &mut out);
        assert!(out.is_empty());
        sums.process(&input[..48], &mut out);
        assert_eq!(out.len(), 1);
    }
}
