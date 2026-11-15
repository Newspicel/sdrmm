use num_complex::Complex;

use super::check_canceller;
use super::delay_line::DelayLine;
use crate::math::fft::FftPair;
use crate::radar::RadarDspError;
use crate::radar::ratio::WindowRatio;

type C32 = Complex<f32>;

const POWER_FLOOR: f32 = 1e-9;
const POWER_MEMORY: f32 = 0.9;
const POWER_LOADING: f32 = 1e-2;

/// Frequency-domain block NLMS: the same canceller as `SurveillanceNlms`, one FFT block at a time.
pub struct BlockNlms {
    lead: usize,
    block: usize,
    step: f32,
    fft: FftPair,
    weights: Vec<C32>,
    power: Vec<f32>,
    previous: Vec<C32>,
    reference: Vec<C32>,
    desired: Vec<C32>,
    output: Vec<C32>,
    spectrum: Vec<C32>,
    work: Vec<C32>,
    fill: usize,
    delay: DelayLine,
    meter: WindowRatio,
    resets: u64,
}

impl BlockNlms {
    pub fn new(
        taps: usize,
        lead: usize,
        step: f32,
        sample_rate: f64,
    ) -> Result<Self, RadarDspError> {
        let order = check_canceller(taps, lead, step)?;
        let block = order.next_power_of_two();
        let full = 2 * block;
        Ok(Self {
            lead,
            block,
            step,
            fft: FftPair::new(full),
            weights: vec![C32::default(); full],
            power: vec![0.0; full],
            previous: vec![C32::default(); block],
            reference: vec![C32::default(); block],
            desired: vec![C32::default(); block],
            output: vec![C32::default(); block],
            spectrum: vec![C32::default(); full],
            work: vec![C32::default(); full],
            fill: 0,
            delay: DelayLine::new(lead),
            meter: WindowRatio::new(sample_rate)?,
            resets: 0,
        })
    }

    #[must_use]
    pub const fn latency(&self) -> usize {
        self.lead + self.block
    }

    #[must_use]
    pub const fn block(&self) -> usize {
        self.block
    }

    pub fn process(&mut self, reference: &[C32], surveillance: &[C32], out: &mut [C32]) -> usize {
        let len = reference.len().min(surveillance.len()).min(out.len());
        for ((&r, &s), e) in reference
            .iter()
            .zip(surveillance)
            .zip(out.iter_mut())
            .take(len)
        {
            *e = self.output[self.fill];
            self.reference[self.fill] = r;
            self.desired[self.fill] = self.delay.push(s);
            self.fill += 1;
            if self.fill == self.block {
                self.run_block();
                self.fill = 0;
            }
        }
        len
    }

    #[must_use]
    pub fn suppression_db(&self) -> f32 {
        self.meter.value_db()
    }

    #[must_use]
    pub const fn resets(&self) -> u64 {
        self.resets
    }

    pub fn reset(&mut self) {
        for buffer in [
            &mut self.weights,
            &mut self.previous,
            &mut self.reference,
            &mut self.desired,
            &mut self.output,
        ] {
            buffer.fill(C32::default());
        }
        self.power.fill(0.0);
        self.fill = 0;
        self.delay.clear();
        self.meter.clear();
    }

    fn run_block(&mut self) {
        let block = self.block;
        self.spectrum[..block].copy_from_slice(&self.previous);
        self.spectrum[block..].copy_from_slice(&self.reference);
        self.fft.forward(&mut self.spectrum);
        for ((out, x), w) in self.work.iter_mut().zip(&self.spectrum).zip(&self.weights) {
            *out = x * w;
        }
        self.fft.inverse_scaled(&mut self.work);
        let mut desired_energy = 0.0f32;
        let mut error_energy = 0.0f32;
        for ((e, d), y) in self
            .output
            .iter_mut()
            .zip(&self.desired)
            .zip(&self.work[block..])
        {
            *e = d - y;
            desired_energy += d.norm_sqr();
            error_energy += e.norm_sqr();
        }
        self.work[..block].fill(C32::default());
        self.work[block..].copy_from_slice(&self.output);
        self.fft.forward(&mut self.work);
        let mut total = 0.0f32;
        for (power, x) in self.power.iter_mut().zip(&self.spectrum) {
            *power = POWER_MEMORY * *power + (1.0 - POWER_MEMORY) * x.norm_sqr();
            total += *power;
        }
        let floor = POWER_FLOOR + POWER_LOADING * total / self.power.len() as f32;
        for ((value, x), power) in self.work.iter_mut().zip(&self.spectrum).zip(&self.power) {
            *value = x.conj() * *value / (*power + floor);
        }
        self.fft.inverse_scaled(&mut self.work);
        self.work[block..].fill(C32::default());
        self.fft.forward(&mut self.work);
        let mut energy = 0.0f32;
        for (w, gradient) in self.weights.iter_mut().zip(&self.work) {
            *w += gradient * self.step;
            energy += w.norm_sqr();
        }
        std::mem::swap(&mut self.previous, &mut self.reference);
        if energy.is_finite() && error_energy.is_finite() {
            self.meter.add(desired_energy, error_energy, self.block);
        } else {
            self.weights.fill(C32::default());
            self.power.fill(0.0);
            self.previous.fill(C32::default());
            self.output.fill(C32::default());
            self.meter.clear();
            self.resets += 1;
        }
    }
}
