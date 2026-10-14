use sdrmm_device::DeviceError;
use sdrmm_wire::{ArgumentOption, ExtraSetting, ExtraValue, Range};

use crate::{
    apply::Write,
    iio::{Client, Context, Direction},
    layout::{RX_LO, TX_LO, available},
};

pub(crate) const RATE_GOVERNOR: &str = "rate_governor";
pub(crate) const CALIBRATION: &str = "calibration";
pub(crate) const STATE: &str = "ensm";
pub(crate) const RX_LO_EXTERNAL: &str = "rx_lo_external";
pub(crate) const TX_LO_EXTERNAL: &str = "tx_lo_external";
pub(crate) const AGC_HIGH: &str = "agc_high";
pub(crate) const AGC_LOW: &str = "agc_low";

const GOVERNOR_ATTR: &str = "trx_rate_governor";
const CALIBRATION_ATTR: &str = "calib_mode";
const STATE_ATTR: &str = "ensm_mode";
const EXTERNAL_ATTR: &str = "external";
pub(crate) const REGISTER_ATTR: &str = "direct_reg_access";

const AGC_HIGH_REGISTER: u16 = 0x101;
const AGC_LOW_REGISTER: u16 = 0x120;
const THRESHOLD_MASK: u8 = 0x7f;
const AGC_HIGH_RANGE: Range = Range {
    min: -40.0,
    max: -1.0,
    step: Some(1.0),
};
const AGC_LOW_RANGE: Range = Range {
    min: -60.0,
    max: -2.0,
    step: Some(1.0),
};

const GOVERNORS: [(&str, &str); 2] = [("nominal", "Nominal"), ("highest_osr", "Highest OSR")];
const CALIBRATIONS: [(&str, &str); 2] = [("auto", "Auto"), ("manual", "Manual")];
const STATES: [(&str, &str); 4] = [
    ("fdd", "Run"),
    ("alert", "Alert"),
    ("wait", "Wait"),
    ("sleep", "Sleep"),
];

const CHIP_GROUP: &str = "Chip";
const LO_GROUP: &str = "Ext LO";
const AGC_GROUP: &str = "AGC level";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Chip {
    pub(crate) governors: Vec<String>,
    pub(crate) calibrations: Vec<String>,
    pub(crate) states: Vec<String>,
    pub(crate) external_lo: bool,
    pub(crate) registers: bool,
}

impl Chip {
    pub(crate) fn read(client: &Client, context: &Context, phy: &str) -> Self {
        let Some(device) = context.device(phy) else {
            return Self::default();
        };
        let offered = |attr: &str, known: &[(&str, &str)]| -> Vec<String> {
            if !device.attributes.iter().any(|name| name == attr) {
                return Vec::new();
            }
            let listed = client
                .read_device_attr(phy, &available(attr))
                .inspect_err(|e| tracing::debug!("{phy}.{attr}: {e}"))
                .unwrap_or_default();
            known
                .iter()
                .filter(|(value, _)| listed.split_whitespace().any(|offer| offer == *value))
                .map(|(value, _)| (*value).to_string())
                .collect()
        };
        Self {
            governors: offered(GOVERNOR_ATTR, &GOVERNORS),
            calibrations: offered(CALIBRATION_ATTR, &CALIBRATIONS),
            states: offered(STATE_ATTR, &STATES),
            external_lo: [RX_LO, TX_LO].iter().all(|lo| {
                device
                    .channel(lo, true)
                    .is_some_and(|channel| channel.has(EXTERNAL_ATTR))
            }),
            registers: device
                .debug_attributes
                .iter()
                .any(|name| name == REGISTER_ATTR),
        }
    }

    pub(crate) fn settings(&self, agc: bool) -> Vec<ExtraSetting> {
        let mut extra = Vec::new();
        for (name, label, offered, known) in [
            (RATE_GOVERNOR, "Governor", &self.governors, &GOVERNORS[..]),
            (
                CALIBRATION,
                "Calibration",
                &self.calibrations,
                &CALIBRATIONS[..],
            ),
            (STATE, "State", &self.states, &STATES[..]),
        ] {
            if offered.len() > 1 {
                extra.push(choice(name, label, offered, known).in_group(CHIP_GROUP));
            }
        }
        if self.external_lo {
            extra.push(ExtraSetting::bool(RX_LO_EXTERNAL, "RX", false).in_group(LO_GROUP));
            extra.push(ExtraSetting::bool(TX_LO_EXTERNAL, "TX", false).in_group(LO_GROUP));
        }
        if agc && self.registers {
            extra.push(
                ExtraSetting::range(AGC_HIGH, "High", AGC_HIGH_RANGE, "dBFS").in_group(AGC_GROUP),
            );
            extra.push(
                ExtraSetting::range(AGC_LOW, "Low", AGC_LOW_RANGE, "dBFS").in_group(AGC_GROUP),
            );
        }
        extra
    }
}

