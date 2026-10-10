use sdrmm_modem::ble::{Packet, Sink, channel_index};
use sdrmm_wire::{BlePhy, DecoderEvent, RemoteIdFrame, RemoteIdPhy, RemoteIdTransport};

use super::{
    odid::{APP_CODE, SERVICE_UUID},
    tracker::{Heard, Tracker},
};
use crate::ble::{
    ad::{SERVICE_DATA_16, structures},
    pdu, wire_phy,
};

pub(crate) fn remote_id_payload(data: &[u8]) -> Option<(u8, &[u8])> {
    structures(data).find_map(|(kind, body)| match body {
        [uuid_low, uuid_high, APP_CODE, counter, rest @ ..]
            if kind == SERVICE_DATA_16
                && u16::from_le_bytes([*uuid_low, *uuid_high]) == SERVICE_UUID =>
        {
            Some((*counter, rest))
        }
        _ => None,
    })
}

fn phy(phy: BlePhy) -> RemoteIdPhy {
    match phy {
        BlePhy::LeCodedS8 => RemoteIdPhy::LeCodedS8,
        BlePhy::LeCodedS2 => RemoteIdPhy::LeCodedS2,
        _ => RemoteIdPhy::Le1m,
    }
}

pub(crate) fn frame(packet: &Packet<'_>, tracker: &mut Tracker) -> Option<RemoteIdFrame> {
    let advert = pdu::parse(packet.pdu, packet.rf.map(channel_index))?;
    let (counter, payload) = remote_id_payload(advert.data)?;
    tracker.frame(Heard {
        transport: if advert.is_extended() {
            RemoteIdTransport::BluetoothExtended
        } else {
            RemoteIdTransport::BluetoothLegacy
        },
        phy: phy(wire_phy(packet.phy)),
        address: advert
            .sender
            .map_or_else(|| "unknown".to_owned(), |sender| sender.text()),
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
