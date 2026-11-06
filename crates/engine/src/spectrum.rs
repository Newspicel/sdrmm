#[cfg(feature = "gpu-fft")]
use std::sync::Arc;

use num_complex::Complex;
use sdrmm_dsp::blackman_harris;

pub(crate) use cpu::CpuSpectrum;

mod cpu;
mod history;

#[cfg(any(feature = "gpu-fft", test))]
const GPU_MIN_FFT_SIZE: usize = 32_768;
const TARGET_FPS: f64 = 30.0;
const MAX_AVERAGES: u32 = 64;
/// Transformed samples per second the DSP thread spends on the display. At 245.76 MS/s this
/// keeps the 16 averages a frame the CPU path always had; slower rates get overlap for free.
const CPU_BUDGET: f64 = 128e6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SpectrumFrame {
    pub(crate) timestamp: u64,
    pub(crate) center_hz: f64,
    pub(crate) span_hz: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tuning {
    pub(crate) center_hz: f64,
    pub(crate) span_hz: f32,
}

impl Tuning {
    pub(crate) const fn frame(self, timestamp: u64) -> SpectrumFrame {
        SpectrumFrame {
            timestamp,
            center_hz: self.center_hz,
            span_hz: self.span_hz,
        }
    }
}

/// How far apart transforms start and how many of them average into one published frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cadence {
    pub(crate) hop: usize,
    pub(crate) averages: u32,
}

impl Cadence {
    /// Overlaps transforms as far as `budget` transformed samples per second allow, never closer
    /// than a quarter window, and averages what fits one frame period.
    pub(crate) fn new(sample_rate: f64, size: usize, budget: f64) -> Self {
        let per_frame = (sample_rate / TARGET_FPS).max(1.0);
        let affordable = sample_rate * size as f64 / budget.max(1.0);
        let hop = (per_frame / f64::from(MAX_AVERAGES))
            .max(affordable)
            .max((size / 4) as f64)
            .ceil()
            .max(1.0) as usize;
        let averages = (per_frame / hop as f64).round() as u32;
        Self {
            hop,
            averages: averages.clamp(1, MAX_AVERAGES),
        }
    }
}

pub(crate) struct SpectrumPlan {
    size: usize,
    #[cfg(feature = "gpu-fft")]
    gpu: Option<(Arc<gpu::Context>, f64)>,
}

impl SpectrumPlan {
    pub(crate) fn new(size: usize, lanes: usize) -> Self {
        #[cfg(feature = "gpu-fft")]
        {
            let gpu = should_use_gpu(size, lanes)
                .then(|| gpu_context(size, lanes))
                .flatten();
            Self { size, gpu }
        }

        #[cfg(not(feature = "gpu-fft"))]
        {
            let _ = lanes;
            Self { size }
        }
    }

    pub(crate) const fn size(&self) -> usize {
        self.size
    }

    pub(crate) fn analyzer(&self) -> SpectrumAnalyzer {
        #[cfg(feature = "gpu-fft")]
        if let Some((context, budget)) = &self.gpu {
            match gpu::spectrum::Stream::new(context.clone(), self.size) {
                Ok(stream) => {
                    return SpectrumAnalyzer::Gpu {
                        stream: Some(Box::new(stream)),
                        budget: *budget,
                        fallback: CpuSpectrum::new(self.size),
                    };
                }
                Err(error) => {
                    tracing::warn!(%error, "could not start the GPU spectrum; using CPU");
                }
            }
        }

        SpectrumAnalyzer::Cpu(CpuSpectrum::new(self.size))
    }
}

#[cfg(feature = "gpu-fft")]
fn gpu_context(size: usize, lanes: usize) -> Option<(Arc<gpu::Context>, f64)> {
    let context = gpu::context()
        .inspect_err(|error| tracing::warn!(%error, "GPU spectrum unavailable; using CPU"))
        .ok()?;
    let budget = gpu::spectrum::budget(context.clone(), size)?;
    tracing::info!(
        adapter = context.adapter_name(),
        fft_size = size,
        lanes,
        "using GPU spectrum"
    );
    Some((context.clone(), budget))
}

pub(crate) enum SpectrumAnalyzer {
    Cpu(CpuSpectrum),
    #[cfg(feature = "gpu-fft")]
    Gpu {
        stream: Option<Box<gpu::spectrum::Stream>>,
        budget: f64,
        fallback: CpuSpectrum,
    },
}

impl SpectrumAnalyzer {
    pub(crate) fn cadence(&self, sample_rate: f64, size: usize) -> Cadence {
        match self {
            Self::Cpu(_) => Cadence::new(sample_rate, size, CPU_BUDGET),
            #[cfg(feature = "gpu-fft")]
            Self::Gpu { stream, budget, .. } => {
                let budget = if stream.is_some() {
                    *budget
                } else {
                    CPU_BUDGET
                };
                Cadence::new(sample_rate, size, budget)
            }
        }
    }

    pub(crate) fn push(
        &mut self,
        samples: &[Complex<f32>],
        index: u64,
        tuning: Tuning,
        cadence: Cadence,
        publish: &mut impl FnMut(SpectrumFrame, &[f32]),
    ) {
        match self {
            Self::Cpu(spectrum) => spectrum.push(samples, index, tuning, cadence, publish),
            #[cfg(feature = "gpu-fft")]
            Self::Gpu {
                stream, fallback, ..
            } => {
                let Some(gpu) = stream else {
                    return fallback.push(samples, index, tuning, cadence, publish);
                };
                if let Err(error) = gpu.push(samples, index, tuning, cadence, publish) {
                    tracing::error!(%error, "GPU spectrum failed; switching to CPU");
                    *stream = None;
                    fallback.push(samples, index, tuning, cadence, publish);
                }
            }
        }
    }

    pub(crate) fn restart(&mut self, publish: &mut impl FnMut(SpectrumFrame, &[f32])) {
        match self {
            Self::Cpu(spectrum) => spectrum.restart(publish),
            #[cfg(feature = "gpu-fft")]
            Self::Gpu {
                stream, fallback, ..
            } => match stream {
                Some(gpu) => gpu.restart(),
                None => fallback.restart(publish),
            },
        }
    }

    pub(crate) fn idle(
        &mut self,
        cadence: Cadence,
        publish: &mut impl FnMut(SpectrumFrame, &[f32]),
    ) {
        match self {
            Self::Cpu(spectrum) => spectrum.idle(cadence, publish),
            #[cfg(feature = "gpu-fft")]
            Self::Gpu {
                stream, fallback, ..
            } => {
                let Some(gpu) = stream else {
                    return fallback.idle(cadence, publish);
                };
                if let Err(error) = gpu.idle(publish) {
                    tracing::error!(%error, "GPU spectrum failed; switching to CPU");
                    *stream = None;
                }
            }
        }
    }
}

pub(crate) fn display_window(size: usize) -> Vec<f32> {
    blackman_harris(size)
}

pub(crate) fn cpu_analyzer(size: usize) -> sdrmm_dsp::SpectrumAnalyzer {
    sdrmm_dsp::SpectrumAnalyzer::with_window(display_window(size))
}

#[cfg(any(feature = "gpu-fft", test))]
fn should_use_gpu(size: usize, _lanes: usize) -> bool {
    size.is_power_of_two() && size >= GPU_MIN_FFT_SIZE
}

#[cfg(feature = "gpu-fft")]
use crate::gpu;

#[cfg(test)]
mod tests;
