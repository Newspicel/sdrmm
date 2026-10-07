use sdrmm_wire::{MeshcoreContent, MeshcorePayloadType};

use super::advert_data::{self, PUBLIC_KEY_LEN};
use super::frame::Frame;
use super::group::{self, MAC_LEN, Sealed, is_block_aligned};
use super::keys::Keys;
use crate::datalink::hex;

const ACK_LEN: usize = 4;
const TRACE_HEADER_LEN: usize = 9;
const MIN_MULTIPART_LEN: usize = 3;

pub(super) fn content(frame: &Frame<'_>, keys: &Keys) -> Option<MeshcoreContent> {
    let payload = frame.payload;
    match frame.payload_type {
        MeshcorePayloadType::Request
        | MeshcorePayloadType::Response
        | MeshcorePayloadType::Text
        | MeshcorePayloadType::Path => encrypted(payload),
        MeshcorePayloadType::Ack => ack(payload),
        MeshcorePayloadType::Advert => advert_data::content(payload),
        MeshcorePayloadType::GroupText => Some(group::text(&Sealed::parse(payload)?, keys)),
        MeshcorePayloadType::GroupData => Some(group::data(&Sealed::parse(payload)?, keys)),
        MeshcorePayloadType::AnonRequest => anon_request(payload),
        MeshcorePayloadType::Trace => trace(payload),
        MeshcorePayloadType::Multipart => multipart(payload),
        MeshcorePayloadType::Control => control(payload),
        MeshcorePayloadType::Reserved | MeshcorePayloadType::RawCustom => {
            Some(MeshcoreContent::Raw { data: hex(payload) })
        }
    }
}

fn encrypted(payload: &[u8]) -> Option<MeshcoreContent> {
    let ([destination, source], rest) = payload.split_first_chunk::<2>()?;
    let (mac, ciphertext) = rest.split_first_chunk::<MAC_LEN>()?;
    is_block_aligned(ciphertext).then(|| MeshcoreContent::Encrypted {
        destination: hex(&[*destination]),
        source: hex(&[*source]),
        mac: hex(mac),
    })
}

fn anon_request(payload: &[u8]) -> Option<MeshcoreContent> {
    let (&destination, rest) = payload.split_first()?;
    let (public_key, rest) = rest.split_first_chunk::<PUBLIC_KEY_LEN>()?;
    let (_, ciphertext) = rest.split_first_chunk::<MAC_LEN>()?;
    is_block_aligned(ciphertext).then(|| MeshcoreContent::AnonRequest {
        destination: hex(&[destination]),
        public_key: hex(public_key),
    })
}

fn ack(payload: &[u8]) -> Option<MeshcoreContent> {
    let checksum: [u8; ACK_LEN] = payload.try_into().ok()?;
    Some(MeshcoreContent::Ack {
        checksum: u32::from_le_bytes(checksum),
    })
}

fn trace(payload: &[u8]) -> Option<MeshcoreContent> {
    let (header, hops) = payload.split_first_chunk::<TRACE_HEADER_LEN>()?;
    let [t0, t1, t2, t3, a0, a1, a2, a3, flags] = *header;
    let hop_size = 1usize << (flags & 0x03);
    if !hops.len().is_multiple_of(hop_size) {
        return None;
    }
    Some(MeshcoreContent::Trace {
        tag: u32::from_le_bytes([t0, t1, t2, t3]),
        auth_code: u32::from_le_bytes([a0, a1, a2, a3]),
        flags,
        hops: hops.chunks_exact(hop_size).map(hex).collect(),
    })
}

fn multipart(payload: &[u8]) -> Option<MeshcoreContent> {
    let (&first, rest) = payload.split_first()?;
    (payload.len() >= MIN_MULTIPART_LEN).then(|| MeshcoreContent::Multipart {
        remaining: first >> 4,
        inner_type: first & 0x0f,
        data: hex(rest),
    })
}

fn control(payload: &[u8]) -> Option<MeshcoreContent> {
    let (&first, rest) = payload.split_first()?;
    Some(MeshcoreContent::Control {
        subtype: first >> 4,
        data: hex(rest),
    })
}
