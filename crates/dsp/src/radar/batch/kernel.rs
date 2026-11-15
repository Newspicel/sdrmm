use num_complex::Complex;

use super::BatchShape;
use crate::math::fft::FftPair;
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
    doppler: FftPair,
    work: Vec<C32>,
    kernel: Vec<C32>,
}

impl BatchKernel {
    #[must_use]
    pub fn new(shape: BatchShape) -> Self {
        Self {
            shape,
            fft: FftPair::new(shape.fft_len),
            doppler: FftPair::new(shape.batches),
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
            for (value, r) in model.iter_mut().zip(spectrum.iter()) {
                *value *= r.conj();
            }
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
        for (value, r) in product.iter_mut().zip(spectrum) {
            *value *= r.conj();
        }
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
            for (((out, q), w), p) in self
                .work
                .iter_mut()
                .zip(product)
                .zip(&self.kernel)
                .zip(model)
            {
                *out = q - w * p;
            }
        } else {
            self.work.copy_from_slice(&product[..m]);
        }
        self.fft.inverse_scaled(&mut self.work);
        let lead = shape.lead;
        gates[..shape.gates].copy_from_slice(&self.work[lead..lead + shape.gates]);
        Ok(())
    }

    pub fn doppler(&mut self, series: &mut [C32], window: &[f32]) -> Result<(), RadarDspError> {
        let batches = self.shape.batches;
        if series.len() < batches || window.len() < batches {
            return Err(RadarDspError::Shape);
        }
        let series = &mut series[..batches];
        for (value, weight) in series.iter_mut().zip(window) {
            *value *= *weight;
        }
        self.doppler.forward(series);
        series.rotate_right(batches / 2);
        Ok(())
    }

    fn fits(&self, window: &[C32], batch: usize) -> bool {
        batch < self.shape.batches && window.len() >= self.shape.window()
    }
}

fn load(buffer: &mut [C32], samples: &[C32]) {
    let (head, tail) = buffer.split_at_mut(samples.len());
    head.copy_from_slice(samples);
    tail.fill(C32::default());
}
