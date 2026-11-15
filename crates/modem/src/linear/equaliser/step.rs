use num_complex::Complex;

const STEP_DECAY: f32 = 0.98;

const STEP_GROWTH: f32 = 2.0;

const STEP_MEMORY: f32 = 0.99;

const MIN_STEP_FRACTION: f32 = 1.0 / 16.0;

/// A variable step that grows while successive errors correlate and decays once they do not.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct StepControl {
    step: Option<f32>,
    correlation: Complex<f32>,
    last: Complex<f32>,
}

impl StepControl {
    pub(super) fn next(&mut self, error: Complex<f32>, max: f32) -> f32 {
        self.correlation =
            self.correlation * STEP_MEMORY + error * self.last.conj() * (1.0 - STEP_MEMORY);
        self.last = error;
        let grown =
            STEP_DECAY * self.step.unwrap_or(max) + STEP_GROWTH * self.correlation.norm_sqr();
        let step = grown.clamp(max * MIN_STEP_FRACTION, max);
        self.step = Some(step);
        step
    }
}
