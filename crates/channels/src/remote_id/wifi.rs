mod dji;
pub(crate) mod dsss;
mod french;
pub(crate) mod ofdm;
pub(crate) mod receiver;

use sdrmm_wire::{RemoteIdFrame, RemoteIdPhy, RemoteIdTransport};

pub(crate) use self::receiver::Wifi;
use super::{
    odid::APP_CODE,
    tracker::{Heard, Tracker},
};

const FCS_BYTES: usize = 4;
const HEADER_BYTES: usize = 24;
const BEACON_FIXED: usize = 12;
const BEACON: u16 = 0x0080;
const ACTION: u16 = 0x00D0;
const TYPE_MASK: u16 = 0x00FC;
const SSID: u8 = 0;
const VENDOR: u8 = 221;
const ASTM_OUI: [u8; 3] = [0xFA, 0x0B, 0xBC];
const WFA_OUI: [u8; 3] = [0x50, 0x6F, 0x9A];
const NAN_TYPE: u8 = 0x13;
const PUBLIC_ACTION: u8 = 4;
const VENDOR_ACTION: u8 = 9;
const SERVICE_DESCRIPTOR: u8 = 0x03;
pub(crate) const REMOTE_ID_SERVICE: [u8; 6] = [0x88, 0x69, 0x19, 0x9D, 0x92, 0x09];
const BINDING_BITMAP: u8 = 0x40;
const MATCHING_FILTER: u8 = 0x04;
const RESPONSE_FILTER: u8 = 0x08;
const SERVICE_INFO: u8 = 0x10;
const BAND_24_START_MHZ: f64 = 2_407.0;
const BAND_5_START_MHZ: f64 = 5_000.0;
const CHANNEL_14_MHZ: f64 = 2_484.0;

#[must_use]
pub(crate) fn crc32(data: &[u8]) -> u32 {
    !data.iter().fold(u32::MAX, |crc, &byte| {
        (0..8).fold(crc ^ u32::from(byte), |crc, _| {
            if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            }
        })
    })
}

#[must_use]
pub(crate) fn fcs_ok(mpdu: &[u8]) -> bool {
    let Some(split) = mpdu.len().checked_sub(FCS_BYTES) else {
        return false;
    };
    let (body, fcs) = mpdu.split_at(split);
    crc32(body).to_le_bytes() == fcs
}

#[must_use]
pub(crate) fn channel_number(frequency_hz: f64) -> Option<u8> {
    let mhz = frequency_hz / 1e6;
    if (mhz - CHANNEL_14_MHZ).abs() < 2.5 {
        return Some(14);
    }
    let start = if (2_400.0..2_500.0).contains(&mhz) {
        BAND_24_START_MHZ
    } else if (5_000.0..5_900.0).contains(&mhz) {
        BAND_5_START_MHZ
    } else {
        return None;
    };
    let number = ((mhz - start) / 5.0).round();
    (1.0..=196.0).contains(&number).then_some(number as u8)
}

#[must_use]
pub(crate) fn wanted_control(control: u8) -> bool {
    matches!(u16::from(control) & TYPE_MASK, BEACON | ACTION)
}

#[must_use]
pub(crate) fn wanted(mpdu: &[u8]) -> bool {
    mpdu.len() >= HEADER_BYTES + FCS_BYTES && wanted_control(mpdu[0])
}

pub(crate) struct Mpdu<'a> {
    pub bytes: &'a [u8],
    pub phy: RemoteIdPhy,
    pub channel: Option<u8>,
    pub level_dbfs: f32,
}

