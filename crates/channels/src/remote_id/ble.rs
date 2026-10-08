use sdrmm_modem::ble::{BlePhy, Packet, Sink, channel_index};
use sdrmm_wire::{DecoderEvent, RemoteIdFrame, RemoteIdPhy, RemoteIdTransport};

use super::{
    odid::{APP_CODE, SERVICE_UUID},
    tracker::{Heard, Tracker},
};

const ADV_IND: u8 = 0;
const ADV_NONCONN_IND: u8 = 2;
const SCAN_RSP: u8 = 4;
const ADV_SCAN_IND: u8 = 6;
const ADV_EXT_IND: u8 = 7;
const ADDRESS_BYTES: usize = 6;
const AD_SERVICE_DATA_16: u8 = 0x16;

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

fn phy(phy: BlePhy) -> RemoteIdPhy {
    match phy {
        BlePhy::Le1m => RemoteIdPhy::Le1m,
        BlePhy::CodedS8 => RemoteIdPhy::LeCodedS8,
        BlePhy::CodedS2 => RemoteIdPhy::LeCodedS2,
    }
}

pub(crate) fn frame(packet: &Packet<'_>, tracker: &mut Tracker) -> Option<RemoteIdFrame> {
    let advert = advert(packet.pdu)?;
    let (counter, payload) = remote_id_payload(advert.data)?;
    tracker.frame(Heard {
        transport: advert.transport,
        phy: phy(packet.phy),
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

pub(crate) struct Events<'a> {
    pub tracker: &'a mut Tracker,
    pub events: &'a mut Vec<DecoderEvent>,
}

impl Sink for Events<'_> {
    fn packet(&mut self, packet: Packet<'_>) {
        if let Some(frame) = frame(&packet, self.tracker) {
            self.events.push(DecoderEvent::RemoteId(frame));
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
