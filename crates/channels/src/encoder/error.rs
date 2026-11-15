use std::fmt;

use sdrmm_modem::keying::QueueFull;

use super::TxPayload;

/// What a payload carries, to name what an encoder takes and what it was handed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Carries {
    Audio,
    Frames,
}

impl Carries {
    const fn other(self) -> Self {
        match self {
            Self::Audio => Self::Frames,
            Self::Frames => Self::Audio,
        }
    }
}

impl fmt::Display for Carries {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Audio => "audio",
            Self::Frames => "frames",
        })
    }
}

/// Why an encoder could not be found, opened, fed or installed.
#[derive(Debug, thiserror::Error)]
pub enum EncoderError {
    #[error("no encoder named {0}")]
    Unknown(String),
    #[error("{encoder} encoder was handed {got} settings")]
    WrongType { encoder: String, got: String },
    #[error("{encoder} settings: {reason}")]
    Settings { encoder: String, reason: String },
    #[error("{encoder} sends {takes}, not {}", .takes.other())]
    WrongPayload { encoder: String, takes: Carries },
    #[error("{0} sends no messages")]
    NoMessages(String),
    #[error("{encoder} message: {reason}")]
    Message { encoder: String, reason: String },
    #[error(transparent)]
    Backlog(#[from] QueueFull),
    #[error("{encoder} plugin refused: {reason}")]
    Refused { encoder: String, reason: String },
    #[error("{encoder} plugin stopped: {reason}")]
    Stopped { encoder: String, reason: String },
    #[error("encoder {0} is already built in")]
    BuiltIn(String),
    #[error("encoder {0} is loaded twice")]
    LoadedTwice(String),
}

impl EncoderError {
    pub fn settings(encoder: &str, reason: impl Into<String>) -> Self {
        Self::Settings {
            encoder: encoder.to_owned(),
            reason: reason.into(),
        }
    }

    pub fn message(encoder: &str, reason: impl Into<String>) -> Self {
        Self::Message {
            encoder: encoder.to_owned(),
            reason: reason.into(),
        }
    }
}

impl TxPayload {
    #[must_use]
    pub const fn carries(&self) -> Carries {
        match self {
            Self::Audio(_) => Carries::Audio,
            Self::Frame(_) => Carries::Frames,
        }
    }

    /// The samples, or why `encoder`, which sends audio, refuses a frame.
    pub fn audio(self, encoder: &str) -> Result<Vec<f32>, EncoderError> {
        match self {
            Self::Audio(pcm) => Ok(pcm),
            Self::Frame(_) => Err(wrong(encoder, Carries::Audio)),
        }
    }

    /// The octets, or why `encoder`, which sends frames, refuses audio.
    pub fn frame(self, encoder: &str) -> Result<Vec<u8>, EncoderError> {
        match self {
            Self::Frame(octets) => Ok(octets),
            Self::Audio(_) => Err(wrong(encoder, Carries::Frames)),
        }
    }
}

fn wrong(encoder: &str, takes: Carries) -> EncoderError {
    EncoderError::WrongPayload {
        encoder: encoder.to_owned(),
        takes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_payload_names_what_the_encoder_sends() {
        let refused = TxPayload::Frame(vec![1]).audio("nfm");
        assert_eq!(
            refused.err().map(|error| error.to_string()).as_deref(),
            Some("nfm sends audio, not frames")
        );
        let refused = TxPayload::Audio(vec![0.0]).frame("aprs");
        assert_eq!(
            refused.err().map(|error| error.to_string()).as_deref(),
            Some("aprs sends frames, not audio")
        );
        assert!(matches!(TxPayload::Frame(vec![7]).frame("aprs"), Ok(octets) if octets == [7]));
    }
}
