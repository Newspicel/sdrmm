use serde::{Deserialize, Serialize};

use sdrmm_wire::labelled;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DvbtBandwidth {
    Khz250,
    Khz333,
    Khz500,
    Mhz1,
    Mhz1_7,
    Mhz2,
    Mhz5,
    Mhz10,
    Mhz6,
    Mhz7,
    #[default]
    Mhz8,
}

impl DvbtBandwidth {
    pub const fn hz(self) -> f64 {
        match self {
            Self::Khz250 => 250_000.0,
            Self::Khz333 => 333_000.0,
            Self::Khz500 => 500_000.0,
            Self::Mhz1 => 1_000_000.0,
            Self::Mhz1_7 => 1_700_000.0,
            Self::Mhz2 => 2_000_000.0,
            Self::Mhz5 => 5_000_000.0,
            Self::Mhz10 => 10_000_000.0,
            Self::Mhz6 => 6_000_000.0,
            Self::Mhz7 => 7_000_000.0,
            Self::Mhz8 => 8_000_000.0,
        }
    }

    pub const fn sample_rate_hz(self) -> f64 {
        self.hz() * 8.0 / 7.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DvbtStandard {
    #[default]
    DvbT,
    DvbT2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DvbtParams {
    #[serde(default)]
    pub standard: DvbtStandard,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plp: Option<u8>,
    #[serde(default)]
    pub bandwidth: DvbtBandwidth,
    #[serde(default)]
    pub low_priority: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<u16>,
}

impl DvbtParams {
    pub const fn sample_rate_hz(self) -> f64 {
        if matches!(self.standard, DvbtStandard::DvbT2)
            && matches!(self.bandwidth, DvbtBandwidth::Mhz1_7)
        {
            131_000_000.0 / 71.0
        } else {
            self.bandwidth.sample_rate_hz()
        }
    }
}

labelled! {
    DvbtStandard { DvbT => "DVB-T", DvbT2 => "DVB-T2" }
}

labelled! {
    DvbtBandwidth {
        Khz250 => "250 kHz", Khz333 => "333 kHz", Khz500 => "500 kHz", Mhz1 => "1 MHz",
        Mhz1_7 => "1.7 MHz", Mhz2 => "2 MHz", Mhz5 => "5 MHz", Mhz6 => "6 MHz", Mhz7 => "7 MHz",
        Mhz8 => "8 MHz", Mhz10 => "10 MHz",
    }
}

impl sdrmm_wire::DecoderSettings for DvbtParams {
    const TYPE_ID: &'static str = "dvbt";
}

#[cfg(test)]
mod model_tests {
    use super::*;

    #[test]
    fn dvbt_bandwidths_roundtrip_with_native_clocks() {
        for (bandwidth, json, hz) in [
            (DvbtBandwidth::Khz250, "khz250", 250_000.0),
            (DvbtBandwidth::Khz333, "khz333", 333_000.0),
            (DvbtBandwidth::Khz500, "khz500", 500_000.0),
            (DvbtBandwidth::Mhz1, "mhz1", 1_000_000.0),
            (DvbtBandwidth::Mhz1_7, "mhz1_7", 1_700_000.0),
            (DvbtBandwidth::Mhz2, "mhz2", 2_000_000.0),
            (DvbtBandwidth::Mhz5, "mhz5", 5_000_000.0),
            (DvbtBandwidth::Mhz10, "mhz10", 10_000_000.0),
            (DvbtBandwidth::Mhz6, "mhz6", 6_000_000.0),
            (DvbtBandwidth::Mhz7, "mhz7", 7_000_000.0),
            (DvbtBandwidth::Mhz8, "mhz8", 8_000_000.0),
        ] {
            let encoded = serde_json::to_value(bandwidth).unwrap();
            assert_eq!(encoded, json);
            assert_eq!(
                serde_json::from_value::<DvbtBandwidth>(encoded).unwrap(),
                bandwidth
            );
            assert_eq!(bandwidth.hz(), hz);
            assert_eq!(bandwidth.sample_rate_hz(), hz * 8.0 / 7.0);
        }
        assert_eq!(
            serde_json::from_str::<DvbtParams>("{}").unwrap(),
            DvbtParams::default()
        );
    }

    #[test]
    fn terrestrial_standard_selects_the_correct_narrow_clock() {
        let params: DvbtParams =
            serde_json::from_str(r#"{"standard":"dvb_t2","bandwidth":"mhz1_7","plp":255}"#)
                .unwrap();
        assert_eq!(params.standard, DvbtStandard::DvbT2);
        assert_eq!(params.plp, Some(255));
        assert_eq!(params.sample_rate_hz(), 131_000_000.0 / 71.0);
        let legacy: DvbtParams = serde_json::from_str(r#"{"bandwidth":"mhz1_7"}"#).unwrap();
        assert_eq!(legacy.standard, DvbtStandard::DvbT);
        assert_eq!(legacy.sample_rate_hz(), 1_700_000.0 * 8.0 / 7.0);
        assert!(serde_json::from_str::<DvbtParams>(r#"{"plp":256}"#).is_err());
    }
}
