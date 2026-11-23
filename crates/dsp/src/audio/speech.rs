//! Speech to text with a Parakeet TDT transducer, the model handed in as bytes.

mod attention;
mod conformer;
mod matmul;
mod mel;
mod model;
mod rate;
mod subsample;
mod tensor;
mod transducer;
mod vocab;

#[cfg(test)]
mod tests;

pub use model::{Parakeet, SAMPLE_RATE_HZ};
pub use rate::to_model_rate;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SpeechError {
    #[error("speech model: {0}")]
    Model(String),
    #[error("speech audio: {0}")]
    Audio(String),
}

impl From<crate::math::neural::NetError> for SpeechError {
    fn from(error: crate::math::neural::NetError) -> Self {
        Self::Model(error.to_string())
    }
}