fn choice(name: &str, label: &str, offered: &[String], known: &[(&str, &str)]) -> ExtraSetting {
    let options = known
        .iter()
        .filter(|(value, _)| offered.iter().any(|offer| offer == value))
        .map(|(value, label)| ArgumentOption {
            value: (*value).to_string(),
            label: Some((*label).to_string()),
        })
        .collect();
    ExtraSetting::choice(name, label, options, offered[0].clone())
}

pub(crate) fn owns(name: &str) -> bool {
    [
        RATE_GOVERNOR,
        CALIBRATION,
        STATE,
        RX_LO_EXTERNAL,
        TX_LO_EXTERNAL,
        AGC_HIGH,
        AGC_LOW,
    ]
    .contains(&name)
}

pub(crate) fn plan(
    extra: &ExtraValue,
    chosen: impl FnOnce() -> Result<String, DeviceError>,
) -> Result<Write, DeviceError> {
    let name = extra.name.as_str();
    Ok(match name {
        RATE_GOVERNOR => device_write(GOVERNOR_ATTR, chosen()?),
        CALIBRATION => device_write(CALIBRATION_ATTR, chosen()?),
        STATE => device_write(STATE_ATTR, chosen()?),
        RX_LO_EXTERNAL | TX_LO_EXTERNAL => Write::Channel {
            output: true,
            channel: if name == RX_LO_EXTERNAL { RX_LO } else { TX_LO }.to_string(),
            attr: EXTERNAL_ATTR.to_string(),
            value: u8::from(flag(extra)?).to_string(),
        },
        _ => threshold(extra)?,
    })
}

fn device_write(attr: &str, value: String) -> Write {
    Write::Device {
        attr: attr.to_string(),
        value,
    }
}

fn flag(extra: &ExtraValue) -> Result<bool, DeviceError> {
    extra
        .value
        .as_bool()
        .ok_or_else(|| DeviceError::Unsupported(format!("{} is on or off", extra.name)))
}

fn threshold(extra: &ExtraValue) -> Result<Write, DeviceError> {
    let (address, range) = match extra.name.as_str() {
        AGC_HIGH => (AGC_HIGH_REGISTER, AGC_HIGH_RANGE),
        _ => (AGC_LOW_REGISTER, AGC_LOW_RANGE),
    };
    let dbfs = extra
        .value
        .as_f64()
        .filter(|dbfs| range.holds(*dbfs))
        .ok_or_else(|| {
            DeviceError::Unsupported(format!(
                "{} takes {} to {} dBFS",
                extra.name, range.min, range.max
            ))
        })?;
    Ok(Write::Register {
        address,
        mask: THRESHOLD_MASK,
        value: (-dbfs).round() as u8,
    })
}

pub(crate) fn check_thresholds(high: Option<f64>, low: Option<f64>) -> Result<(), DeviceError> {
    match (high, low) {
        (Some(high), Some(low)) if low >= high => Err(DeviceError::Unsupported(format!(
            "the AGC low threshold ({low} dBFS) has to sit below the high one ({high} dBFS)"
        ))),
        _ => Ok(()),
    }
}

pub(crate) fn read(
    client: &Client,
    phy: &str,
    setting: &ExtraSetting,
) -> Option<serde_json::Value> {
    let device = |attr: &str| {
        client
            .read_device_attr(phy, attr)
            .inspect_err(|e| tracing::debug!("{phy}.{attr}: {e}"))
            .ok()
            .map(|value| serde_json::Value::String(value.trim().to_string()))
    };
    let external = |lo: &str| {
        client
            .read_channel_attr(phy, Direction::Out, lo, EXTERNAL_ATTR)
            .inspect_err(|e| tracing::debug!("{phy}.{lo}.{EXTERNAL_ATTR}: {e}"))
            .ok()
            .map(|value| serde_json::Value::Bool(value.trim() == "1"))
    };
    let known = |value: serde_json::Value| {
        let ExtraSetting::Enum { options, .. } = setting else {
            return None;
        };
        options
            .iter()
            .any(|option| Some(option.value.as_str()) == value.as_str())
            .then_some(value)
    };
    match setting.name() {
        RATE_GOVERNOR => device(GOVERNOR_ATTR).and_then(known),
        CALIBRATION => device(CALIBRATION_ATTR).and_then(known),
        STATE => device(STATE_ATTR).and_then(known),
        RX_LO_EXTERNAL => external(RX_LO),
        TX_LO_EXTERNAL => external(TX_LO),
        AGC_HIGH => register(client, phy, AGC_HIGH_REGISTER).map(dbfs),
        AGC_LOW => register(client, phy, AGC_LOW_REGISTER).map(dbfs),
        _ => None,
    }
}

