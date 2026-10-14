use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    decode::EventKind,
    present::{EventPayload, Field, Presentation, Unit, count, fields, number, repaired, text},
};

pub const WMBUS_SAMPLE_RATE_HZ: f64 = 400_000.0;
pub const WMBUS_BANDWIDTH_HZ: f64 = 300_000.0;
pub const WMBUS_T_C_HZ: f64 = 868_950_000.0;
pub const WMBUS_S_HZ: f64 = 868_300_000.0;
pub const MAX_WMBUS_KEYS: usize = 256;
const METER_DIGITS: usize = 8;
const KEY_DIGITS: usize = 32;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WmbusKey {
    pub meter: String,
    pub key: String,
}

impl WmbusKey {
    fn check(&self) -> Result<(), String> {
        let digits = |text: &str, len: usize| {
            text.len() == len && text.bytes().all(|byte| byte.is_ascii_hexdigit())
        };
        if !digits(&self.meter, METER_DIGITS) {
            return Err(format!(
                "a wM-Bus meter ID is {METER_DIGITS} digits, got {:?}",
                self.meter
            ));
        }
        if !digits(&self.key, KEY_DIGITS) {
            return Err(format!(
                "a wM-Bus key is {KEY_DIGITS} hex digits, meter {}",
                self.meter
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WmbusParams {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<WmbusKey>,
}

impl WmbusParams {
    pub fn validate(&self) -> Result<(), String> {
        if self.keys.len() > MAX_WMBUS_KEYS {
            return Err(format!("at most {MAX_WMBUS_KEYS} wM-Bus keys"));
        }
        self.keys.iter().try_for_each(WmbusKey::check)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WmbusMode {
    S,
    #[default]
    T,
    C,
}

impl WmbusMode {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::S => "S",
            Self::T => "T",
            Self::C => "C",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WmbusFormat {
    #[default]
    A,
    B,
}

impl WmbusFormat {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WmbusSecurity {
    #[default]
    None,
    Decrypted,
    NoKey,
    WrongKey,
    Unsupported,
}

impl WmbusSecurity {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Decrypted => "decrypted",
            Self::NoKey => "encrypted, no key",
            Self::WrongKey => "encrypted, wrong key",
            Self::Unsupported => "encrypted, unsupported mode",
        }
    }

    #[must_use]
    pub const fn locked(self) -> bool {
        matches!(self, Self::NoKey | Self::WrongKey | Self::Unsupported)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WmbusQuantity {
    Energy,
    Volume,
    Mass,
    OnTime,
    OperatingTime,
    Power,
    VolumeFlow,
    MassFlow,
    FlowTemperature,
    ReturnTemperature,
    TemperatureDifference,
    ExternalTemperature,
    Pressure,
    Date,
    DateTime,
    HeatCostUnits,
    AveragingDuration,
    ActualityDuration,
    FabricationNumber,
    Identification,
    Humidity,
    ErrorFlags,
    Dimensionless,
    BatteryDays,
    Other,
}

impl WmbusQuantity {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Energy => "Energy",
            Self::Volume => "Volume",
            Self::Mass => "Mass",
            Self::OnTime => "On time",
            Self::OperatingTime => "Operating time",
            Self::Power => "Power",
            Self::VolumeFlow => "Flow",
            Self::MassFlow => "Mass flow",
            Self::FlowTemperature => "Flow temperature",
            Self::ReturnTemperature => "Return temperature",
            Self::TemperatureDifference => "Temperature difference",
            Self::ExternalTemperature => "Temperature",
            Self::Pressure => "Pressure",
            Self::Date => "Date",
            Self::DateTime => "Time",
            Self::HeatCostUnits => "Heat cost units",
            Self::AveragingDuration => "Averaging",
            Self::ActualityDuration => "Actuality",
            Self::FabricationNumber => "Fabrication no.",
            Self::Identification => "Identification",
            Self::Humidity => "Humidity",
            Self::ErrorFlags => "Error flags",
            Self::Dimensionless => "Value",
            Self::BatteryDays => "Battery",
            Self::Other => "Record",
        }
    }

    #[must_use]
    pub const fn unit(self) -> &'static str {
        match self {
            Self::Energy => "Wh",
            Self::Volume => "m³",
            Self::Mass => "kg",
            Self::OnTime
            | Self::OperatingTime
            | Self::AveragingDuration
            | Self::ActualityDuration => "s",
            Self::Power => "W",
            Self::VolumeFlow => "m³/h",
            Self::MassFlow => "kg/h",
            Self::FlowTemperature | Self::ReturnTemperature | Self::ExternalTemperature => "°C",
            Self::TemperatureDifference => "K",
            Self::Pressure => "bar",
            Self::Humidity => "%",
            Self::BatteryDays => "days",
            Self::Date
            | Self::DateTime
            | Self::HeatCostUnits
            | Self::FabricationNumber
            | Self::Identification
            | Self::ErrorFlags
            | Self::Dimensionless
            | Self::Other => "",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WmbusFunction {
    #[default]
    Instantaneous,
    Maximum,
    Minimum,
    Error,
}

impl WmbusFunction {
    #[must_use]
    pub const fn label(self) -> Option<&'static str> {
        match self {
            Self::Instantaneous => None,
            Self::Maximum => Some("max"),
            Self::Minimum => Some("min"),
            Self::Error => Some("error"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct WmbusRecord {
    pub quantity: WmbusQuantity,
    #[serde(default)]
    pub function: WmbusFunction,
    #[serde(default)]
    pub storage: u32,
    #[serde(default)]
    pub tariff: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl WmbusRecord {
    #[must_use]
    pub fn label(&self) -> String {
        let mut label = self.quantity.label().to_owned();
        if let Some(function) = self.function.label() {
            label.push_str(&format!(" ({function})"));
        }
        if self.storage > 0 {
            label.push_str(&format!(" #{}", self.storage));
        }
        if self.tariff > 0 {
            label.push_str(&format!(" T{}", self.tariff));
        }
        label
    }

    #[must_use]
    pub fn reading(&self) -> String {
        if let Some(text) = &self.text {
            return text.clone();
        }
        let Some(value) = self.value else {
            return String::new();
        };
        let unit = self.quantity.unit();
        let number = format!("{value}");
        if unit.is_empty() {
            number
        } else {
            format!("{number} {unit}")
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct WmbusMessage {
    pub mode: WmbusMode,
    pub format: WmbusFormat,
    pub control: u8,
    pub manufacturer: String,
    pub id: String,
    pub version: u8,
    pub device_type: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u8>,
    #[serde(default)]
    pub security: WmbusSecurity,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<WmbusRecord>,
    pub frame: String,
    #[serde(default)]
    pub corrected: u32,
    pub snr_db: f32,
}

impl WmbusMessage {
    #[must_use]
    pub fn station(&self) -> String {
        format!("{} {}", self.manufacturer, self.id)
    }

    #[must_use]
    pub fn device(&self) -> &'static str {
        device_label(self.device_type)
    }

    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts = vec![format!("{} {}", self.device(), self.station())];
        parts.extend(
            self.records
                .iter()
                .find(|record| record.value.is_some() && record.storage == 0)
                .map(|record| format!("{} {}", record.quantity.label(), record.reading())),
        );
        if self.security.locked() {
            parts.push(self.security.label().to_owned());
        }
        parts.join(" · ")
    }

    fn fields(&self) -> Vec<Field> {
        let mut shown = fields(&[
            ("Meter", text(Some(&self.station()))),
            ("Type", text(Some(self.device()))),
            ("Version", count(Some(self.version))),
            ("Via", text(self.via.as_deref())),
            (
                "Link",
                text(Some(&format!(
                    "mode {} format {}, {}",
                    self.mode.label(),
                    self.format.label(),
                    control_label(self.control)
                ))),
            ),
            (
                "CI",
                self.ci.and_then(|ci| text(Some(&format!("{ci:02X}")))),
            ),
            ("Access", count(self.access)),
            (
                "Status",
                self.status.and_then(|st| text(Some(&format!("{st:02X}")))),
            ),
            ("Security", text(Some(self.security.label()))),
        ]);
        shown.extend(self.records.iter().map(|record| Field {
            label: record.label(),
            value: crate::present::FieldValue::Text(record.reading()),
        }));
        shown.extend(fields(&[
            ("Repaired", repaired(self.corrected)),
            ("SNR", number(Some(self.snr_db), Unit::Db, 1)),
            ("Frame", text(Some(&self.frame))),
        ]));
        shown
    }
}

impl EventPayload for WmbusMessage {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            entity: Some(self.station()),
            fields: self.fields(),
            ..Presentation::of(self.summary())
        }
    }
}

#[must_use]
pub fn control_label(control: u8) -> &'static str {
    match control {
        0x44 => "SND_NR",
        0x46 => "SND_IR",
        0x47 => "ACC_NR",
        0x48 => "ACC_DMD",
        0x08 => "RSP_UD",
        0x06 => "CNF_IR",
        0x40 => "SND_NKE",
        0x53 | 0x73 => "SND_UD",
        0x5A | 0x7A => "REQ_UD1",
        0x5B | 0x7B => "REQ_UD2",
        _ => "other",
    }
}

#[must_use]
pub fn device_label(device_type: u8) -> &'static str {
    match device_type {
        0x01 => "Oil",
        0x02 => "Electricity",
        0x03 => "Gas",
        0x04 => "Heat",
        0x05 => "Steam",
        0x06 => "Warm water",
        0x07 => "Water",
        0x08 => "Heat cost allocator",
        0x09 => "Compressed air",
        0x0A | 0x0B => "Cooling",
        0x0C => "Heat",
        0x0D => "Heat and cooling",
        0x0E => "Bus",
        0x15 => "Hot water",
        0x16 => "Cold water",
        0x17 => "Dual water",
        0x18 => "Pressure",
        0x19 => "A/D converter",
        0x1A => "Smoke detector",
        0x1B => "Room sensor",
        0x1C => "Gas detector",
        0x20 => "Breaker",
        0x21 => "Valve",
        0x25 => "Customer unit",
        0x28 => "Waste water",
        0x29 => "Garbage",
        0x31 => "Controller",
        0x32 | 0x33 => "Repeater",
        0x36 | 0x37 => "Radio converter",
        _ => "Meter",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message() -> WmbusMessage {
        WmbusMessage {
            mode: WmbusMode::T,
            format: WmbusFormat::A,
            control: 0x44,
            manufacturer: "BMT".to_owned(),
            id: "18162333".to_owned(),
            version: 0x13,
            device_type: 0x07,
            ci: Some(0x7A),
            access: Some(0xA5),
            status: Some(0),
            security: WmbusSecurity::NoKey,
            frame: "4e44b409".to_owned(),
            snr_db: 20.0,
            ..WmbusMessage::default()
        }
    }

    #[test]
    fn an_encrypted_meter_says_it_has_no_key() {
        assert_eq!(
            message().summary(),
            "Water BMT 18162333 · encrypted, no key"
        );
        let json = serde_json::to_value(message()).unwrap();
        assert_eq!(json["mode"], "t");
        assert_eq!(json["security"], "no_key");
        assert!(json.get("records").is_none());
        let back: WmbusMessage = serde_json::from_value(json).unwrap();
        assert_eq!(back, message());
    }

    #[test]
    fn a_plain_meter_leads_with_its_current_reading() {
        let plain = WmbusMessage {
            security: WmbusSecurity::None,
            records: vec![
                WmbusRecord {
                    quantity: WmbusQuantity::Date,
                    function: WmbusFunction::Instantaneous,
                    storage: 0,
                    tariff: 0,
                    value: None,
                    text: Some("2026-10-01".to_owned()),
                },
                WmbusRecord {
                    quantity: WmbusQuantity::Volume,
                    function: WmbusFunction::Instantaneous,
                    storage: 1,
                    tariff: 0,
                    value: Some(11.0),
                    text: None,
                },
                WmbusRecord {
                    quantity: WmbusQuantity::Volume,
                    function: WmbusFunction::Instantaneous,
                    storage: 0,
                    tariff: 0,
                    value: Some(12.345),
                    text: None,
                },
            ],
            ..message()
        };
        assert_eq!(plain.summary(), "Water BMT 18162333 · Volume 12.345 m³");
        let shown = plain.present(EventKind::Wmbus);
        assert!(shown.labels().contains(&"Volume #1"));
        assert_eq!(
            shown.field("Volume"),
            Some(&crate::present::FieldValue::Text("12.345 m³".to_owned()))
        );
    }

    #[test]
    fn keys_need_an_eight_digit_meter_and_a_full_aes_key() {
        let mut params = WmbusParams {
            keys: vec![WmbusKey {
                meter: "18162333".to_owned(),
                key: "000102030405060708090A0B0C0D0E0F".to_owned(),
            }],
        };
        assert!(params.validate().is_ok());
        params.keys[0].meter = "1816233".to_owned();
        assert!(params.validate().is_err());
        params.keys[0].meter = "18162333".to_owned();
        params.keys[0].key.pop();
        assert!(params.validate().is_err());
        assert_eq!(
            serde_json::to_value(WmbusParams::default()).unwrap(),
            serde_json::json!({})
        );
    }
}
