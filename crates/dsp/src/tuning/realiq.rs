use num_complex::Complex;

use crate::{
    filter::fir::{StreamFir, design_halfband, kaiser_transition},
    tuning::ddc::STOPBAND_DB,
};

pub const DEFAULT_TAPS: usize = 131;

#[must_use]
pub fn clean_fraction(taps: usize) -> f64 {
    1.0 - 2.0 * kaiser_transition(taps, STOPBAND_DB)
}

/// Turns the real samples a quadrature-sampling receiver delivers into the complex baseband the
/// rest of the chain expects, at half the input rate.
///
/// The band of interest sits at a quarter of the input rate. Translating by that quarter costs no
/// multiplies, the rotation cycles through 1, -j, -1, j, and leaves the wanted signal at DC and
/// its mirror at the edge, where a half-band low-pass removes it on the way down by two.
#[derive(Clone, Debug)]
pub struct RealToIq {
    filter: StreamFir<f32, f32>,
    centre_tap: f32,
    in_phase: Vec<f32>,
    filtered: Vec<f32>,
    quadrature: Vec<f32>,
    quadrature_delay: usize,
    phase: u8,
    quadrature_sign: f32,
}

impl RealToIq {
    /// # Panics
    /// Panics unless `taps` is at least 3 and congruent to 3 modulo 4, which is what puts a zero
    /// at every even offset from the centre tap and makes the filter a half-band.
    #[must_use]
    pub fn new(taps: usize) -> Self {
        let lowpass = design_halfband(taps, STOPBAND_DB);
        let centre = taps / 2;
        let even_taps: Vec<f32> = lowpass.iter().step_by(2).copied().collect();
        let mut converter = Self {
            filter: StreamFir::new(&even_taps, 1),
            centre_tap: lowpass[centre],
            in_phase: Vec::new(),
            filtered: Vec::new(),
            quadrature: Vec::new(),
            quadrature_delay: centre.div_ceil(2),
            phase: 0,
            quadrature_sign: -1.0,
        };
        converter.reset();
        converter
    }

    #[must_use]
    pub fn inverted(mut self) -> Self {
        self.quadrature_sign = 1.0;
        self
    }

    pub fn reset(&mut self) {
        self.filter.reset();
        self.in_phase.clear();
        self.quadrature.clear();
        self.quadrature.resize(self.quadrature_delay, 0.0);
        self.phase = 0;
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<Complex<f32>>) {
        self.in_phase.clear();
        let lead = usize::from((4 - self.phase) & 3).min(input.len());
        let (head, aligned) = input.split_at(lead);
        let (quads, tail) = aligned.as_chunks::<4>();
        for &sample in head {
            self.rotate(sample);
        }
        self.rotate_quads(quads);
        for &sample in tail {
            self.rotate(sample);
        }
        self.filter.process(&self.in_phase, &mut self.filtered);
        let centre_tap = self.centre_tap;
        let len = self.filtered.len().min(self.quadrature.len());
        out.clear();
        out.reserve(len);
        interleave_scaled(
            &self.filtered[..len],
            &self.quadrature[..len],
            centre_tap,
            out,
        );
        self.quadrature.drain(..len);
    }

    fn rotate(&mut self, sample: f32) {
        match self.phase {
            0 => self.in_phase.push(sample),
            1 => self.quadrature.push(self.quadrature_sign * sample),
            2 => self.in_phase.push(-sample),
            _ => self.quadrature.push(-self.quadrature_sign * sample),
        }
        self.phase = (self.phase + 1) & 3;
    }

