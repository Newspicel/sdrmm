use num_complex::Complex;

use super::{CoarseError, CoarseLag, CoarseSearch, MAX_CFO_HZ, SEGMENT};
use crate::{
    array::array_sync::derotate::derotate, math::complex::mean_power, math::fft::FftPair,
    math::special::parabolic_peak,
};

impl CoarseSearch {
    pub fn lag_across_clocks(
        &mut self,
        reference: &[Complex<f32>],
        lane: &[Complex<f32>],
        rate_hz: f64,
    ) -> Result<CoarseLag, CoarseError> {
        if !(rate_hz.is_finite() && rate_hz > 0.0) {
            return Err(CoarseError::Rate);
        }
        self.check(reference, lane)?;
        let reach = (MAX_CFO_HZ / rate_hz * self.shift.bins() as f64).ceil() as usize;
        let rough = self.shift.estimate(
            &reference[self.lags..self.lags + self.frame],
            &lane[..self.span()],
            reach,
        );
        let cycles = if let Some(rough) = rough {
            let index = self.incoherent_peak(reference, lane, rough);
            rough + self.residual(reference, lane, index, rough)
        } else {
            let index = self.envelope_peak(reference, lane);
            self.product_offset(reference, lane, index, MAX_CFO_HZ / rate_hz)
        };
        let found = self.coherent(reference, lane, cycles)?;
        Ok(CoarseLag {
            cfo_hz: cycles * rate_hz,
            ..found
        })
    }

    fn incoherent_peak(
        &mut self,
        reference: &[Complex<f32>],
        lane: &[Complex<f32>],
        cycles: f64,
    ) -> usize {
        let span = self.span();
        self.lane_spectrum.fill(Complex::default());
        derotate(&lane[..span], 0, cycles, &mut self.lane_spectrum[..span]);
        self.fft.forward(&mut self.lane_spectrum);
        self.power.fill(0.0);
        let segment = SEGMENT.min(self.frame);
        for start in (0..self.frame).step_by(segment) {
            self.load_reference(reference, start, (start + segment).min(self.frame));
            for ((value, lane), reference) in
                self.b.iter_mut().zip(&self.lane_spectrum).zip(&self.a)
            {
                *value = lane * reference.conj();
            }
            self.fft.inverse(&mut self.b);
            for (power, value) in self.power.iter_mut().zip(&self.b) {
                *power += f64::from(value.norm_sqr());
            }
        }
        self.power
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map_or(self.lags, |(index, _)| index)
    }

    fn envelope_peak(&mut self, reference: &[Complex<f32>], lane: &[Complex<f32>]) -> usize {
        let span = self.span();
        let frame = &reference[self.lags..self.lags + self.frame];
        envelope_spectrum(&mut self.fft, &mut self.a, frame);
        envelope_spectrum(&mut self.fft, &mut self.b, &lane[..span]);
        for (value, reference) in self.b.iter_mut().zip(&self.a) {
            *value *= reference.conj();
        }
        self.fft.inverse(&mut self.b);
        self.b[..self.power.len()]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.re.total_cmp(&b.1.re))
            .map_or(self.lags, |(index, _)| index)
    }

    fn product_offset(
        &mut self,
        reference: &[Complex<f32>],
        lane: &[Complex<f32>],
        index: usize,
        reach_cycles: f64,
    ) -> f64 {
        let frame = self.frame;
        self.b.fill(Complex::default());
        for ((value, sample), reference) in self
            .b
            .iter_mut()
            .zip(&lane[index..index + frame])
            .zip(&reference[self.lags..self.lags + frame])
        {
            *value = sample * reference.conj();
        }
        self.fft.forward(&mut self.b);
        let size = self.b.len() as isize;
        let reach = ((reach_cycles * size as f64).ceil() as isize).clamp(1, (size / 2 - 1).max(1));
        self.strongest_cycles(reach)
    }

    fn residual(
        &mut self,
        reference: &[Complex<f32>],
        lane: &[Complex<f32>],
        index: usize,
        cycles: f64,
    ) -> f64 {
        let frame = self.frame;
        self.b.fill(Complex::default());
        derotate(
            &lane[index..index + frame],
            index,
            cycles,
            &mut self.b[..frame],
        );
        for (value, reference) in self.b[..frame]
            .iter_mut()
            .zip(&reference[self.lags..self.lags + frame])
        {
            *value *= reference.conj();
        }
        self.fft.forward(&mut self.b);
        let size = self.b.len() as isize;
        self.strongest_cycles((size / SEGMENT.min(frame) as isize).max(1))
    }

    /// Strongest bin of the spectrum in `b` within `reach` bins of DC, interpolated, in
    /// cycles per sample.
    fn strongest_cycles(&self, reach: isize) -> f64 {
        let size = self.b.len() as isize;
        let magnitude = |bin: isize| f64::from(self.b[bin.rem_euclid(size) as usize].norm());
        let best = (-reach..=reach)
            .max_by(|a, b| magnitude(*a).total_cmp(&magnitude(*b)))
            .unwrap_or(0);
        let fraction = parabolic_peak(magnitude(best - 1), magnitude(best), magnitude(best + 1));
        (best as f64 + fraction) / size as f64
    }
}

/// Spectrum of the power envelope of `samples`, its mean removed, zero padded to `buffer`.
fn envelope_spectrum(fft: &mut FftPair, buffer: &mut [Complex<f32>], samples: &[Complex<f32>]) {
    let mean = mean_power(samples);
    buffer.fill(Complex::default());
    for (value, sample) in buffer.iter_mut().zip(samples) {
        *value = Complex::new(sample.norm_sqr() - mean, 0.0);
    }
    fft.forward(buffer);
}
