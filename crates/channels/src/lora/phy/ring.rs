use num_complex::Complex;
use sdrmm_dsp::{Decimator, design_halfband};

const ZERO: Complex<f32> = Complex::new(0.0, 0.0);
const HALFBAND_TAPS: usize = 47;
const HALFBAND_ATTENUATION_DB: f64 = 60.0;
const HALFBAND_REACH: usize = HALFBAND_TAPS / 2;

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
            samples: vec![ZERO; capacity],
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

    pub(crate) fn extend(&mut self, chunk: &[Complex<f32>]) {
        let keep = chunk.len().min(self.samples.len());
        let skipped = chunk.len() - keep;
        let at = (self.end as usize + skipped) & self.mask;
        let (first, second) = chunk[skipped..].split_at(keep.min(self.samples.len() - at));
        self.samples[at..at + first.len()].copy_from_slice(first);
        self.samples[..second.len()].copy_from_slice(second);
        self.end += chunk.len() as u64;
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn at(&self, index: i64) -> Complex<f32> {
        let mut sample = [ZERO];
        self.copy(index, &mut sample);
        sample[0]
    }

    /// Copies the samples from `start` on, with zeros where the ring holds none.
    pub(crate) fn copy(&self, start: i64, out: &mut [Complex<f32>]) {
        let end = start.saturating_add(out.len() as i64);
        let from = start.max(self.start() as i64);
        let to = end.min(self.end as i64);
        if from >= to {
            out.fill(ZERO);
            return;
        }
        let (head, rest) = out.split_at_mut((from - start) as usize);
        let (body, tail) = rest.split_at_mut((to - from) as usize);
        head.fill(ZERO);
        tail.fill(ZERO);
        let at = from as usize & self.mask;
        let (first, second) = body.split_at_mut(body.len().min(self.samples.len() - at));
        first.copy_from_slice(&self.samples[at..at + first.len()]);
        second.copy_from_slice(&self.samples[..second.len()]);
    }
}

/// The wide stream at two samples per chip and a half-band copy at one sample per chip, aligned
/// so that narrow sample `j` is centred on wide sample `2j`.
pub(crate) struct Front {
    wide: Ring,
    narrow: Ring,
    halfband: Decimator,
    decimated: Vec<Complex<f32>>,
    unborn: usize,
}

impl Front {
    /// The half-band's centre trails its newest input by an odd count, so one leading zero puts
    /// every output on an even wide sample; the outputs centred before wide sample 0 are dropped.
    #[must_use]
    pub(crate) fn new(wide: usize, narrow: usize) -> Self {
        let mut halfband =
            Decimator::new(&design_halfband(HALFBAND_TAPS, HALFBAND_ATTENUATION_DB), 2);
        let mut decimated = Vec::new();
        halfband.process(&[ZERO], &mut decimated);
        Self {
            wide: Ring::new(wide),
            narrow: Ring::new(narrow),
            halfband,
            unborn: HALFBAND_REACH.div_ceil(2) - decimated.len(),
            decimated,
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
        self.wide.extend(chunk);
        self.halfband.process(chunk, &mut self.decimated);
        let skip = self.unborn.min(self.decimated.len());
        self.unborn -= skip;
        self.narrow.extend(&self.decimated[skip..]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ring_keeps_its_latest_samples_and_zeroes_the_rest() {
        let mut ring = Ring::new(4);
        for k in 0..6 {
            ring.extend(&[Complex::new(k as f32, 0.0)]);
        }
        assert_eq!(ring.start(), 2);
        assert_eq!(ring.at(1), Complex::new(0.0, 0.0));
        assert_eq!(ring.at(5), Complex::new(5.0, 0.0));
        assert_eq!(ring.at(6), Complex::new(0.0, 0.0));
    }

    #[test]
    fn a_copy_across_the_wrap_matches_sample_by_sample_reads() {
        let mut ring = Ring::new(8);
        let data: Vec<_> = (0..13).map(|k| Complex::new(k as f32, -k as f32)).collect();
        ring.extend(&data[..3]);
        ring.extend(&data[3..]);
        let mut out = [ZERO; 12];
        ring.copy(2, &mut out);
        for (k, sample) in out.iter().enumerate() {
            let index = 2 + k;
            let expected = if (5..13).contains(&index) {
                data[index]
            } else {
                ZERO
            };
            assert_eq!(*sample, expected, "{index}");
        }
        ring.extend(&data);
        assert_eq!(ring.at(25), data[12]);
        assert_eq!(ring.at(17), Complex::new(0.0, 0.0));
    }

    #[test]
    fn the_narrow_stream_takes_every_other_sample_of_a_slow_tone() {
        let mut front = Front::new(1024, 512);
        let tone: Vec<_> = (0..800)
            .map(|k| Complex::from_polar(1.0, 0.05 * k as f32))
            .collect();
        for piece in tone.chunks(37) {
            front.push(piece);
        }
        let narrow = front.narrow();
        assert_eq!(narrow.end(), (800 - 1 - HALFBAND_REACH as u64) / 2 + 1);
        for j in 50..300 {
            let expected = tone[2 * j];
            assert!((narrow.at(j as i64) - expected).norm() < 0.01, "{j}");
        }
    }
}
