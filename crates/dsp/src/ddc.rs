use num_complex::Complex;

use crate::{
    CubicInterpolator, Decimator, FracResampler, Nco,
    fir::{design_halfband, design_lowpass_kaiser, halfband_taps, kaiser_taps},
    resamp::{Prototype, taps_per_phase},
};

const PASSBAND_FRAC: f64 = 0.4;
const PROTECT_FRAC: f64 = 0.5;
pub const STOPBAND_DB: f64 = 100.0;
const CHUNK: usize = 2048;
/// Outputs the later stages yield per pass, enough to fill whole kernel groups.
const BATCH_OUTPUTS: usize = 128;
const CUBIC_SPAN: f64 = 4.0;
const MIN_FILTERED_UPSAMPLING: f64 = 2.0;
const MIN_STAGE_TAPS: usize = 11;

#[must_use]
pub fn flat_bandwidth_hz(output_rate: f64) -> f64 {
    2.0 * PASSBAND_FRAC * output_rate
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum DdcError {
    #[error("rates must be positive and finite (input {input} Hz, output {output} Hz)")]
    InvalidRates { input: f64, output: f64 },
}

#[derive(Clone, Debug)]
enum Fraction {
    None,
    Resample(Box<FracResampler>),
    Interpolate(CubicInterpolator),
}

impl Fraction {
    fn for_ratio(ratio: f64, keep: Option<f64>) -> Self {
        if (ratio - 1.0).abs() <= 1e-12 {
            Self::None
        } else if (1.0..MIN_FILTERED_UPSAMPLING).contains(&ratio) {
            Self::Interpolate(CubicInterpolator::new(ratio))
        } else {
            Self::Resample(Box::new(FracResampler::rejecting(ratio, keep, STOPBAND_DB)))
        }
    }

    fn reset(&mut self) {
        match self {
            Self::None => {}
            Self::Resample(r) => r.reset(),
            Self::Interpolate(r) => r.reset(),
        }
    }

    fn process(&mut self, input: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        match self {
            Self::None => {
                out.clear();
                out.extend_from_slice(input);
            }
            Self::Resample(r) => r.process(input, out),
            Self::Interpolate(r) => r.process(input, out),
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
        Self::build(input_rate, output_rate, offset_hz, None)
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
        )
    }

    fn build(
        input_rate: f64,
        output_rate: f64,
        offset_hz: f64,
        keep: Option<f64>,
    ) -> Result<Self, DdcError> {
        if !input_rate.is_finite()
            || !output_rate.is_finite()
            || input_rate <= 0.0
            || output_rate <= 0.0
        {
            return Err(DdcError::InvalidRates {
                input: input_rate,
                output: output_rate,
            });
        }

        let mut stages = Vec::new();
        let mut rate = input_rate;
        let mut span = 0.0;
        let factors = if output_rate < input_rate {
            prime_factors_desc(integer_decimation(input_rate / output_rate))
        } else {
            Vec::new()
        };
        let decimation: usize = factors.iter().product();
        let first_factor = factors.first().copied().unwrap_or(1);
        for &factor in &factors {
            let taps = stage_filter(rate, factor, output_rate).taps();
            span += (taps - 1) as f64 * input_rate / rate;
            stages.push(stage(rate, factor, output_rate));
            rate /= factor as f64;
        }
        let ratio = output_rate / rate;
        if (1.0 + 1e-12..MIN_FILTERED_UPSAMPLING).contains(&ratio) {
            span += CUBIC_SPAN * input_rate / rate;
        } else if (ratio - 1.0).abs() > 1e-12 {
            span +=
                taps_per_phase(ratio, Prototype::Kaiser(STOPBAND_DB)) as f64 * input_rate / rate;
        }
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

/// Asks for the next chunk while this one runs through the stages, so mixing it does not wait on
/// memory: the stages never touch the input, so the hardware stream would stall in between.
fn prefetch(next: &[Complex<f32>]) {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::{_MM_HINT_T1, _mm_prefetch};
        const LINE_SAMPLES: usize = 64 / std::mem::size_of::<Complex<f32>>();
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

fn integer_decimation(quotient: f64) -> usize {
    let rounded = quotient.round();
    let mut candidate = if (quotient - rounded).abs() < 1e-9 {
        rounded as usize
    } else {
        quotient.floor() as usize
    };
    let minimum = (candidate / 2).max(candidate.saturating_sub(256)).max(1);
    let mut best = 1usize << candidate.max(1).ilog2();
    let mut best_cost = decimation_cost(quotient, best);
    while candidate >= minimum {
        let mut remaining = candidate;
        for factor in [2, 3, 5, 7, 11, 13] {
            while remaining.is_multiple_of(factor) {
                remaining /= factor;
            }
        }
        if remaining == 1 {
            let cost = decimation_cost(quotient, candidate);
            if cost < best_cost {
                best = candidate;
                best_cost = cost;
            }
        }
        candidate -= 1;
    }
    best
}

fn decimation_cost(quotient: f64, decimation: usize) -> f64 {
    let mut rate = quotient;
    let mut cost = 0.0;
    for factor in prime_factors_desc(decimation) {
        let taps = lowpass_stage(rate, factor, 1.0).taps();
        rate /= factor as f64;
        cost += taps as f64 * rate / quotient;
    }
    if (rate - 1.0).abs() > 1e-12 {
        cost +=
            2.0 * taps_per_phase(rate.recip(), Prototype::Kaiser(STOPBAND_DB)) as f64 / quotient;
    }
    cost
}

fn prime_factors_desc(mut n: usize) -> Vec<usize> {
    let mut factors = Vec::new();
    let mut d = 2;
    while d * d <= n {
        while n.is_multiple_of(d) {
            factors.push(d);
            n /= d;
        }
        d += 1;
    }
    if n > 1 {
        factors.push(n);
    }
    factors.reverse();
    factors
}

fn stage(input_rate: f64, factor: usize, output_rate: f64) -> Decimator {
    Decimator::new(
        &stage_filter(input_rate, factor, output_rate).design(),
        factor,
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum StageFilter {
    Lowpass { taps: usize, cutoff: f64 },
    Halfband { taps: usize },
}

impl StageFilter {
    fn taps(self) -> usize {
        match self {
            Self::Lowpass { taps, .. } | Self::Halfband { taps } => taps,
        }
    }

    /// The nonzero taps, which is what one output costs.
    fn products(self) -> usize {
        match self {
            Self::Lowpass { taps, .. } => taps,
            Self::Halfband { taps } => taps / 2 + 2,
        }
    }

    fn design(self) -> Vec<f32> {
        match self {
            Self::Lowpass { taps, cutoff } => design_lowpass_kaiser(taps, cutoff, STOPBAND_DB),
            Self::Halfband { taps } => design_halfband(taps, STOPBAND_DB),
        }
    }
}

/// The cheaper of the stage's low-pass and, for a stage by two, a half-band that meets the same
/// passband and protection edges at the same attenuation. Planning prices every stage as its
/// low-pass, so a cheaper stage by two never changes the plan.
fn stage_filter(input_rate: f64, factor: usize, output_rate: f64) -> StageFilter {
    let lowpass = lowpass_stage(input_rate, factor, output_rate);
    let (pass, stop) = stage_edges(input_rate, factor, output_rate);
    let transition = 0.5 - 2.0 * pass.max(0.5 - stop);
    if factor != 2 || transition <= 0.0 {
        return lowpass;
    }
    let halfband = StageFilter::Halfband {
        taps: halfband_taps(transition, STOPBAND_DB).max(MIN_STAGE_TAPS),
    };
    if halfband.products() < lowpass.products() {
        halfband
    } else {
        lowpass
    }
}

fn lowpass_stage(input_rate: f64, factor: usize, output_rate: f64) -> StageFilter {
    let (pass, stop) = stage_edges(input_rate, factor, output_rate);
    StageFilter::Lowpass {
        taps: kaiser_taps(stop - pass, STOPBAND_DB).max(MIN_STAGE_TAPS),
        cutoff: (pass + stop) / 2.0,
    }
}

/// The edge the passband must reach and the one beyond which nothing may fold into the
/// protected band, as fractions of the stage's input rate.
fn stage_edges(input_rate: f64, factor: usize, output_rate: f64) -> (f64, f64) {
    let pass = PASSBAND_FRAC * output_rate / input_rate;
    let stop = (input_rate / factor as f64 - PROTECT_FRAC * output_rate) / input_rate;
    (pass, stop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::rms_c;

    const FS_IN: f64 = 2_048_000.0;
    const FS_OUT: f64 = 48_000.0;
    const BLOCK: usize = 16_384;

    fn tone_at_rate(freq_hz: f64, rate: f64, len: usize) -> Vec<Complex<f32>> {
        let mut nco = Nco::new(freq_hz as f32, rate as f32);
        (0..len).map(|_| nco.next_sample()).collect()
    }

    #[test]
    fn the_start_up_transient_ends_within_the_reported_settling() {
        for (input_rate, output_rate) in [
            (2_400_000.0, 48_000.0),
            (1_024_000.0, 37_000.0),
            (48_000.0, 48_000.0),
        ] {
            let mut ddc = Ddc::new(input_rate, output_rate, 0.0).unwrap();
            let mut out = Vec::new();
            ddc.process(&vec![Complex::new(1.0, 0.0); 200_000], &mut out);
            let settled = &out[ddc.settling()..];
            let worst = settled
                .iter()
                .map(|s| (s.re - 1.0).abs())
                .fold(0.0f32, f32::max);
            assert!(
                worst < 1e-3,
                "{input_rate} to {output_rate}: {worst} after {}",
                ddc.settling()
            );
        }
    }

    #[test]
    fn neighbours_outside_the_channel_stay_a_hundred_db_down() {
        for (input_rate, output_rate) in [(2_400_000.0, 48_000.0), (10_000_000.0, 48_000.0)] {
            for offset in [0.6, 0.75, 1.0, 2.0, 7.0, 21.0, 49.0] {
                for sign in [1.0, -1.0] {
                    let tone = tone_at_rate(sign * offset * output_rate, input_rate, 120_000);
                    let mut ddc = Ddc::new(input_rate, output_rate, 0.0).unwrap();
                    let mut out = Vec::new();
                    ddc.process(&tone, &mut out);
                    let leak = 20.0 * rms_c(&out[out.len() / 2..]).log10();
                    assert!(
                        leak < -96.0,
                        "{input_rate} to {output_rate}: {} Hz leaks at {leak} dB",
                        sign * offset * output_rate
                    );
                }
            }
        }
    }

    fn tone(freq_hz: f64, len: usize) -> Vec<Complex<f32>> {
        tone_at_rate(freq_hz, FS_IN, len)
    }

    fn run(ddc: &mut Ddc, input: &[Complex<f32>]) -> Vec<Complex<f32>> {
        let mut out = Vec::new();
        let mut collected = Vec::new();
        for chunk in input.chunks(BLOCK) {
            ddc.process(chunk, &mut out);
            collected.extend_from_slice(&out);
        }
        collected
    }

    fn mean_freq_hz(out: &[Complex<f32>], rate: f64) -> f64 {
        let mut sum = 0.0f64;
        for pair in out.windows(2) {
            sum += f64::from((pair[1] * pair[0].conj()).arg());
        }
        sum / (out.len() - 1) as f64 * rate / std::f64::consts::TAU
    }

    #[test]
    fn reset_does_not_splice_old_filter_history_into_a_fresh_signal() {
        for (input_rate, output_rate) in [(240_000.0, 48_000.0), (240_000.0, 44_100.0)] {
            let mut used = Ddc::new(input_rate, output_rate, 1234.0).expect("rates");
            let mut fresh = used.clone();
            let mut actual = Vec::new();
            used.process(&vec![Complex::new(1.0, 0.5); 4001], &mut actual);
            used.reset();
            let signal = tone_at_rate(5678.0, input_rate, 4096);
            used.process(&signal, &mut actual);
            let mut expected = Vec::new();
            fresh.process(&signal, &mut expected);
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn ragged_and_empty_blocks_preserve_output_across_all_resampling_paths() {
        for (input_rate, output_rate) in [
            (20_000_000.0, 48_000.0),
            (20_000_000.0, 240_000.0),
            (240_000.0, 48_000.0),
            (240_000.0, 44_100.0),
            (48_000.0, 48_000.0),
            (48_000.0, 96_000.0),
        ] {
            let signal = tone_at_rate(731.0, input_rate, 16_389);
            let mut whole = Ddc::new(input_rate, output_rate, 123.0).expect("rates");
            let mut ragged = whole.clone();
            let mut expected = Vec::new();
            whole.process(&signal, &mut expected);
            let mut actual = Vec::new();
            let mut block = Vec::new();
            let mut at = 0;
            for len in [1, 17, 2048, 3, 4099, 0].into_iter().cycle() {
                let end = (at + len).min(signal.len());
                ragged.process(&signal[at..end], &mut block);
                actual.extend_from_slice(&block);
                at = end;
                if at == signal.len() {
                    break;
                }
            }
            assert_eq!(actual, expected, "{input_rate} -> {output_rate}");
            ragged.reset();
            ragged.process(&signal, &mut actual);
            assert_eq!(actual, expected, "reset {input_rate} -> {output_rate}");
        }
    }

    #[test]
    fn rejects_rates_that_are_not_positive_and_finite() {
        for (input, output) in [
            (f64::NAN, 48_000.0),
            (48_000.0, f64::INFINITY),
            (0.0, 48_000.0),
            (48_000.0, -1.0),
        ] {
            assert!(
                matches!(
                    Ddc::new(input, output, 0.0),
                    Err(DdcError::InvalidRates { .. })
                ),
                "{input}→{output}"
            );
        }
    }

    #[test]
    fn upsampling_leaves_no_image_of_a_tone_near_the_input_edge() {
        let (fs_in, fs_out) = (12_000.0f64, 48_000.0f64);
        let tone_hz = 0.4 * fs_in;
        let mut ddc = Ddc::new(fs_in, fs_out, 0.0).unwrap();
        let collected = run(&mut ddc, &tone_at_rate(tone_hz, fs_in, 24_000));
        let settled = &collected[4_096..];
        let level = |hz: f64| {
            let sum: Complex<f64> = settled
                .iter()
                .enumerate()
                .map(|(n, s)| {
                    let phase = -std::f64::consts::TAU * hz * n as f64 / fs_out;
                    Complex::new(f64::from(s.re), f64::from(s.im)) * Complex::from_polar(1.0, phase)
                })
                .sum();
            20.0 * (sum.norm() / settled.len() as f64).log10()
        };
        let tone = level(tone_hz);
        for image in [tone_hz - fs_in, tone_hz + fs_in] {
            let below = tone - level(image);
            assert!(below > 60.0, "image at {image} Hz only {below:.1} dB down");
        }
    }

    #[test]
    fn upsampling_keeps_the_tone_and_delivers_the_output_rate() {
        for (fs_in, fs_out) in [
            (2_000_000.0f64, 2_400_000.0f64),
            (2_048_000.0, 2_400_000.0),
            (2_400_000.0, 16_000_000.0),
        ] {
            let offset = 0.1 * fs_in;
            let mut ddc = Ddc::new(fs_in, fs_out, offset).unwrap();
            let total_in = (fs_in / 8.0) as usize;
            let collected = run(
                &mut ddc,
                &tone_at_rate(offset + 0.05 * fs_in, fs_in, total_in),
            );
            let ideal = (total_in as f64 * fs_out / fs_in) as i64;
            assert!(
                (collected.len() as i64 - ideal).abs() <= 2,
                "{fs_in}→{fs_out}: got {} S, ideal {ideal}",
                collected.len()
            );
            let settled = &collected[1024..];
            let rms = rms_c(settled);
            assert!((0.97..1.03).contains(&rms), "{fs_in}→{fs_out}: rms {rms}");
            let freq = mean_freq_hz(settled, fs_out);
            let want = 0.05 * fs_in;
            assert!(
                (freq - want).abs() < 0.001 * want,
                "{fs_in}→{fs_out}: tone at {freq} Hz, wanted {want} Hz"
            );
        }
    }

    #[test]
    fn tone_at_offset_lands_at_dc() {
        let offset = 400_000.0;
        let mut ddc = Ddc::new(FS_IN, FS_OUT, offset).unwrap();
        let collected = run(&mut ddc, &tone(offset, 262_144));
        let settled = &collected[512..];
        for (i, y) in settled.iter().enumerate() {
            let mag = y.norm();
            assert!((0.97..1.03).contains(&mag), "sample {i}: |y| = {mag}");
        }
        let freq = mean_freq_hz(settled, FS_OUT);
        assert!(freq.abs() < 2.0, "residual frequency {freq} Hz");
    }

    #[test]
    fn tone_1_2x_output_rate_away_suppressed_over_50_db() {
        let offset = 400_000.0;
        let mut ddc = Ddc::new(FS_IN, FS_OUT, offset).unwrap();
        let collected = run(&mut ddc, &tone(offset + 1.2 * FS_OUT, 262_144));
        let rms = rms_c(&collected[512..]);
        assert!(rms < 3.16e-3, "leak rms {rms}");
    }

    #[test]
    fn quotient_below_two_still_suppresses_folding_blockers_over_50_db() {
        for (fs_in, fs_out, blocker_hz) in [
            (460_000.0, 240_000.0, 145_000.0),
            (76_800.0, 48_000.0, 29_000.0),
        ] {
            let mut ddc = Ddc::new(fs_in, fs_out, 0.0).unwrap();
            let collected = run(&mut ddc, &tone_at_rate(blocker_hz, fs_in, 262_144));
            let rms = rms_c(&collected[512..]);
            assert!(rms < 3.16e-3, "{fs_in}→{fs_out}: blocker leak rms {rms}");

            let mut ddc = Ddc::new(fs_in, fs_out, 0.0).unwrap();
            let inband = run(&mut ddc, &tone_at_rate(0.35 * fs_out, fs_in, 262_144));
            let rms = rms_c(&inband[512..]);
            assert!(
                (0.97..1.03).contains(&rms),
                "{fs_in}→{fs_out}: in-band rms {rms}"
            );
        }
    }

    #[test]
    fn exact_long_run_output_rate() {
        for (fs_in, fs_out) in [
            (2_048_000.0f64, 48_000.0f64),
            (2_400_000.0, 240_000.0),
            (20_000_000.0, 240_000.0),
            (19_920_000.0, 240_000.0),
        ] {
            let mut ddc = Ddc::new(fs_in, fs_out, 0.0).unwrap();
            let total_in = fs_in as usize;
            let input = vec![Complex::new(1.0f32, 0.0); total_in];
            let mut out = Vec::new();
            let mut count = 0i64;
            for chunk in input.chunks(BLOCK) {
                ddc.process(chunk, &mut out);
                count += out.len() as i64;
            }
            let ideal = fs_out as i64;
            assert!(
                (count - ideal).abs() <= 2,
                "{fs_in}→{fs_out}: got {count} S/s, ideal {ideal}"
            );
        }
    }

    #[test]
    fn a_stage_by_two_is_a_halfband_only_where_it_is_cheaper() {
        let early = stage_filter(2_048_000.0, 2, 48_000.0);
        assert!(matches!(early, StageFilter::Halfband { .. }), "{early:?}");
        let last = stage_filter(96_000.0, 2, 48_000.0);
        assert!(matches!(last, StageFilter::Lowpass { .. }), "{last:?}");
        assert!(matches!(
            stage_filter(2_048_000.0, 3, 48_000.0),
            StageFilter::Lowpass { .. }
        ));
    }

    #[test]
    fn every_plan_keeps_the_passband_flat_and_the_neighbours_a_hundred_db_down() {
        for (input_rate, output_rate) in [
            (2_048_000.0, 64_000.0),
            (20_480_000.0, 40_000.0),
            (61_440_000.0, 48_000.0),
            (8_000_000.0, 12_500.0),
            (2_000_000.0, 44_100.0),
        ] {
            for relative in [0.0, 0.25, -0.4, 0.4, 0.6, -0.6, 0.75, 1.5, 7.0] {
                let mut ddc = Ddc::new(input_rate, output_rate, 0.0).expect("rates");
                let input = tone_at_rate(relative * output_rate, input_rate, 200_000);
                let output = run(&mut ddc, &input);
                let db = 20.0 * f64::from(rms_c(&output[ddc.settling()..])).log10();
                if relative.abs() <= 0.4 {
                    assert!(db.abs() < 1e-3, "{input_rate} {relative}: passband {db} dB");
                } else {
                    assert!(db < -99.5, "{input_rate} {relative}: neighbour {db} dB");
                }
            }
        }
    }

    #[test]
    fn prime_ratios_keep_passband_and_reject_aliases() {
        for input_rate in [19_920_000.0, 20_000_000.0, 8_000_000.0, 3_200_000.0] {
            let output_rate = 240_000.0;
            let offset = 100_000.0;
            let intermediate = input_rate / integer_decimation(input_rate / output_rate) as f64;
            for relative in [
                0.0,
                0.35 * output_rate,
                -0.35 * output_rate,
                0.4 * output_rate,
                -0.4 * output_rate,
                0.6 * output_rate,
                -0.6 * output_rate,
                intermediate - 0.35 * output_rate,
                intermediate + 0.35 * output_rate,
                input_rate / 2.0 - offset - 1000.0,
            ] {
                let mut ddc = Ddc::new(input_rate, output_rate, offset).expect("rates");
                let input = tone_at_rate(offset + relative, input_rate, 262_144);
                let output = run(&mut ddc, &input);
                let settled = &output[512..];
                let rms = rms_c(settled);
                if relative.abs() <= 0.4 * output_rate {
                    assert!(
                        (0.97..1.03).contains(&rms),
                        "{input_rate} {relative}: passband {rms}"
                    );
                    assert!((mean_freq_hz(settled, output_rate) - relative).abs() < 3.0);
                } else {
                    assert!(rms < 3.16e-3, "{input_rate} {relative}: alias {rms}");
                }
            }
        }
    }

    #[test]
    fn set_offset_retunes_within_one_block() {
        let (f1, f2) = (300_000.0, -250_000.0);
        let mut ddc = Ddc::new(FS_IN, FS_OUT, f1).unwrap();
        let mut out = Vec::new();

        let phase1 = tone(f1, 20 * BLOCK);
        let mut settled = Vec::new();
        for (i, chunk) in phase1.chunks(BLOCK).enumerate() {
            ddc.process(chunk, &mut out);
            if i >= 1 {
                settled.extend_from_slice(&out);
            }
        }
        for y in &settled {
            assert!(
                (0.9..1.1).contains(&y.norm()),
                "pre-retune |y| = {}",
                y.norm()
            );
        }

        ddc.set_offset(f2);
        let phase2 = tone(f2, 20 * BLOCK);
        settled.clear();
        for (i, chunk) in phase2.chunks(BLOCK).enumerate() {
            ddc.process(chunk, &mut out);
            if i >= 1 {
                settled.extend_from_slice(&out);
            }
        }
        for y in &settled {
            assert!(
                (0.9..1.1).contains(&y.norm()),
                "post-retune |y| = {}",
                y.norm()
            );
        }
        let freq = mean_freq_hz(&settled, FS_OUT);
        assert!(freq.abs() < 2.0, "post-retune residual frequency {freq} Hz");
    }
}
