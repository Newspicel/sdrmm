use num_complex::Complex;

use super::BatchShape;
use crate::math::complex::{mul_conj_in_place, scale_real_in_place, sub_mul_into};
use crate::math::fft::{FftPair, Transform};
use crate::radar::RadarDspError;
use crate::radar::wiener::WeightTable;

type C32 = Complex<f32>;

#[derive(Clone, Copy)]
pub struct WeightsAt<'a> {
    table: Option<&'a WeightTable>,
    lane: usize,
    batch: usize,
}

impl<'a> WeightsAt<'a> {
    #[must_use]
    pub const fn none() -> Self {
        Self {
            table: None,
            lane: 0,
            batch: 0,
        }
    }

    pub(crate) const fn of(table: &'a WeightTable, lane: usize, batch: usize) -> Self {
        Self {
            table: Some(table),
            lane,
            batch,
        }
    }
}

pub struct BatchKernel {
    shape: BatchShape,
    pub(super) fft: FftPair,
    doppler: Transform<f32>,
    work: Vec<C32>,
    kernel: Vec<C32>,
}

impl BatchKernel {
    #[must_use]
    pub fn new(shape: BatchShape) -> Self {
        Self {
            shape,
            fft: FftPair::new(shape.fft_len),
            doppler: Transform::forward(shape.batches),
            work: vec![C32::default(); shape.fft_len],
            kernel: vec![C32::default(); shape.fft_len],
        }
    }

    #[must_use]
    pub const fn shape(&self) -> BatchShape {
        self.shape
    }

    pub fn reference(
        &mut self,
        window: &[C32],
        batch: usize,
        spectrum: &mut [C32],
        model: &mut [C32],
    ) -> Result<(), RadarDspError> {
        let shape = self.shape;
        let m = shape.fft_len;
        if !self.fits(window, batch) || spectrum.len() < m || (shape.eca() && model.len() < m) {
            return Err(RadarDspError::Shape);
        }
        let spectrum = &mut spectrum[..m];
        let start = shape.pre() + batch * shape.batch_len;
        load(spectrum, &window[start..start + shape.batch_len]);
        self.fft.forward(spectrum);
        if shape.eca() {
            let model = &mut model[..m];
            let from = batch * shape.batch_len;
            let len = shape.batch_len + shape.span() + shape.order() - 2;
            load(model, &window[from..from + len]);
            self.fft.forward(model);
            mul_conj_in_place(model, spectrum);
        }
        Ok(())
    }

    pub fn surveillance(
        &mut self,
        window: &[C32],
        batch: usize,
        spectrum: &[C32],
        product: &mut [C32],
    ) -> Result<(), RadarDspError> {
        let shape = self.shape;
        let m = shape.fft_len;
        if !self.fits(window, batch) || spectrum.len() < m || product.len() < m {
            return Err(RadarDspError::Shape);
        }
        let product = &mut product[..m];
        let start = shape.pre() + batch * shape.batch_len - shape.lead;
        let len = shape.batch_len + shape.span() - 1;
        load(product, &window[start..start + len]);
        self.fft.forward(product);
        mul_conj_in_place(product, spectrum);
        Ok(())
    }

    pub fn residual(
        &mut self,
        product: &[C32],
        model: &[C32],
        weights: WeightsAt<'_>,
        gates: &mut [C32],
    ) -> Result<(), RadarDspError> {
        let shape = self.shape;
        let m = shape.fft_len;
        if product.len() < m || gates.len() < shape.gates {
            return Err(RadarDspError::Shape);
        }
        let combined = match weights.table {
            Some(table) if shape.eca() => {
                if model.len() < m {
                    return Err(RadarDspError::Shape);
                }
                table.combine(weights.lane, weights.batch, &mut self.kernel)?
            }
            _ => false,
        };
        if combined {
            sub_mul_into(&mut self.work, product, &self.kernel, model);
        } else {
            self.work.copy_from_slice(&product[..m]);
        }
        self.fft.inverse(&mut self.work);
        let lead = shape.lead;
        let inv = 1.0 / m as f32;
        scale_copy(
            &mut gates[..shape.gates],
            &self.work[lead..lead + shape.gates],
            inv,
        );
        Ok(())
    }

    pub fn doppler(&mut self, series: &mut [C32], window: &[f32]) -> Result<(), RadarDspError> {
        let batches = self.shape.batches;
        if series.len() < batches || window.len() < batches {
            return Err(RadarDspError::Shape);
        }
        let series = &mut series[..batches];
        scale_real_in_place(series, &window[..batches]);
        self.doppler.process(series);
        series.rotate_right(batches / 2);
        Ok(())
    }

    fn fits(&self, window: &[C32], batch: usize) -> bool {
        batch < self.shape.batches && window.len() >= self.shape.window()
    }
}

fn scale_copy(dst: &mut [C32], src: &[C32], factor: f32) {
    #[cfg(target_arch = "aarch64")]
    {
        use std::arch::aarch64::{vld1q_f32, vmulq_n_f32, vst1q_f32};
        let (dst_chunks, dst_tail) = dst.as_chunks_mut::<4>();
        let (src_chunks, src_tail) = src.as_chunks::<4>();
        unsafe {
            for (d, s) in dst_chunks.iter_mut().zip(src_chunks) {
                let sp = s.as_ptr().cast::<f32>();
                let dp = d.as_mut_ptr().cast::<f32>();
                vst1q_f32(dp, vmulq_n_f32(vld1q_f32(sp), factor));
                vst1q_f32(dp.add(4), vmulq_n_f32(vld1q_f32(sp.add(4)), factor));
            }
        }
        for (d, &s) in dst_tail.iter_mut().zip(src_tail) {
            *d = s * factor;
        }
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        for (d, &s) in dst.iter_mut().zip(src) {
            *d = s * factor;
        }
    }
}

fn load(buffer: &mut [C32], samples: &[C32]) {
    let (head, tail) = buffer.split_at_mut(samples.len());
    head.copy_from_slice(samples);
    tail.fill(C32::default());
}