    fn rotate_quads(&mut self, quads: &[[f32; 4]]) {
        let sign = self.quadrature_sign;
        let add = 2 * quads.len();
        let i_base = self.in_phase.len();
        let q_base = self.quadrature.len();
        self.in_phase.reserve(add);
        self.quadrature.reserve(add);
        let i_dst = self
            .in_phase
            .spare_capacity_mut()
            .as_mut_ptr()
            .cast::<f32>();
        let q_dst = self
            .quadrature
            .spare_capacity_mut()
            .as_mut_ptr()
            .cast::<f32>();
        let mut done = 0;
        #[cfg(target_arch = "aarch64")]
        {
            use std::arch::aarch64::{
                float32x4x2_t, vdupq_n_f32, vld4q_f32, vmulq_f32, vnegq_f32, vst2q_f32,
            };
            let n = quads.len() & !3;
            unsafe {
                let sp = quads.as_ptr().cast::<f32>();
                let v_sign = vdupq_n_f32(sign);
                let v_neg_sign = vdupq_n_f32(-sign);
                while done < n {
                    let v = vld4q_f32(sp.add(done * 4));
                    vst2q_f32(i_dst.add(done * 2), float32x4x2_t(v.0, vnegq_f32(v.2)));
                    vst2q_f32(
                        q_dst.add(done * 2),
                        float32x4x2_t(vmulq_f32(v.1, v_sign), vmulq_f32(v.3, v_neg_sign)),
                    );
                    done += 4;
                }
            }
        }
        #[cfg(target_arch = "x86_64")]
        {
            if std::arch::is_x86_feature_detected!("avx2") {
                done = unsafe { rotate_quads_avx2(quads, sign, i_dst, q_dst) };
            }
        }
        for quad in &quads[done..] {
            unsafe {
                i_dst.add(done * 2).write(quad[0]);
                i_dst.add(done * 2 + 1).write(-quad[2]);
                q_dst.add(done * 2).write(sign * quad[1]);
                q_dst.add(done * 2 + 1).write(-sign * quad[3]);
            }
            done += 1;
        }
        unsafe {
            self.in_phase.set_len(i_base + add);
            self.quadrature.set_len(q_base + add);
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn rotate_quads_avx2(
    quads: &[[f32; 4]],
    sign: f32,
    i_dst: *mut f32,
    q_dst: *mut f32,
) -> usize {
    use std::arch::x86_64::{
        _mm256_castpd_ps, _mm256_castps_pd, _mm256_loadu_ps, _mm256_mul_ps, _mm256_permute4x64_pd,
        _mm256_setr_ps, _mm256_shuffle_ps, _mm256_storeu_ps, _mm256_xor_ps,
    };
    let n = quads.len() & !3;
    let sp = quads.as_ptr().cast::<f32>();
    let neg_odd = _mm256_setr_ps(0.0, -0.0, 0.0, -0.0, 0.0, -0.0, 0.0, -0.0);
    let q_scale = _mm256_setr_ps(sign, -sign, sign, -sign, sign, -sign, sign, -sign);
    let mut done = 0;
    while done < n {
        unsafe {
            let v0 = _mm256_loadu_ps(sp.add(done * 4));
            let v1 = _mm256_loadu_ps(sp.add(done * 4 + 8));
            let i_shuf = _mm256_shuffle_ps::<0x88>(v0, v1);
            let i_ord = _mm256_castpd_ps(_mm256_permute4x64_pd::<0xD8>(_mm256_castps_pd(i_shuf)));
            _mm256_storeu_ps(i_dst.add(done * 2), _mm256_xor_ps(i_ord, neg_odd));
            let q_shuf = _mm256_shuffle_ps::<0xDD>(v0, v1);
            let q_ord = _mm256_castpd_ps(_mm256_permute4x64_pd::<0xD8>(_mm256_castps_pd(q_shuf)));
            _mm256_storeu_ps(q_dst.add(done * 2), _mm256_mul_ps(q_ord, q_scale));
        }
        done += 4;
    }
    done
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn interleave_scaled_avx2(re: &[f32], im: &[f32], scale: f32, dst: *mut f32) -> usize {
    use std::arch::x86_64::{
        _mm256_loadu_ps, _mm256_mul_ps, _mm256_permute2f128_ps, _mm256_set1_ps, _mm256_storeu_ps,
        _mm256_unpackhi_ps, _mm256_unpacklo_ps,
    };
    let len = re.len().min(im.len());
    let n = len & !7;
    let rp = re.as_ptr();
    let ip = im.as_ptr();
    let vs = _mm256_set1_ps(scale);
    let mut done = 0;
    while done < n {
        unsafe {
            let vr = _mm256_loadu_ps(rp.add(done));
            let vi = _mm256_mul_ps(_mm256_loadu_ps(ip.add(done)), vs);
            let lo = _mm256_unpacklo_ps(vr, vi);
            let hi = _mm256_unpackhi_ps(vr, vi);
            _mm256_storeu_ps(dst.add(done * 2), _mm256_permute2f128_ps::<0x20>(lo, hi));
            _mm256_storeu_ps(
                dst.add(done * 2 + 8),
                _mm256_permute2f128_ps::<0x31>(lo, hi),
            );
        }
        done += 8;
    }
    done
}

fn interleave_scaled(re: &[f32], im: &[f32], scale: f32, out: &mut Vec<Complex<f32>>) {
    let len = re.len().min(im.len());
    let base = out.len();
    let dst = out.spare_capacity_mut().as_mut_ptr().cast::<f32>();
    let mut done = 0;
    #[cfg(target_arch = "aarch64")]
    {
        use std::arch::aarch64::{float32x4x2_t, vdupq_n_f32, vld1q_f32, vmulq_f32, vst2q_f32};
        let n = len & !3;
        unsafe {
            let rp = re.as_ptr();
            let ip = im.as_ptr();
            let vs = vdupq_n_f32(scale);
            while done < n {
                let vr = vld1q_f32(rp.add(done));
                let vi = vmulq_f32(vld1q_f32(ip.add(done)), vs);
                vst2q_f32(dst.add(done * 2), float32x4x2_t(vr, vi));
                done += 4;
            }
        }
    }
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            done = unsafe { interleave_scaled_avx2(re, im, scale, dst) };
        }
    }
    while done < len {
        unsafe {
            dst.add(done * 2).write(re[done]);
            dst.add(done * 2 + 1).write(scale * im[done]);
        }
        done += 1;
    }
    unsafe {
        out.set_len(base + len);
    }
}

impl Default for RealToIq {
    fn default() -> Self {
        Self::new(DEFAULT_TAPS)
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use super::*;
    use crate::{
        filter::fir::{design_lowpass_kaiser, in_ragged_blocks},
        math::complex::widen,
        math::db::Decibels,
        math::fft::Transform,
        math::window::blackman_harris,
        testutil::tone_peak_and_snr,
    };

    fn windowed_level(samples: &[Complex<f32>], frequency: f64) -> f64 {
        let window = blackman_harris(samples.len());
        samples
            .iter()
            .zip(&window)
            .enumerate()
            .map(|(k, (x, &w))| {
                widen(*x) * f64::from(w) * Complex::from_polar(1.0, -TAU * frequency * k as f64)
            })
            .sum::<Complex<f64>>()
            .norm()
    }

    #[test]
    fn the_mirror_stays_a_hundred_db_down_across_the_clean_fraction() {
        for share in [0.1, 0.5, 0.8, clean_fraction(DEFAULT_TAPS)] {
            let frequency = 0.25 + share * 0.25;
            let input: Vec<f32> = (0..32_768)
                .map(|k| (TAU * frequency * f64::from(k)).cos() as f32)
                .collect();
            let mut out = Vec::new();
            RealToIq::default().process(&input, &mut out);
            let tail = &out[out.len() / 2..];
            let (a, b) = (
                windowed_level(tail, share / 2.0),
                windowed_level(tail, -share / 2.0),
            );
            let image = (a.min(b) / a.max(b)).amplitude_to_db();
            assert!(
                image < 1.0 - STOPBAND_DB,
                "{share} of the band: image at {image} dB"
            );
        }
    }

    fn real_cosine(freq_norm: f64, len: usize) -> Vec<f32> {
        (0..len)
            .map(|n| (TAU * freq_norm * n as f64).cos() as f32)
            .collect()
    }

    fn converted(freq_norm: f64, len: usize) -> Vec<Complex<f32>> {
        let mut converter = RealToIq::default();
        let mut out = Vec::new();
        converter.process(&real_cosine(freq_norm, len), &mut out);
        out
    }

    #[test]
    fn a_tone_at_a_quarter_of_the_rate_lands_on_dc() {
        let out = converted(0.25, 8192);
        let settled = &out[128..];
        let mean = settled.iter().sum::<Complex<f32>>() / settled.len() as f32;
        assert!(
            mean.norm() > 0.45,
            "a tone at the centre must survive as a steady vector, got {}",
            mean.norm()
        );
    }

    const WINDOW: usize = 4096;

    fn settled(freq_norm: f64) -> Vec<Complex<f32>> {
        converted(freq_norm, 2 * WINDOW + 1024)[512..512 + WINDOW].to_vec()
    }

    fn bin_power(samples: &[Complex<f32>], bin: i64) -> f64 {
        let n = samples.len();
        let mut buf = samples.to_vec();
        Transform::forward(n).process(&mut buf);
        f64::from(buf[bin.rem_euclid(n as i64) as usize].norm_sqr())
    }

    /// Offsets are chosen to land on a whole output bin, so that what the assertions measure is
    /// the converter rather than the leakage of an unwindowed tone between two bins.
    fn offset_for_bin(bin: i64) -> f64 {
        bin as f64 / (2.0 * WINDOW as f64)
    }

    #[test]
    fn an_offset_tone_lands_at_twice_its_offset_and_keeps_its_sign() {
        for bin in [160_i64, -160] {
            let offset = offset_for_bin(bin);
            let out = settled(0.25 + offset);
            let (peak, snr) = tone_peak_and_snr(&out);
            let expected = bin.rem_euclid(WINDOW as i64) as usize;
            assert!(
                peak.abs_diff(expected) <= 1,
                "an offset of {offset} should appear at bin {expected}, found {peak}"
            );
            assert!(
                snr > 45.0,
                "an offset of {offset} kept only {snr} dB above everything else"
            );
        }
    }

    /// The whole real band below Nyquist is kept, not just a window around the centre: input
    /// frequency `f` arrives at `2(f - 1/4)`, which sweeps the full output band as `f` sweeps
    /// from 0 to a half.
    #[test]
    fn the_whole_real_band_maps_onto_the_output_band() {
        for bin in [-1600_i64, -800, 800, 1600] {
            let offset = offset_for_bin(bin);
            let out = settled(0.25 + offset);
            let (peak, _) = tone_peak_and_snr(&out);
            let expected = bin.rem_euclid(WINDOW as i64) as usize;
            assert!(
                peak.abs_diff(expected) <= 1,
                "an offset of {offset} should appear at bin {expected}, found {peak}"
            );
        }
    }

    /// A real input carries a tone at `+f` and at `-f` alike. Only one of the two may survive as
    /// a complex output, or every signal arrives with a mirror image folded over it.
    #[test]
    fn the_unwanted_half_of_a_real_tone_is_rejected() {
        let wanted = 240_i64;
        let out = settled(0.25 + offset_for_bin(wanted));
        let rejection =
            (bin_power(&out, wanted) / bin_power(&out, -wanted).max(1e-30)).power_to_db();
        assert!(
            rejection > 50.0,
            "the mirror of the wanted tone is only {rejection} dB down"
        );
    }

    #[test]
    fn the_output_runs_at_half_the_input_rate() {
        let mut converter = RealToIq::default();
        let mut out = Vec::new();
        converter.process(&real_cosine(0.25, 4096), &mut out);
        assert_eq!(out.len(), 2048);
    }

    #[test]
    fn ragged_blocks_match_one_shot_exactly() {
        let input = real_cosine(0.27, 10_000);
        let mut whole = RealToIq::default();
        let mut expected = Vec::new();
        whole.process(&input, &mut expected);

        let mut ragged = RealToIq::default();
        let got = in_ragged_blocks(&input, &[1, 7, 64, 3, 129, 1024, 17], |chunk, out| {
            ragged.process(chunk, out);
        });
        assert_eq!(expected, got);
    }

    #[test]
    fn a_reset_converter_repeats_its_first_output() {
        let input = real_cosine(0.3, 2048);
        let mut converter = RealToIq::default();
        let mut first = Vec::new();
        converter.process(&input, &mut first);
        converter.reset();
        let mut again = Vec::new();
        converter.process(&input, &mut again);
        assert_eq!(first, again);
    }

    #[test]
    fn an_inverted_converter_mirrors_the_band() {
        for bin in [160_i64, -800] {
            let mut converter = RealToIq::default().inverted();
            let mut out = Vec::new();
            converter.process(
                &real_cosine(0.25 + offset_for_bin(bin), 2 * WINDOW + 1024),
                &mut out,
            );
            let (peak, snr) = tone_peak_and_snr(&out[512..512 + WINDOW]);
            let expected = (-bin).rem_euclid(WINDOW as i64) as usize;
            assert!(
                peak.abs_diff(expected) <= 1,
                "expected {expected}, found {peak}"
            );
            assert!(snr > 45.0, "kept only {snr} dB");
        }
    }

    fn full_filter(input: &[f32], quadrature_sign: f32) -> Vec<Complex<f32>> {
        let rotated: Vec<Complex<f32>> = input
            .iter()
            .enumerate()
            .map(|(n, &x)| match n % 4 {
                0 => Complex::new(x, 0.0),
                1 => Complex::new(0.0, quadrature_sign * x),
                2 => Complex::new(-x, 0.0),
                _ => Complex::new(0.0, -quadrature_sign * x),
            })
            .collect();
        let mut out = Vec::new();
        crate::filter::decim::Decimator::new(
            &design_lowpass_kaiser(DEFAULT_TAPS, 0.25, STOPBAND_DB),
            2,
        )
        .process(&rotated, &mut out);
        out
    }

    #[test]
    fn the_half_band_shortcut_matches_the_full_filter() {
        let input: Vec<f32> = (0..5001)
            .map(|n| ((n * 7919) % 4096) as f32 / 2048.0 - 1.0)
            .collect();
        for (converter, sign) in [
            (RealToIq::default(), -1.0),
            (RealToIq::default().inverted(), 1.0),
        ] {
            let mut converter = converter;
            let mut got = Vec::new();
            converter.process(&input, &mut got);
            let expected = full_filter(&input, sign);
            assert_eq!(got.len(), expected.len());
            let worst = got
                .iter()
                .zip(&expected)
                .map(|(a, b)| (a - b).norm())
                .fold(0.0f32, f32::max);
            assert!(worst < 1e-5, "differs by {worst}");
        }
    }

    #[test]
    #[should_panic(expected = "half-band filter has 4k+3 taps")]
    fn a_tap_count_that_is_not_half_band_is_refused() {
        let _ = RealToIq::new(49);
    }
}
