use sdrmm_wire::{device::DeviceInfo, rest::WriteSerialRequest};
use serde::{Deserialize, Deserializer};

use crate::{api::ApiFailure, ui::faces::device::devices::device_id};

const CONFLICT: u16 = 409;
const NEEDS_SERIAL: &str = "rtlsdr";

#[must_use]
pub fn lacks_serial(device: &DeviceInfo) -> bool {
    device.driver == NEEDS_SERIAL && device.serial.is_none()
}

#[must_use]
pub fn next_to_ask<'a>(devices: &'a [DeviceInfo], asked: &[String]) -> Option<&'a DeviceInfo> {
    devices
        .iter()
        .find(|device| lacks_serial(device) && !asked.contains(&device_id(device)))
}

#[must_use]
pub fn mark_asked(asked: &[String], id: &str) -> Vec<String> {
    let mut next = asked.to_vec();
    if !next.iter().any(|held| held == id) {
        next.push(id.to_owned());
    }
    next
}

#[must_use]
pub fn request(device_id: String, typed: &str) -> WriteSerialRequest {
    let serial = typed.trim();
    WriteSerialRequest {
        device_id,
        serial: (!serial.is_empty()).then(|| serial.to_owned()),
    }
}

#[must_use]
pub fn failure(error: &anyhow::Error) -> String {
    match error.downcast_ref::<ApiFailure>() {
        Some(failure) if failure.status == CONFLICT => {
            String::from("Close this radio on its Device node first.")
        }
        _ => error.to_string(),
    }
}

pub fn lenient_ids<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Array(items) => items
            .into_iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::prefs::Prefs;

    fn device(driver: &str, key: &str, serial: Option<&str>) -> DeviceInfo {
        DeviceInfo {
            driver: driver.to_owned(),
            key: key.to_owned(),
            label: key.to_owned(),
            serial: serial.map(str::to_owned),
            profile: None,
        }
    }

    #[test]
    fn asks_only_about_rtl_sdrs_told_apart_by_port() {
        assert!(lacks_serial(&device("rtlsdr", "1/5", None)));
        assert!(!lacks_serial(&device(
            "rtlsdr",
            "00000123",
            Some("00000123")
        )));
        assert!(!lacks_serial(&device("hackrf", "1/6", None)));
    }

    #[test]
    fn asks_once_per_dongle() {
        let attached = [
            device("rtlsdr", "00000007", Some("00000007")),
            device("rtlsdr", "1/5", None),
        ];
        let first = next_to_ask(&attached, &[]).map(|device| device.key.as_str());
        assert_eq!(first, Some("1/5"));
        let asked = mark_asked(&[], "rtlsdr:1/5");
        assert!(next_to_ask(&attached, &asked).is_none());
        assert_eq!(asked, ["rtlsdr:1/5"]);
        assert_eq!(mark_asked(&asked, "rtlsdr:1/5"), ["rtlsdr:1/5"]);
    }

    #[test]
    fn an_empty_serial_lets_the_server_pick_one() {
        assert_eq!(request(String::from("rtlsdr:1/5"), "  ").serial, None);
        assert_eq!(
            request(String::from("rtlsdr:1/5"), " 42 ")
                .serial
                .as_deref(),
            Some("42")
        );
    }

    #[test]
    fn a_busy_radio_says_where_to_close_it() {
        let busy = anyhow::Error::from(ApiFailure::read(409, r#"{"error":"device busy"}"#));
        assert_eq!(failure(&busy), "Close this radio on its Device node first.");
        let bad = anyhow::Error::from(ApiFailure::read(400, r#"{"error":"bad serial"}"#));
        assert_eq!(failure(&bad), "bad serial");
    }

    #[test]
    fn stays_off_once_turned_off() {
        let prefs: Prefs =
            serde_json::from_str(r#"{"serial_prompt_off":true}"#).expect("prefs read");
        assert!(prefs.serial_prompt_off);
        assert!(!Prefs::default().serial_prompt_off);
    }

    #[test]
    fn survives_a_garbled_store() {
        let garbled: Prefs = serde_json::from_str(r#"{"theme":"light","serial_asked":"{nope"}"#)
            .expect("prefs read");
        assert_eq!(garbled.serial_asked, Vec::<String>::new());
        assert_eq!(garbled.theme, "light");
        let mixed: Prefs = serde_json::from_str(r#"{"serial_asked":["a",3]}"#).expect("prefs read");
        assert_eq!(mixed.serial_asked, ["a"]);
    }
}
