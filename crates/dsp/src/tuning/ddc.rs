mod fraction;
mod plan;

use fraction::Fraction;
use num_complex::Complex;
use plan::{stage, stage_factors, stage_filter};

use crate::{
    Decimator, Nco,
    filter::resamp::{Prototype, taps_per_phase},
};

/// The flat passband each side of the centre, as a share of the band rate.
const PASSBAND_FRAC: f64 = 0.4;
pub const STOPBAND_DB: f64 = 100.0;
/// The shortest stage filter, in taps.
pub(crate) const MIN_STAGE_TAPS: usize = 11;
/// Input samples mixed and run through the first stage per pass.
const CHUNK: usize = 2048;
/// Outputs the later stages yield per pass, enough to fill whole kernel groups.
const BATCH_OUTPUTS: usize = 128;
/// The cubic interpolator's reach, in samples at its input rate.
const CUBIC_SPAN: f64 = 4.0;
const MIN_FILTERED_UPSAMPLING: f64 = 2.0;
/// A rate ratio this close to one needs no resampler.
const UNITY_TOLERANCE: f64 = 1e-12;
#[cfg(target_arch = "x86_64")]
const CACHE_LINE_BYTES: usize = 64;

#[must_use]
pub fn flat_bandwidth_hz(output_rate: f64) -> f64 {
    2.0 * PASSBAND_FRAC * output_rate
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum DdcError {
    #[error("rates must be positive and finite (input {input} Hz, output {output} Hz)")]
    InvalidRates { input: f64, output: f64 },
}

impl DdcError {
    /// Refuses a pair of rates unless both are positive and finite.
    pub(crate) fn check(input: f64, output: f64) -> Result<(), Self> {
        let valid = |rate: f64| rate.is_finite() && rate > 0.0;
        if valid(input) && valid(output) {
            Ok(())
        } else {
            Err(Self::InvalidRates { input, output })
        }
    }
}

#[derive(Clone, Debug)]
pub struct Ddc {
    input_rate: f64,
    nco: Nco,
    stages: Vec<Decimator>,
    fraction: Fraction,
    mixed: Vec<Complex<f32>>,
    work_in: Vec<Complex<f32>>,
    work_out: Vec<Complex<f32>>,
    staged: Vec<Complex<f32>>,
    batch: usize,
    settling: usize,
}

impl Ddc {
    pub fn new(input_rate: f64, output_rate: f64, offset_hz: f64) -> Result<Self, DdcError> {
        Self::build(input_rate, output_rate, offset_hz, None, output_rate)
    }

    /// A converter for a filter that takes the output down to `band_rate` itself: the passband
    /// and the alias protection cover only that band, so the last stage, which would otherwise
    /// guard the whole output, stays short. Beyond half of `band_rate` the output may carry
    /// aliases, and the filter that follows must stop them.
    pub fn for_band(
        input_rate: f64,
        output_rate: f64,
        band_rate: f64,
        offset_hz: f64,
    ) -> Result<Self, DdcError> {
        if !band_rate.is_finite() || band_rate <= 0.0 || band_rate > output_rate {
            return Err(DdcError::InvalidRates {
                input: input_rate,
                output: band_rate,
            });
        }
        let keep = (band_rate < output_rate).then_some(band_rate / output_rate);
        Self::build(input_rate, output_rate, offset_hz, keep, band_rate)
    }

    pub fn keeping(
        input_rate: f64,
        output_rate: f64,
        offset_hz: f64,
        keep_hz: f64,
    ) -> Result<Self, DdcError> {
        Self::build(
            input_rate,
            output_rate,
            offset_hz,
            Some(keep_hz / output_rate),
            output_rate,
        )
    }

    fn build(
        input_rate: f64,
        output_rate: f64,
        offset_hz: f64,
        keep: Option<f64>,
        band_rate: f64,
    ) -> Result<Self, DdcError> {
        DdcError::check(input_rate, output_rate)?;
        let mut stages = Vec::new();
        let mut rate = input_rate;
        let mut span = 0.0;
        let factors = if output_rate < input_rate {
            stage_factors(input_rate / output_rate)
        } else {
            Vec::new()
        };
        let decimation: usize = factors.iter().product();
        let first_factor = factors.first().copied().unwrap_or(1);
        for &factor in &factors {
            let taps = stage_filter(rate, factor, band_rate).taps();
            span += (taps - 1) as f64 * input_rate / rate;
            stages.push(stage(rate, factor, band_rate));
            rate /= factor as f64;
        }
        let ratio = output_rate / rate;
        span += fraction_span(ratio) * input_rate / rate;
        Ok(Self {
            settling: (span * output_rate / input_rate).ceil() as usize,
            input_rate,
            nco: Nco::new((-offset_hz) as f32, input_rate as f32),
            stages,
            fraction: Fraction::for_ratio(ratio, keep),
            mixed: Vec::new(),
            work_in: Vec::new(),
            work_out: Vec::new(),
            staged: Vec::new(),
            batch: decimation / first_factor * BATCH_OUTPUTS,
        })
    }

    pub fn reset(&mut self) {
        self.nco.reset();
        for stage in &mut self.stages {
            stage.reset();
        }
        self.fraction.reset();
        self.mixed.clear();
        self.work_in.clear();
        self.work_out.clear();
    }

    #[must_use]
    pub fn settling(&self) -> usize {
        self.settling
    }

    pub fn set_offset(&mut self, offset_hz: f64) {
        self.nco
            .set_freq((-offset_hz) as f32, self.input_rate as f32);
    }

    /// Mixes and runs the first stage chunk by chunk, and the later stages as soon as their
    /// input yields a batch of outputs, so each stage reads what the one before it just wrote
    /// while it is still in cache.
    pub fn process(&mut self, input: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        out.clear();
        let Some((first, rest)) = self.stages.split_first_mut() else {
            let mixed = mix(&mut self.nco, input, &mut self.mixed);
            self.fraction.process(mixed, out);
            return;
        };
        let mut tail = Tail {
            stages: rest,
            fraction: &mut self.fraction,
            work_in: &mut self.work_in,
            work_out: &mut self.work_out,
            staged: &mut self.staged,
        };
        tail.work_in.clear();
        for (index, chunk) in input.chunks(CHUNK).enumerate() {
            prefetch(input.get((index + 1) * CHUNK..).unwrap_or_default());
            let mixed = mix(&mut self.nco, chunk, &mut self.mixed);
            first.process(mixed, tail.work_out);
            tail.work_in.extend_from_slice(tail.work_out);
            if tail.work_in.len() >= self.batch {
                tail.flush(out);
            }
        }
        if !tail.work_in.is_empty() {
            tail.flush(out);
        }
    }
}

/// The stages after the first, with the buffers they pass samples through.
struct Tail<'a> {
    stages: &'a mut [Decimator],
    fraction: &'a mut Fraction,
    work_in: &'a mut Vec<Complex<f32>>,
    work_out: &'a mut Vec<Complex<f32>>,
    staged: &'a mut Vec<Complex<f32>>,
}

