use serde::{Deserialize, Serialize};

use sdrmm_wire::labelled;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IridiumSpan {
    #[default]
    Channel,
    Mhz1,
    Mhz2_5,
    Mhz5,
    Mhz10,
}

impl IridiumSpan {
    #[must_use]
    pub fn sample_rate_hz(self) -> Option<f64> {
        match self {
            Self::Channel => None,
            Self::Mhz1 => Some(1_000_000.0),
            Self::Mhz2_5 => Some(2_500_000.0),
            Self::Mhz5 => Some(5_000_000.0),
            Self::Mhz10 => Some(10_000_000.0),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct IridiumParams {
    #[serde(default)]
    pub span: IridiumSpan,
}

labelled! {
    IridiumSpan {
        Channel => "50 kHz": "One channel",
        Mhz1 => "1 MHz": "Radio at 1 MS/s, decodes the middle 800 kHz",
        Mhz2_5 => "2.5 MHz": "Radio at 2.5 MS/s, decodes the middle 2 MHz",
        Mhz5 => "5 MHz": "Radio at 5 MS/s, decodes the middle 4 MHz",
        Mhz10 => "10 MHz": "Radio at 10 MS/s, decodes the middle 8 MHz",
    }
}

impl sdrmm_wire::DecoderSettings for IridiumParams {
    const TYPE_ID: &'static str = "iridium";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::ChannelParams;

    #[test]
    fn iridium_span_defaults_to_one_channel_and_roundtrips() {
        assert_eq!(
            serde_json::from_str::<IridiumParams>("{}").unwrap(),
            IridiumParams::default()
        );
        let legacy: ChannelParams =
            serde_json::from_str(r#"{"type":"iridium","settings":{}}"#).unwrap();
        assert_eq!(
            legacy.parse::<IridiumParams>(),
            Ok(IridiumParams::default())
        );
        for (span, json, rate) in [
            (IridiumSpan::Channel, "channel", None),
            (IridiumSpan::Mhz1, "mhz1", Some(1_000_000.0)),
            (IridiumSpan::Mhz2_5, "mhz2_5", Some(2_500_000.0)),
            (IridiumSpan::Mhz5, "mhz5", Some(5_000_000.0)),
            (IridiumSpan::Mhz10, "mhz10", Some(10_000_000.0)),
        ] {
            let encoded = serde_json::to_value(span).unwrap();
            assert_eq!(encoded, json);
            assert_eq!(
                serde_json::from_value::<IridiumSpan>(encoded).unwrap(),
                span
            );
            assert_eq!(span.sample_rate_hz(), rate);
        }
    }
}
