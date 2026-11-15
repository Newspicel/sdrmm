use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EqualiserConfig {
    pub span_symbols: usize,
    pub feedback_symbols: usize,
    pub blind_step: f32,
    pub decision_step: f32,
    pub burst_passes: usize,
}

impl EqualiserConfig {
    pub const DEFAULT: Self = Self {
        span_symbols: 8,
        feedback_symbols: 0,
        blind_step: 0.2,
        decision_step: 0.05,
        burst_passes: 3,
    };

    #[must_use]
    pub const fn with_feedback(mut self, symbols: usize) -> Self {
        self.feedback_symbols = symbols;
        self
    }

    #[must_use]
    pub const fn with_span(mut self, symbols: usize) -> Self {
        self.span_symbols = symbols;
        self
    }

    pub(super) fn validate(&self) -> Result<(), EqualiserError> {
        if self.span_symbols == 0 {
            return Err(EqualiserError::NoSpan);
        }
        if self.burst_passes == 0 {
            return Err(EqualiserError::NoPasses);
        }
        for step in [self.blind_step, self.decision_step] {
            if !(step > 0.0 && step < 1.0) {
                return Err(EqualiserError::StepOutOfRange(step));
            }
        }
        Ok(())
    }
}

impl Default for EqualiserConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EqualiserError {
    NoSpan,
    NoPasses,
    StepOutOfRange(f32),
}

impl fmt::Display for EqualiserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSpan => write!(f, "equaliser needs a span of at least one symbol"),
            Self::NoPasses => write!(f, "burst equalisation needs at least one pass"),
            Self::StepOutOfRange(step) => write!(f, "equaliser step {step} is outside (0, 1)"),
        }
    }
}

impl std::error::Error for EqualiserError {}
