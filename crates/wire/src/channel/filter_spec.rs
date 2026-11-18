use serde::{Deserialize, Serialize};

/// The filter between a channel's downconverter and its decoder, as the decoder asks for it.
/// Edges are offsets from the channel frequency in Hz; the runtime designs the taps at the rate
/// the filter runs at, the decoder's input rate times any decimation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FilterSpec {
    /// No filter: the decoder takes the converter's output as it is.
    Passthrough,
    /// A lowpass centred on the channel. One that decimates takes over the converter's last
    /// step, so the converter runs at the decoder's rate times `decimation`.
    Lowpass {
        taps: usize,
        cutoff_hz: f64,
        #[serde(default = "one")]
        decimation: usize,
        /// A Kaiser window reaching this rejection, in place of the Blackman window.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stopband_db: Option<f64>,
    },
    /// A lowpass shifted onto `low_hz..high_hz`, for a passband off the channel frequency.
    Band {
        taps: usize,
        low_hz: f64,
        high_hz: f64,
    },
}

const fn one() -> usize {
    1
}

impl FilterSpec {
    /// A Blackman lowpass passing `cutoff_hz` either side of the channel frequency.
    #[must_use]
    pub const fn lowpass(taps: usize, cutoff_hz: f64) -> Self {
        Self::Lowpass {
            taps,
            cutoff_hz,
            decimation: 1,
            stopband_db: None,
        }
    }

    /// The 129-tap lowpass most decoders select their channel with.
    #[must_use]
    pub const fn channel(cutoff_hz: f64) -> Self {
        Self::lowpass(129, cutoff_hz)
    }

    #[must_use]
    pub const fn band(taps: usize, low_hz: f64, high_hz: f64) -> Self {
        Self::Band {
            taps,
            low_hz,
            high_hz,
        }
    }

    /// How far the filter brings the rate down.
    #[must_use]
    pub const fn decimation(&self) -> usize {
        match self {
            Self::Lowpass { decimation, .. } => *decimation,
            Self::Passthrough | Self::Band { .. } => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spec_reads_back_from_json_with_its_defaults() {
        let spec: FilterSpec =
            serde_json::from_str(r#"{"type":"lowpass","taps":65,"cutoff_hz":5000.0}"#)
                .expect("parses");
        assert_eq!(spec, FilterSpec::lowpass(65, 5_000.0));
        assert_eq!(spec.decimation(), 1);
        let band = FilterSpec::band(129, 300.0, 2_700.0);
        let json = serde_json::to_string(&band).expect("serializes");
        assert_eq!(serde_json::from_str::<FilterSpec>(&json).ok(), Some(band));
        let none: FilterSpec = serde_json::from_str(r#"{"type":"passthrough"}"#).expect("parses");
        assert_eq!(none, FilterSpec::Passthrough);
    }
}