fn dbfs(raw: u8) -> serde_json::Value {
    serde_json::Value::from(-f64::from(raw & THRESHOLD_MASK))
}

pub(crate) fn register(client: &Client, phy: &str, address: u16) -> Option<u8> {
    client
        .write_debug_attr(phy, REGISTER_ATTR, &format!("0x{address:x}"))
        .and_then(|()| client.read_debug_attr(phy, REGISTER_ATTR))
        .inspect_err(|e| tracing::debug!("{phy} register 0x{address:x}: {e}"))
        .ok()
        .and_then(|text| parse_register(&text))
}

pub(crate) fn parse_register(text: &str) -> Option<u8> {
    let text = text.trim();
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    u8::from_str_radix(digits, 16).ok()
}

pub(crate) fn write_register(
    client: &Client,
    phy: &str,
    address: u16,
    mask: u8,
    value: u8,
) -> Result<(), DeviceError> {
    let held = register(client, phy, address)
        .ok_or_else(|| DeviceError::Io(format!("register 0x{address:x} did not read back")))?;
    let next = (held & !mask) | (value & mask);
    client.write_debug_attr(phy, REGISTER_ATTR, &format!("0x{address:x} 0x{next:x}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn chip() -> Chip {
        Chip {
            governors: vec!["nominal".to_string(), "highest_osr".to_string()],
            calibrations: vec!["auto".to_string(), "manual".to_string()],
            states: ["fdd", "alert", "wait", "sleep"]
                .map(str::to_string)
                .to_vec(),
            external_lo: true,
            registers: true,
        }
    }

    fn value(name: &str, value: serde_json::Value) -> ExtraValue {
        ExtraValue {
            name: name.to_string(),
            value,
        }
    }

    #[test]
    fn every_chip_setting_lands_in_a_group_and_agc_needs_registers() {
        let settings = chip().settings(true);
        let names: Vec<&str> = settings.iter().map(ExtraSetting::name).collect();
        assert_eq!(
            names,
            vec![
                RATE_GOVERNOR,
                CALIBRATION,
                STATE,
                RX_LO_EXTERNAL,
                TX_LO_EXTERNAL,
                AGC_HIGH,
                AGC_LOW
            ]
        );
        assert!(settings.iter().all(|setting| setting.group().is_some()));
        let bare = Chip {
            registers: false,
            ..chip()
        };
        assert!(!bare.settings(true).iter().any(|s| s.name() == AGC_HIGH));
        assert!(!chip().settings(false).iter().any(|s| s.name() == AGC_LOW));
    }

    #[test]
    fn a_threshold_is_the_register_count_below_full_scale() {
        assert_eq!(
            plan(&value(AGC_HIGH, json!(-10.0)), || unreachable!()).expect("planned"),
            Write::Register {
                address: AGC_HIGH_REGISTER,
                mask: THRESHOLD_MASK,
                value: 10
            }
        );
        assert!(
            plan(&value(AGC_LOW, json!(-80.0)), || unreachable!())
                .expect_err("refused")
                .to_string()
                .contains("-60 to -2")
        );
        assert!(check_thresholds(Some(-12.0), Some(-10.0)).is_err());
        assert!(check_thresholds(Some(-10.0), Some(-12.0)).is_ok());
    }

    #[test]
    fn an_external_lo_is_a_switch_on_its_synthesizer() {
        assert_eq!(
            plan(&value(TX_LO_EXTERNAL, json!(true)), || unreachable!()).expect("planned"),
            Write::Channel {
                output: true,
                channel: TX_LO.to_string(),
                attr: EXTERNAL_ATTR.to_string(),
                value: "1".to_string()
            }
        );
    }

    #[test]
    fn a_register_reads_back_in_the_hex_the_debug_file_prints() {
        assert_eq!(parse_register("0xB\n"), Some(11));
        assert_eq!(parse_register("0x7F"), Some(127));
        assert_eq!(parse_register("nope"), None);
    }
}
