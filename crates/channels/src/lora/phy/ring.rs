use num_complex::Complex;

use super::filter::{HALF_TAPS, Prototype};

pub(crate) struct Ring {
    samples: Vec<Complex<f32>>,
    mask: usize,
    end: u64,
}

impl Ring {
    #[must_use]
    pub(crate) fn new(capacity: usize) -> Self {
        let capacity = capacity.next_power_of_two();
        Self {
            samples: vec![Complex::new(0.0, 0.0); capacity],
            mask: capacity - 1,
            end: 0,
        }
    }

    #[must_use]
    pub(crate) fn end(&self) -> u64 {
        self.end
    }

    #[must_use]
    pub(crate) fn start(&self) -> u64 {
        self.end.saturating_sub(self.samples.len() as u64)
    }

    pub(crate) fn push(&mut self, sample: Complex<f32>) {
        self.samples[self.end as usize & self.mask] = sample;
        self.end += 1;
    }

    #[must_use]
    pub(crate) fn at(&self, index: i64) -> Complex<f32> {
        if index < 0 || (index as u64) < self.start() || index as u64 >= self.end {
            Complex::new(0.0, 0.0)
        } else {
            self.samples[index as usize & self.mask]
        }
    }

    pub(crate) fn copy(&self, start: u64, out: &mut [Complex<f32>]) {
        for (k, sample) in out.iter_mut().enumerate() {
            *sample = self.at((start + k as u64) as i64);
        }
    }
}

pub(crate) struct Front {
    wide: Ring,
    narrow: Ring,
    taps: Vec<f32>,
}

impl Front {
    #[must_use]
    pub(crate) fn new(wide: usize, narrow: usize, prototype: &Prototype) -> Self {
        Self {
            wide: Ring::new(wide),
            narrow: Ring::new(narrow),
            taps: prototype.centred(),
        }
    }

    #[must_use]
    pub(crate) fn wide(&self) -> &Ring {
        &self.wide
    }

    #[must_use]
    pub(crate) fn narrow(&self) -> &Ring {
        &self.narrow
    }

    pub(crate) fn push(&mut self, chunk: &[Complex<f32>]) {
        for &sample in chunk {
            self.wide.push(sample);
        }
        while 2 * self.narrow.end() + HALF_TAPS as u64 <= self.wide.end().saturating_sub(1) {
            let centre = 2 * self.narrow.end() as i64;
            let value = self
                .taps
                .iter()
                .enumerate()
                .fold(Complex::new(0.0, 0.0), |acc, (k, &tap)| {
                    acc + self.wide.at(centre + k as i64 - HALF_TAPS as i64) * tap
                });
            self.narrow.push(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ring_keeps_its_latest_samples_and_zeroes_the_rest() {
        let mut ring = Ring::new(4);
        for k in 0..6 {
            ring.push(Complex::new(k as f32, 0.0));
        }
        assert_eq!(ring.start(), 2);
        assert_eq!(ring.at(1), Complex::new(0.0, 0.0));
        assert_eq!(ring.at(5), Complex::new(5.0, 0.0));
        assert_eq!(ring.at(6), Complex::new(0.0, 0.0));
    }

    #[test]
    fn the_narrow_stream_takes_every_other_sample_of_a_slow_tone() {
        let mut front = Front::new(1024, 512, &Prototype::new());
        let tone: Vec<_> = (0..800)
            .map(|k| Complex::from_polar(1.0, 0.05 * k as f32))
            .collect();
        front.push(&tone);
        let narrow = front.narrow();
        assert!(narrow.end() > 300);
        for j in 50..300 {
            let expected = tone[2 * j];
            assert!((narrow.at(j as i64) - expected).norm() < 0.01, "{j}");
        }
    }
}
