use core::ops::Range;

use num_complex::Complex;

use crate::{
    math::complex::{add_cross_f64, add_norm_sqr_f64, narrow, scale_real_into},
    math::db::Decibels,
    math::fft::Transform,
    math::linalg::MAX_ORDER,
    math::special::parabolic_peak,
    math::window::hann,
};

pub const MIN_CORRELATOR_FFT: usize = 64;
pub const MAX_CORRELATOR_FFT: usize = 8192;
pub const DELAY_PAD: usize = 4;
const FRAME_BATCH: usize = 4;

type C64 = Complex<f64>;

const ZERO: Complex<f32> = Complex::new(0.0, 0.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CorrelatorError {
    #[error("fft size {0} is not a power of two in 64..=8192")]
    FftSize(usize),
    #[error("a correlator needs 2 to 16 lanes, not {0}")]
    Lanes(usize),
    #[error("lanes differ in length")]
    LaneLength,
    #[error("baseline {0} does not exist")]
    Baseline(usize),
    #[error("bins {0}..{1} are outside the spectrum")]
    Bins(usize, usize),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BandVisibility {
    pub amplitude: f64,
    pub phase_rad: f64,
    pub coherence: f64,
    pub snr_db: f32,
}

pub struct FxCorrelator {
    lanes: usize,
    fft_size: usize,
    hop: usize,
    window: Vec<f32>,
    fft: Transform<f32>,
    frames: Vec<Vec<Complex<f32>>>,
    spectra: Vec<Complex<f32>>,
    fill: usize,
    pairs: Vec<(usize, usize)>,
    cross: Vec<C64>,
    auto: Vec<f64>,
    count: u64,
    lag: Vec<Complex<f32>>,
    lag_fft: Transform<f32>,
}

impl FxCorrelator {
    pub fn new(lanes: usize, fft_size: usize, overlap: bool) -> Result<Self, CorrelatorError> {
        if !(2..=MAX_ORDER).contains(&lanes) {
            return Err(CorrelatorError::Lanes(lanes));
        }
        if !(fft_size.is_power_of_two()
            && (MIN_CORRELATOR_FFT..=MAX_CORRELATOR_FFT).contains(&fft_size))
        {
            return Err(CorrelatorError::FftSize(fft_size));
        }
        let pairs: Vec<(usize, usize)> = (0..lanes)
            .flat_map(|a| (a + 1..lanes).map(move |b| (a, b)))
            .collect();
        let padded = DELAY_PAD * fft_size;
        Ok(Self {
            lanes,
            fft_size,
            hop: if overlap { fft_size / 2 } else { fft_size },
            window: hann(fft_size),
            fft: Transform::forward(fft_size),
            frames: vec![vec![ZERO; fft_size]; lanes],
            spectra: vec![ZERO; FRAME_BATCH * lanes * fft_size],
            fill: 0,
            cross: vec![C64::new(0.0, 0.0); pairs.len() * fft_size],
            auto: vec![0.0; lanes * fft_size],
            pairs,
            count: 0,
            lag: vec![ZERO; padded],
            lag_fft: Transform::inverse(padded),
        })
    }

    #[must_use]
    pub const fn lanes(&self) -> usize {
        self.lanes
    }

    #[must_use]
    pub const fn fft_size(&self) -> usize {
        self.fft_size
    }

    #[must_use]
    pub const fn hop(&self) -> usize {
        self.hop
    }

    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.count
    }

    #[must_use]
    pub fn baselines(&self) -> usize {
        self.pairs.len()
    }

    #[must_use]
    pub fn pair(&self, baseline: usize) -> Option<(usize, usize)> {
        self.pairs.get(baseline).copied()
    }

    pub fn push(&mut self, lanes: &[&[Complex<f32>]]) -> Result<u32, CorrelatorError> {
        if lanes.len() != self.lanes {
            return Err(CorrelatorError::Lanes(lanes.len()));
        }
        let len = lanes[0].len();
        if lanes.iter().any(|lane| lane.len() != len) {
            return Err(CorrelatorError::LaneLength);
        }
        let overlap = self.fft_size - self.hop;
        let mut done = 0;
        let mut at = 0;
        while self.fill > 0 && at < len {
            let take = (self.fft_size - self.fill).min(len - at);
            for (frame, lane) in self.frames.iter_mut().zip(lanes) {
                frame[self.fill..self.fill + take].copy_from_slice(&lane[at..at + take]);
            }
            self.fill += take;
            at += take;
            if self.fill == self.fft_size {
                self.transform_buffered(0);
                self.accumulate_batch(1);
                done += 1;
                if at >= overlap {
                    at -= overlap;
                    self.fill = 0;
                } else {
                    for frame in &mut self.frames {
                        frame.copy_within(self.hop.., 0);
                    }
                    self.fill = overlap;
                }
            }
        }
        if self.fill == 0 {
            let f = self.fft_size;
            let stride = self.lanes * f;
            while at + f <= len {
                let mut batch = 0;
                while batch < FRAME_BATCH && at + f <= len {
                    for (lane_idx, lane) in lanes.iter().enumerate() {
                        let start = batch * stride + lane_idx * f;
                        let dst = &mut self.spectra[start..start + f];
                        scale_real_into(dst, &lane[at..at + f], &self.window);
                        self.fft.process(dst);
                    }
                    at += self.hop;
                    batch += 1;
                }
                self.accumulate_batch(batch);
                done += batch as u32;
            }
            let rem = len - at;
            if rem > 0 {
                for (frame, lane) in self.frames.iter_mut().zip(lanes) {
                    frame[..rem].copy_from_slice(&lane[at..len]);
                }
                self.fill = rem;
            }
        }
        Ok(done)
    }

    #[must_use]
    pub fn visibility(&self, baseline: usize, bin: usize) -> Option<C64> {
        if baseline >= self.pairs.len() || bin >= self.fft_size || self.count == 0 {
            return None;
        }
        Some(self.cross[baseline * self.fft_size + self.raw_bin(bin)] / self.count as f64)
    }

    #[must_use]
    pub fn auto(&self, lane: usize, bin: usize) -> Option<f64> {
        if lane >= self.lanes || bin >= self.fft_size || self.count == 0 {
            return None;
        }
        Some(self.auto[lane * self.fft_size + self.raw_bin(bin)] / self.count as f64)
    }

    pub fn band(
        &self,
        baseline: usize,
        bins: Range<usize>,
    ) -> Result<BandVisibility, CorrelatorError> {
        let (a, b) = self.checked(baseline, &bins)?;
        if self.count == 0 {
            return Ok(BandVisibility::default());
        }
        let sum = self.cross_sum(baseline, &bins);
        let norm = self.power_norm(a, b, &bins);
        let coherence = if norm > 0.0 { sum.norm() / norm } else { 0.0 };
        let snr = coherence * (self.count as f64 * bins.len() as f64).sqrt();
        Ok(BandVisibility {
            amplitude: sum.norm() / self.count as f64,
            phase_rad: sum.arg(),
            coherence,
            snr_db: if snr > 0.0 {
                snr.amplitude_to_db() as f32
            } else {
                f32::NEG_INFINITY
            },
        })
    }

    pub fn delay_samples(
        &mut self,
        baseline: usize,
        bins: Range<usize>,
    ) -> Result<f64, CorrelatorError> {
        let (a, b) = self.checked(baseline, &bins)?;
        let norm = self.power_norm(a, b, &bins);
        if !(norm.is_finite() && norm > 0.0) {
            return Ok(0.0);
        }
        let f = self.fft_size;
        let padded = self.lag.len() as i64;
        self.lag.fill(ZERO);
        for bin in bins {
            let slot = (bin as i64 - (f / 2) as i64).rem_euclid(padded) as usize;
            let value = self.cross[baseline * f + self.raw_bin(bin)].conj() / norm;
            self.lag[slot] = narrow(value);
        }
        self.lag_fft.process(&mut self.lag);
        Ok(peak_lag(&self.lag) / DELAY_PAD as f64)
    }

    pub fn clear_integration(&mut self) {
        self.cross.fill(C64::new(0.0, 0.0));
        self.auto.fill(0.0);
        self.count = 0;
    }

    pub fn reset(&mut self) {
        self.clear_integration();
        self.fill = 0;
    }

    fn checked(
        &self,
        baseline: usize,
        bins: &Range<usize>,
    ) -> Result<(usize, usize), CorrelatorError> {
        let pair = self
            .pair(baseline)
            .ok_or(CorrelatorError::Baseline(baseline))?;
        if bins.start >= bins.end || bins.end > self.fft_size {
            return Err(CorrelatorError::Bins(bins.start, bins.end));
        }
        Ok(pair)
    }

    #[inline]
    const fn raw_bin(&self, bin: usize) -> usize {
        (bin + self.fft_size / 2) & (self.fft_size - 1)
    }

    fn cross_sum(&self, baseline: usize, bins: &Range<usize>) -> C64 {
        let base = baseline * self.fft_size;
        bins.clone()
            .map(|bin| self.cross[base + self.raw_bin(bin)])
            .sum()
    }

    fn power_norm(&self, a: usize, b: usize, bins: &Range<usize>) -> f64 {
        let f = self.fft_size;
        let power = |lane: usize| -> f64 {
            bins.clone()
                .map(|bin| self.auto[lane * f + self.raw_bin(bin)])
                .sum()
        };
        (power(a) * power(b)).sqrt()
    }

    fn transform_buffered(&mut self, slot: usize) {
        let f = self.fft_size;
        let base = slot * self.lanes * f;
        for (lane_idx, frame) in self.frames.iter().enumerate() {
            let dst = &mut self.spectra[base + lane_idx * f..base + (lane_idx + 1) * f];
            scale_real_into(dst, frame, &self.window);
            self.fft.process(dst);
        }
    }

    fn accumulate_batch(&mut self, batch: usize) {
        let f = self.fft_size;
        let stride = self.lanes * f;
        #[cfg(target_arch = "aarch64")]
        if batch == FRAME_BATCH {
            for (sums, &(a, b)) in self.cross.chunks_exact_mut(f).zip(&self.pairs) {
                add_cross_f64_4_neon(
                    sums,
                    [
                        &self.spectra[a * f..(a + 1) * f],
                        &self.spectra[stride + a * f..stride + (a + 1) * f],
                        &self.spectra[2 * stride + a * f..2 * stride + (a + 1) * f],
                        &self.spectra[3 * stride + a * f..3 * stride + (a + 1) * f],
                    ],
                    [
                        &self.spectra[b * f..(b + 1) * f],
                        &self.spectra[stride + b * f..stride + (b + 1) * f],
                        &self.spectra[2 * stride + b * f..2 * stride + (b + 1) * f],
                        &self.spectra[3 * stride + b * f..3 * stride + (b + 1) * f],
                    ],
                );
            }
            for (lane, sums) in self.auto.chunks_exact_mut(f).enumerate() {
                add_norm_sqr_f64_4_neon(
                    sums,
                    [
                        &self.spectra[lane * f..(lane + 1) * f],
                        &self.spectra[stride + lane * f..stride + (lane + 1) * f],
                        &self.spectra[2 * stride + lane * f..2 * stride + (lane + 1) * f],
                        &self.spectra[3 * stride + lane * f..3 * stride + (lane + 1) * f],
                    ],
                );
            }
            self.count += batch as u64;
            return;
        }
        for k in 0..batch {
            let base = k * stride;
            for (sums, &(a, b)) in self.cross.chunks_exact_mut(f).zip(&self.pairs) {
                add_cross_f64(
                    sums,
                    &self.spectra[base + a * f..base + (a + 1) * f],
                    &self.spectra[base + b * f..base + (b + 1) * f],
                );
            }
            for (lane, sums) in self.auto.chunks_exact_mut(f).enumerate() {
                add_norm_sqr_f64(sums, &self.spectra[base + lane * f..base + (lane + 1) * f]);
            }
        }
        self.count += batch as u64;
    }
}

#[cfg(target_arch = "aarch64")]
fn add_cross_f64_4_neon(acc: &mut [C64], a: [&[Complex<f32>]; 4], b: [&[Complex<f32>]; 4]) {
    use std::arch::aarch64::{
        float64x2x2_t, vcvt_f64_f32, vcvt_high_f64_f32, vfmaq_f64, vfmsq_f64, vget_low_f32,
        vld2q_f32, vld2q_f64, vst2q_f64,
    };
    let len = acc.len();
    assert!(len.is_multiple_of(4) && a.iter().chain(&b).all(|lane| lane.len() == len));
    let mut i = 0;
    unsafe {
        let sp = acc.as_mut_ptr().cast::<f64>();
        let ap = [
            a[0].as_ptr().cast::<f32>(),
            a[1].as_ptr().cast::<f32>(),
            a[2].as_ptr().cast::<f32>(),
            a[3].as_ptr().cast::<f32>(),
        ];
        let bp = [
            b[0].as_ptr().cast::<f32>(),
            b[1].as_ptr().cast::<f32>(),
            b[2].as_ptr().cast::<f32>(),
            b[3].as_ptr().cast::<f32>(),
        ];
        while i + 4 <= len {
            let off = i * 2;
            let vs0 = vld2q_f64(sp.add(off));
            let vs1 = vld2q_f64(sp.add(off + 4));
            let (mut re0, mut im0) = (vs0.0, vs0.1);
            let (mut re1, mut im1) = (vs1.0, vs1.1);
            for k in 0..4 {
                let va = vld2q_f32(ap[k].add(off));
                let vb = vld2q_f32(bp[k].add(off));
                let ar0 = vcvt_f64_f32(vget_low_f32(va.0));
                let ai0 = vcvt_f64_f32(vget_low_f32(va.1));
                let br0 = vcvt_f64_f32(vget_low_f32(vb.0));
                let bi0 = vcvt_f64_f32(vget_low_f32(vb.1));
                re0 = vfmaq_f64(vfmaq_f64(re0, ar0, br0), ai0, bi0);
                im0 = vfmsq_f64(vfmaq_f64(im0, ai0, br0), ar0, bi0);

                let ar1 = vcvt_high_f64_f32(va.0);
                let ai1 = vcvt_high_f64_f32(va.1);
                let br1 = vcvt_high_f64_f32(vb.0);
                let bi1 = vcvt_high_f64_f32(vb.1);
                re1 = vfmaq_f64(vfmaq_f64(re1, ar1, br1), ai1, bi1);
                im1 = vfmsq_f64(vfmaq_f64(im1, ai1, br1), ar1, bi1);
            }
            vst2q_f64(sp.add(off), float64x2x2_t(re0, im0));
            vst2q_f64(sp.add(off + 4), float64x2x2_t(re1, im1));
            i += 4;
        }
    }
}

#[cfg(target_arch = "aarch64")]
fn add_norm_sqr_f64_4_neon(acc: &mut [f64], s: [&[Complex<f32>]; 4]) {
    use std::arch::aarch64::{
        vcvt_f64_f32, vcvt_high_f64_f32, vfmaq_f64, vget_low_f32, vld1q_f64, vld2q_f32, vst1q_f64,
    };
    let len = acc.len();
    assert!(len.is_multiple_of(4) && s.iter().all(|lane| lane.len() == len));
    let mut i = 0;
    unsafe {
        let dp = acc.as_mut_ptr();
        let sp = [
            s[0].as_ptr().cast::<f32>(),
            s[1].as_ptr().cast::<f32>(),
            s[2].as_ptr().cast::<f32>(),
            s[3].as_ptr().cast::<f32>(),
        ];
        while i + 4 <= len {
            let mut out0 = vld1q_f64(dp.add(i));
            let mut out1 = vld1q_f64(dp.add(i + 2));
            let off = i * 2;
            for p in sp {
                let vs = vld2q_f32(p.add(off));
                let r0 = vcvt_f64_f32(vget_low_f32(vs.0));
                let i0 = vcvt_f64_f32(vget_low_f32(vs.1));
                let r1 = vcvt_high_f64_f32(vs.0);
                let i1 = vcvt_high_f64_f32(vs.1);
                out0 = vfmaq_f64(vfmaq_f64(out0, r0, r0), i0, i0);
                out1 = vfmaq_f64(vfmaq_f64(out1, r1, r1), i1, i1);
            }
            vst1q_f64(dp.add(i), out0);
            vst1q_f64(dp.add(i + 2), out1);
            i += 4;
        }
    }
}

fn peak_lag(lag: &[Complex<f32>]) -> f64 {
    let len = lag.len();
    let magnitude = |index: usize| f64::from(lag[index % len].norm());
    let peak = (0..len)
        .max_by(|&x, &y| magnitude(x).total_cmp(&magnitude(y)))
        .unwrap_or(0);
    let shift = parabolic_peak(
        magnitude(peak + len - 1),
        magnitude(peak),
        magnitude(peak + 1),
    );
    let position = peak as f64 + shift;
    if position > len as f64 / 2.0 {
        position - len as f64
    } else {
        position
    }
}

#[cfg(test)]
mod tests;
