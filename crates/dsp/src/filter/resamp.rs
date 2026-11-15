mod clock;
mod prototype;

use clock::{Clock, FREE_PHASES};
use num_complex::Complex;
pub(crate) use prototype::{Prototype, taps_per_phase};

use crate::filter::fir::{DelayLine, Isa, plane_dot, plane_interpolated, reversed_branch};

/// Branch lengths round up to this, a whole number of kernel lanes.
const TAP_MULTIPLE: usize = 8;
/// Outputs of one exact branch computed side by side.
const BATCH: usize = 4;
/// The cutoff of a resampler that keeps the whole band, as a share of the lower rate.
const WHOLE_BAND_CUTOFF: f64 = 0.45;

#[derive(Clone, Debug)]
pub struct FracResampler {
    rows: Vec<f32>,
    slopes: Vec<f32>,
    taps_per_phase: usize,
    clock: Clock,
    position: usize,
    re: DelayLine<f32>,
    im: DelayLine<f32>,
    isa: Isa,
}

/// A run of outputs that share one exact branch, and the slots they fill.
struct Sequence<I> {
    ticks: usize,
    stride: usize,
    outputs: usize,
    slots: I,
}

impl FracResampler {
    #[must_use]
    pub fn new(ratio: f64) -> Self {
        Self::shaped(ratio, None, Prototype::Blackman)
    }

    #[must_use]
    pub fn keeping(ratio: f64, keep: f64) -> Self {
        Self::shaped(ratio, Some(keep), Prototype::Blackman)
    }

    /// [`Self::new`] or, given a band to keep, [`Self::keeping`], cut from a Kaiser prototype that
    /// holds `attenuation_db` beyond half the output rate.
    #[must_use]
    pub fn rejecting(ratio: f64, keep: Option<f64>, attenuation_db: f64) -> Self {
        Self::shaped(ratio, keep, Prototype::Kaiser(attenuation_db))
    }

    fn shaped(ratio: f64, keep: Option<f64>, prototype: Prototype) -> Self {
        assert!(ratio.is_finite() && ratio > 0.0, "ratio must be positive");
        let full = taps_per_phase(ratio, prototype);
        let whole = || Self::design(ratio, WHOLE_BAND_CUTOFF * ratio.min(1.0), full, prototype);
        let Some(keep) = keep.filter(|&keep| ratio < 1.0 && keep > 0.0 && keep < 1.0) else {
            return whole();
        };
        let taps = prototype.taps(ratio * (1.0 - keep));
        if taps >= full {
            return whole();
        }
        Self::design(ratio, 0.5 * ratio, taps, prototype)
    }

    fn design(ratio: f64, cutoff: f64, taps: usize, prototype: Prototype) -> Self {
        let taps_per_phase = taps.next_multiple_of(TAP_MULTIPLE);
        let clock = Clock::for_ratio(ratio, taps_per_phase);
        let phases = clock.phases();
        let proto = prototype.lowpass(phases * taps + 1, cutoff / phases as f64);
        let mut rows = vec![0.0f32; (phases + 1) * taps_per_phase];
        for (phase, row) in rows.chunks_exact_mut(taps_per_phase).enumerate() {
            reversed_branch(&proto, phases, phase, &mut row[taps_per_phase - taps..]);
        }
        let slopes = match clock {
            Clock::Exact { .. } => Vec::new(),
            Clock::Free { .. } => rows[taps_per_phase..]
                .iter()
                .zip(&rows)
                .map(|(upper, lower)| upper - lower)
                .collect(),
        };
        Self {
            rows,
            slopes,
            taps_per_phase,
            clock,
            position: taps_per_phase - 1,
            re: DelayLine::new(taps_per_phase - 1, 1, 0),
            im: DelayLine::new(taps_per_phase - 1, 1, 0),
            isa: Isa::detect(),
        }
    }

    pub fn reset(&mut self) {
        self.position = self.taps_per_phase - 1;
        self.clock.reset();
        self.re.reset();
        self.im.reset();
    }

    pub fn process(&mut self, input: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        out.clear();
        for chunk in input.chunks(self.re.room()) {
            self.re.push_with(chunk, |sample| sample.re);
            self.im.push_with(chunk, |sample| sample.im);
            match self.clock {
                Clock::Exact { .. } => self.emit_exact(out),
                Clock::Free { .. } => self.emit_free(out),
            }
            let tpp = self.taps_per_phase;
            let consumed = self.position.saturating_sub(tpp - 1).min(self.re.len());
            self.re.consume(consumed);
            self.im.consume(consumed);
            self.position -= consumed;
        }
    }

    fn emit_exact(&mut self, out: &mut Vec<Complex<f32>>) {
        let Clock::Exact {
            phases,
            whole,
            rest,
            phase,
        } = self.clock
        else {
            return;
        };
        let advance = whole * phases + rest;
        let ticks = self.position * phases + phase;
        let count = (self.re.len() * phases)
            .saturating_sub(ticks)
            .div_ceil(advance);
        let first = out.len();
        out.resize(first + count, Complex::new(0.0, 0.0));
        for lead in 0..phases.min(count) {
            let mut sequence = Sequence {
                ticks: ticks + lead * advance,
                stride: advance,
                outputs: (count - lead).div_ceil(phases),
                slots: out[first + lead..].iter_mut().step_by(phases),
            };
            self.fill::<BATCH>(&mut sequence, phases);
            self.fill::<1>(&mut sequence, phases);
        }
        let ticks = ticks + count * advance;
        self.position = ticks / phases;
        self.clock = Clock::Exact {
            phases,
            whole,
            rest,
            phase: ticks % phases,
        };
    }

    fn fill<'a, const N: usize>(
        &self,
        sequence: &mut Sequence<impl Iterator<Item = &'a mut Complex<f32>>>,
        phases: usize,
    ) {
        let tpp = self.taps_per_phase;
        let row = sequence.ticks % phases;
        let taps = &self.rows[row * tpp..(row + 1) * tpp];
        let (re, im) = (self.re.rows_from(0), self.im.rows_from(0));
        while sequence.outputs >= N {
            let window = sequence.ticks / phases + 1 - tpp;
            let starts: [usize; N] = std::array::from_fn(|index| window + index * sequence.stride);
            let values = plane_dot(self.isa, re, im, starts, taps);
            for (value, slot) in values.into_iter().zip(sequence.slots.by_ref()) {
                *slot = value;
            }
            sequence.ticks += N * sequence.stride * phases;
            sequence.outputs -= N;
        }
    }

    fn emit_free(&mut self, out: &mut Vec<Complex<f32>>) {
        let Clock::Free {
            step,
            ref mut fraction,
        } = self.clock
        else {
            return;
        };
        let tpp = self.taps_per_phase;
        while self.position < self.re.len() {
            let phase = *fraction * FREE_PHASES as f64;
            let row = phase as usize;
            let mu = (phase - row as f64) as f32;
            let taps = row * tpp..(row + 1) * tpp;
            let window = self.position + 1 - tpp;
            out.push(plane_interpolated(
                self.isa,
                &self.re.rows_from(window)[..tpp],
                &self.im.rows_from(window)[..tpp],
                &self.rows[taps.clone()],
                &self.slopes[taps],
                mu,
            ));
            *fraction += step;
            let advance = *fraction as usize;
            self.position += advance;
            *fraction -= advance as f64;
        }
    }
}

#[cfg(test)]
mod tests;