pub(crate) fn frame(mpdu: &Mpdu<'_>, tracker: &mut Tracker) -> Option<RemoteIdFrame> {
    let bytes = mpdu.bytes;
    let body_end = bytes.len().checked_sub(FCS_BYTES)?;
    let header = bytes.get(..HEADER_BYTES)?;
    let control = u16::from_le_bytes([header[0], header[1]]) & TYPE_MASK;
    let address = address_text(&header[10..16]);
    let body = bytes.get(HEADER_BYTES..body_end)?;
    let heard = |transport, counter, ssid, payload| Heard {
        transport,
        phy: mpdu.phy,
        address: address.clone(),
        channel: mpdu.channel,
        counter,
        ssid,
        level_dbfs: mpdu.level_dbfs,
        payload,
    };
    match control {
        BEACON => {
            let elements = body.get(BEACON_FIXED..)?;
            let ssid = element(elements, |id, _| id == SSID).map(text);
            if let Some(found) = element(elements, |id, data| {
                id == VENDOR && data.starts_with(&ASTM_OUI) && data.get(3) == Some(&APP_CODE)
            }) {
                let (&counter, pack) = found.get(4..)?.split_first()?;
                return tracker.frame(heard(
                    RemoteIdTransport::WifiBeacon,
                    Some(counter),
                    ssid,
                    pack,
                ));
            }
            if let Some(drone_id) =
                element(elements, |id, data| id == VENDOR && dji::is_drone_id(data))
            {
                let messages = dji::messages(drone_id)?;
                return Some(tracker.direct(
                    heard(RemoteIdTransport::WifiBeaconDji, None, ssid, &[]),
                    messages,
                ));
            }
            let french = element(elements, |id, data| {
                id == VENDOR && data.starts_with(&french::OUI) && data.get(3) == Some(&french::TYPE)
            })?;
            let messages = french::messages(french.get(4..)?)?;
            Some(tracker.direct(
                heard(RemoteIdTransport::WifiBeaconFrench, None, ssid, &[]),
                messages,
            ))
        }
        ACTION => {
            let (counter, pack) = nan_service_info(body)?;
            tracker.frame(heard(RemoteIdTransport::WifiNan, Some(counter), None, pack))
        }
        _ => None,
    }
}

fn element(elements: &[u8], wanted: impl Fn(u8, &[u8]) -> bool) -> Option<&[u8]> {
    let mut rest = elements;
    while let [id, length, tail @ ..] = rest {
        let (data, next) = tail.split_at_checked(usize::from(*length))?;
        if wanted(*id, data) {
            return Some(data);
        }
        rest = next;
    }
    None
}

fn nan_service_info(body: &[u8]) -> Option<(u8, &[u8])> {
    let [
        PUBLIC_ACTION,
        VENDOR_ACTION,
        o1,
        o2,
        o3,
        NAN_TYPE,
        attributes @ ..,
    ] = body
    else {
        return None;
    };
    if [*o1, *o2, *o3] != WFA_OUI {
        return None;
    }
    let mut rest = attributes;
    while let [id, low, high, tail @ ..] = rest {
        let (data, next) = tail.split_at_checked(usize::from(u16::from_le_bytes([*low, *high])))?;
        if *id == SERVICE_DESCRIPTOR
            && data.starts_with(&REMOTE_ID_SERVICE)
            && let Some(info) = service_info(data)
        {
            let (&counter, pack) = info.split_first()?;
            return Some((counter, pack));
        }
        rest = next;
    }
    None
}

fn service_info(descriptor: &[u8]) -> Option<&[u8]> {
    let control = *descriptor.get(8)?;
    let mut at = 9;
    if control & BINDING_BITMAP != 0 {
        at += 2;
    }
    for flag in [MATCHING_FILTER, RESPONSE_FILTER] {
        if control & flag != 0 {
            at += 1 + usize::from(*descriptor.get(at)?);
        }
    }
    if control & SERVICE_INFO == 0 {
        return None;
    }
    let length = usize::from(*descriptor.get(at)?);
    descriptor.get(at + 1..at + 1 + length)
}

fn address_text(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

#[cfg(any(test, feature = "synth"))]
pub mod build;

#[cfg(test)]
mod tests;
