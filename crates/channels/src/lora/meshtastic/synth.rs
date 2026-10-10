use sdrmm_wire::MeshtasticPacket;

use super::proto::Writer;
use super::{HOP_START_SHIFT, channel_hash, expand_psk, nonce, ports};
use crate::lora::crypto::{self, Aes};

const HOPS: u8 = 3;

pub(crate) fn text(from: u32, to: u32, id: u32, channel: &str, psk: &[u8], text: &str) -> Vec<u8> {
    let data = Writer::default()
        .varint(1, u64::from(ports::TEXT_MESSAGE))
        .bytes(2, text.as_bytes())
        .finish();
    packet(from, to, id, channel, psk, data)
}

pub(crate) fn position(
    from: u32,
    id: u32,
    channel: &str,
    psk: &[u8],
    lat: f64,
    lon: f64,
    altitude_m: i32,
) -> Vec<u8> {
    let position = Writer::default()
        .sfixed32(1, scaled(lat))
        .sfixed32(2, scaled(lon))
        .int32(3, altitude_m)
        .finish();
    let data = Writer::default()
        .varint(1, u64::from(ports::POSITION))
        .bytes(2, &position)
        .finish();
    packet(from, MeshtasticPacket::BROADCAST, id, channel, psk, data)
}

fn scaled(degrees: f64) -> i32 {
    (degrees * 1e7).round() as i32
}

pub(super) fn packet(
    from: u32,
    to: u32,
    id: u32,
    channel: &str,
    psk: &[u8],
    mut data: Vec<u8>,
) -> Vec<u8> {
    let key = expand_psk(psk).unwrap_or_default();
    if let Some(cipher) = Aes::new(&key) {
        crypto::ctr(&cipher, &nonce(id, from), &mut data);
    }
    let mut packet = Vec::with_capacity(super::HEADER_LEN + data.len());
    packet.extend_from_slice(&to.to_le_bytes());
    packet.extend_from_slice(&from.to_le_bytes());
    packet.extend_from_slice(&id.to_le_bytes());
    packet.push(HOPS | (HOPS << HOP_START_SHIFT));
    packet.push(channel_hash(channel, &key));
    packet.push(0);
    packet.push(from.to_le_bytes()[0]);
    packet.extend_from_slice(&data);
    packet
}
