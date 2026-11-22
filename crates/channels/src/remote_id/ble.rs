use crate::remote_id::RemoteIdFrame;
use crate::remote_id::RemoteIdPhy;
use crate::remote_id::RemoteIdTransport;
use sdrmm_decoder::SymbolTap;
use sdrmm_modem::ble::{
    BlePhy, Packet, Sink,
    ad::{SERVICE_DATA_16, structures},
    channel_index, pdu,
};
use sdrmm_modem::cpm::Mapping;
use sdrmm_wire::DecoderEvent;

use super::{
    odid::{APP_CODE, SERVICE_UUID},
    tracker::{Heard, Tracker},
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
        BlePhy::CodedS8 => RemoteIdPhy::LeCodedS8,
        BlePhy::CodedS2 => RemoteIdPhy::LeCodedS2,
        BlePhy::Le1m => RemoteIdPhy::Le1m,
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
        phy: phy(packet.phy),
        address: advert
            .sender
            .map_or_else(|| "unknown".to_owned(), pdu::Party::text),
        channel: packet.rf.map(channel_index),
        counter: Some(counter),
        ssid: None,
        level_dbfs: packet.level_dbfs,
        payload,
    })
}

pub(crate) struct Events<'a> {
    pub tracker: &'a mut Tracker,
    pub out: &'a mut Vec<DecoderEvent>,
    pub tap: &'a mut SymbolTap,
    pub mapping: &'a Mapping,
}

impl Sink for Events<'_> {
    fn packet(&mut self, packet: Packet<'_>) {
        if let Some(frame) = frame(&packet, self.tracker) {
            self.out.push(DecoderEvent::of(&frame));
        }
    }

    fn symbols(&mut self, soft: &[f32], phy: BlePhy, offset_hz: f64) {
        self.tap
            .levels(soft, &[], self.mapping, phy.soft_rate_hz(), offset_hz);
    }
}

#[cfg(test)]
pub(crate) mod tests;