impl Tail<'_> {
    fn flush(&mut self, out: &mut Vec<Complex<f32>>) {
        for stage in self.stages.iter_mut() {
            stage.process(self.work_in, self.work_out);
            std::mem::swap(self.work_in, self.work_out);
        }
        self.fraction.process(self.work_in, self.staged);
        out.extend_from_slice(self.staged);
        self.work_in.clear();
    }
}

fn is_unity(ratio: f64) -> bool {
    (ratio - 1.0).abs() <= UNITY_TOLERANCE
}

/// The samples, at its input rate, the fractional stage for `ratio` takes to settle.
fn fraction_span(ratio: f64) -> f64 {
    if (1.0 + UNITY_TOLERANCE..MIN_FILTERED_UPSAMPLING).contains(&ratio) {
        CUBIC_SPAN
    } else if !is_unity(ratio) {
        taps_per_phase(ratio, Prototype::Kaiser(STOPBAND_DB)) as f64
    } else {
        0.0
    }
}

/// Asks for the next chunk while this one runs through the stages, so mixing it does not wait on
/// memory: the stages never touch the input, so the hardware stream would stall in between.
fn prefetch(next: &[Complex<f32>]) {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::{_MM_HINT_T1, _mm_prefetch};
        const LINE_SAMPLES: usize = CACHE_LINE_BYTES / std::mem::size_of::<Complex<f32>>();
        let next = &next[..next.len().min(CHUNK)];
        for line in next.chunks(LINE_SAMPLES) {
            unsafe { _mm_prefetch::<_MM_HINT_T1>(line.as_ptr().cast()) };
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    let _ = next;
}

fn mix<'a>(
    nco: &mut Nco,
    input: &'a [Complex<f32>],
    mixed: &'a mut Vec<Complex<f32>>,
) -> &'a [Complex<f32>] {
    if nco.is_identity() {
        return input;
    }
    mixed.resize(input.len(), Complex::new(0.0, 0.0));
    nco.mix_into(input, mixed);
    mixed
}

#[cfg(test)]
pub(crate) mod tests;
