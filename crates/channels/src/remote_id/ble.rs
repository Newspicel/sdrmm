pub(crate) mod band;
pub(crate) mod coded;
pub(crate) mod gfsk;
pub(crate) mod receiver;

use sdrmm_wire::{RemoteIdFrame, RemoteIdTransport};

use self::receiver::Packet;
use super::{
    odid::{APP_CODE, SERVICE_UUID},
    tracker::{Heard, Tracker},
};

pub(crate) const ACCESS_ADDRESS: u32 = 0x8E89_BED6;
pub(crate) const CRC_INIT: u32 = 0x55_5555;
pub(crate) const CRC_BYTES: usize = 3;
pub(crate) const HEADER_BYTES: usize = 2;
pub(crate) const CHANNEL_SPACING_HZ: f64 = 2_000_000.0;
pub(crate) const ADVERTISING_37: u8 = 0;
const BAND_START_HZ: f64 = 2_402_000_000.0;
const CHANNELS: u8 = 40;
const ADV_IND: u8 = 0;
const ADV_NONCONN_IND: u8 = 2;
const SCAN_RSP: u8 = 4;
const ADV_SCAN_IND: u8 = 6;
const ADV_EXT_IND: u8 = 7;
const ADDRESS_BYTES: usize = 6;
const AD_SERVICE_DATA_16: u8 = 0x16;
const CRC_POLY_REFLECTED: u32 = 0xDA_6000;

#[must_use]
pub(crate) fn rf_channel_hz(rf: u8) -> f64 {
    BAND_START_HZ + f64::from(rf) * CHANNEL_SPACING_HZ
}

#[must_use]
pub(crate) fn nearest_rf_channel(frequency_hz: f64) -> Option<u8> {
    let rf = ((frequency_hz - BAND_START_HZ) / CHANNEL_SPACING_HZ).round();
    (0.0..f64::from(CHANNELS)).contains(&rf).then_some(rf as u8)
}

#[must_use]
pub(crate) fn channel_index(rf: u8) -> u8 {
    match rf {
        0 => 37,
        12 => 38,
        39 => 39,
        1..=11 => rf - 1,
        _ => rf - 2,
    }
}

pub(crate) struct Whitener(u8);

impl Whitener {
    pub(crate) fn new(channel_index: u8) -> Self {
        Self(channel_index.reverse_bits() | 0x02)
    }

    pub(crate) fn bit(&mut self) -> bool {
        let out = self.0 & 0x80 != 0;
        if out {
            self.0 ^= 0x11;
        }
        self.0 <<= 1;
        out
    }

    pub(crate) fn byte(&mut self, byte: u8) -> u8 {
        (0..8).fold(byte, |acc, bit| acc ^ (u8::from(self.bit()) << bit))
    }
}

#[must_use]
pub(crate) fn crc24(data: &[u8]) -> u32 {
    let mut crc = CRC_INIT.reverse_bits() >> 8;
    for &byte in data {
        for bit in 0..8 {
            let feedback = (crc ^ u32::from(byte >> bit)) & 1;
            crc >>= 1;
            if feedback != 0 {
                crc ^= CRC_POLY_REFLECTED;
            }
        }
    }
    crc
}

#[must_use]
pub(crate) fn crc_ok(pdu_and_crc: &[u8]) -> bool {
    let Some(split) = pdu_and_crc.len().checked_sub(CRC_BYTES) else {
        return false;
    };
    let (pdu, crc) = pdu_and_crc.split_at(split);
    let received = u32::from(crc[0]) | u32::from(crc[1]) << 8 | u32::from(crc[2]) << 16;
    crc24(pdu) == received
}

#[derive(Debug, PartialEq)]
pub(crate) struct Advert<'a> {
    pub transport: RemoteIdTransport,
    pub address: Option<[u8; ADDRESS_BYTES]>,
    pub data: &'a [u8],
}

pub(crate) fn advert(pdu: &[u8]) -> Option<Advert<'_>> {
    let [header, length, payload @ ..] = pdu else {
        return None;
    };
    let payload = payload.get(..usize::from(*length))?;
    match header & 0x0F {
        ADV_IND | ADV_NONCONN_IND | SCAN_RSP | ADV_SCAN_IND => {
            let (address, data) = payload.split_at_checked(ADDRESS_BYTES)?;
            Some(Advert {
                transport: RemoteIdTransport::BluetoothLegacy,
                address: address.try_into().ok(),
                data,
            })
        }
        ADV_EXT_IND => extended(payload),
        _ => None,
    }
}

fn extended(payload: &[u8]) -> Option<Advert<'_>> {
    let (&first, rest) = payload.split_first()?;
    let header_len = usize::from(first & 0x3F);
    let (header, data) = rest.split_at_checked(header_len)?;
    let address = header
        .split_first()
        .filter(|(flags, _)| *flags & 0x01 != 0)
        .and_then(|(_, fields)| fields.get(..ADDRESS_BYTES)?.try_into().ok());
    Some(Advert {
        transport: RemoteIdTransport::BluetoothExtended,
        address,
        data,
    })
}

pub(crate) fn remote_id_payload(data: &[u8]) -> Option<(u8, &[u8])> {
    let mut rest = data;
    while let [length, tail @ ..] = rest {
        let length = usize::from(*length);
        if length == 0 {
            return None;
        }
        let (structure, next) = tail.split_at_checked(length)?;
        if let [
            AD_SERVICE_DATA_16,
            uuid_low,
            uuid_high,
            APP_CODE,
            counter,
            body @ ..,
        ] = structure
            && u16::from_le_bytes([*uuid_low, *uuid_high]) == SERVICE_UUID
        {
            return Some((*counter, body));
        }
        rest = next;
    }
    None
}

#[must_use]
pub(crate) fn address_text(address: &[u8; ADDRESS_BYTES]) -> String {
    address
        .iter()
        .rev()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

pub(crate) fn frame(packet: &Packet, tracker: &mut Tracker) -> Option<RemoteIdFrame> {
    let advert = advert(&packet.pdu)?;
    let (counter, payload) = remote_id_payload(advert.data)?;
    tracker.frame(Heard {
        transport: advert.transport,
        phy: packet.phy,
        address: advert
            .address
            .as_ref()
            .map_or_else(|| "unknown".to_owned(), address_text),
        channel: packet.rf.map(channel_index),
        counter: Some(counter),
        ssid: None,
        level_dbfs: packet.level_dbfs,
        payload,
    })
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn whiten(channel_index: u8, bytes: &mut [u8]) {
    let mut whitener = Whitener::new(channel_index);
    for byte in bytes {
        *byte = whitener.byte(*byte);
    }
}

#[cfg(test)]
pub(crate) mod tests;
