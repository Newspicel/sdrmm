use nusb::{DeviceInfo, MaybeFuture};

use crate::error::{Error, Result};

pub(crate) const VENDOR_ID: u16 = 0x04d8;
pub(crate) const PRODUCT_ID: u16 = 0xfb31;
#[cfg(target_os = "macos")]
const AUDIO_ENGINE: &str = "AppleUSBAudioEngine:";

pub(crate) fn is_pro_plus(vendor_id: u16, product_id: u16) -> bool {
    vendor_id == VENDOR_ID && product_id == PRODUCT_ID
}

pub(crate) fn list() -> Result<Vec<DeviceInfo>> {
    Ok(nusb::list_devices()
        .wait()
        .map_err(Error::Usb)?
        .filter(|info| is_pro_plus(info.vendor_id(), info.product_id()))
        .collect())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn find(key: &str) -> Result<DeviceInfo> {
    list()?
        .into_iter()
        .find(|info| self::key(info) == key)
        .ok_or(Error::DeviceNotFound)
}

#[cfg(target_os = "macos")]
pub(crate) fn key(info: &DeviceInfo) -> String {
    format!("{:08x}", info.location_id())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn key(info: &DeviceInfo) -> String {
    port_key(info.bus_id(), info.port_chain())
}

#[cfg_attr(target_os = "macos", cfg(test))]
pub(crate) fn port_key(bus: &str, ports: &[u8]) -> String {
    let ports: Vec<String> = ports.iter().map(u8::to_string).collect();
    format!("{bus}-{}", ports.join("."))
}

#[cfg(target_os = "macos")]
pub(crate) fn location(key: &str) -> Option<u32> {
    if key.len() != 8 {
        return None;
    }
    u32::from_str_radix(key, 16).ok()
}

#[cfg(target_os = "macos")]
pub(crate) fn audio_belongs_to(uid: &str, location: u32) -> bool {
    uid.starts_with(AUDIO_ENGINE)
        && uid
            .rsplit(':')
            .nth(1)
            .is_some_and(|part| u32::from_str_radix(part, 16) == Ok(location))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_pro_plus_is_claimed() {
        assert!(is_pro_plus(0x04d8, 0xfb31));
        assert!(!is_pro_plus(0x04d8, 0xfb56), "that is the original Pro");
        assert!(!is_pro_plus(0x0bda, 0x2838));
    }

    #[test]
    fn a_port_key_names_the_bus_and_every_hub_port() {
        assert_eq!(port_key("1", &[4]), "1-4");
        assert_eq!(port_key("3", &[1, 2, 7]), "3-1.2.7");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_location_key_survives_a_round_trip() {
        assert_eq!(location("01140000"), Some(0x0114_0000));
        assert_eq!(location("1140000"), None);
        assert_eq!(location("0114000g"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_audio_device_is_found_by_the_port_in_its_uid() {
        const UID: &str = "AppleUSBAudioEngine:Hanlincrest Ltd.:FUNcube Dongle V2.0:1140000:1";
        assert!(audio_belongs_to(UID, 0x0114_0000));
        assert!(!audio_belongs_to(UID, 0x0115_0000));
        assert!(!audio_belongs_to("BuiltInMicrophoneDevice", 0x0114_0000));
    }
}
