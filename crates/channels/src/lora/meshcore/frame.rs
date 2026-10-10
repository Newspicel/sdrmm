use sdrmm_wire::{MeshcorePayloadType, MeshcoreRoute};

use crate::datalink::hex;

pub(super) const MAX_PATH_BYTES: usize = 64;
pub(super) const MAX_PAYLOAD_BYTES: usize = 184;
const RESERVED_HASH_SIZE: usize = 4;

pub(super) struct Frame<'a> {
    pub(super) route: MeshcoreRoute,
    pub(super) payload_type: MeshcorePayloadType,
    pub(super) transport_codes: Vec<u16>,
    pub(super) path: &'a [u8],
    pub(super) hash_size: usize,
    pub(super) payload: &'a [u8],
}

impl<'a> Frame<'a> {
    pub(super) fn parse(bytes: &'a [u8]) -> Option<Self> {
        let (&header, rest) = bytes.split_first()?;
        if header >> 6 != 0 {
            return None;
        }
        let route = route(header & 0x03);
        let (transport_codes, rest) = transport_codes(route, rest)?;
        let (&path_length, rest) = rest.split_first()?;
        let hash_size = usize::from(path_length >> 6) + 1;
        let path_bytes = usize::from(path_length & 0x3f) * hash_size;
        if hash_size == RESERVED_HASH_SIZE || path_bytes > MAX_PATH_BYTES {
            return None;
        }
        let (path, payload) = rest.split_at_checked(path_bytes)?;
        if payload.is_empty() || payload.len() > MAX_PAYLOAD_BYTES {
            return None;
        }
        Some(Self {
            route,
            payload_type: payload_type((header >> 2) & 0x0f),
            transport_codes,
            path,
            hash_size,
            payload,
        })
    }

    pub(super) fn path_labels(&self) -> Vec<String> {
        if self.payload_type == MeshcorePayloadType::Trace {
            self.path.iter().map(|&snr| snr_label(snr)).collect()
        } else {
            self.path.chunks(self.hash_size).map(hex).collect()
        }
    }
}

fn transport_codes(route: MeshcoreRoute, bytes: &[u8]) -> Option<(Vec<u16>, &[u8])> {
    if !matches!(
        route,
        MeshcoreRoute::TransportFlood | MeshcoreRoute::TransportDirect
    ) {
        return Some((Vec::new(), bytes));
    }
    let ([a0, a1, b0, b1], rest) = bytes.split_first_chunk::<4>()?;
    Some((
        vec![
            u16::from_le_bytes([*a0, *a1]),
            u16::from_le_bytes([*b0, *b1]),
        ],
        rest,
    ))
}

fn snr_label(byte: u8) -> String {
    format!("{:.2}", f32::from(byte.cast_signed()) / 4.0)
}

fn route(bits: u8) -> MeshcoreRoute {
    match bits {
        0 => MeshcoreRoute::TransportFlood,
        1 => MeshcoreRoute::Flood,
        2 => MeshcoreRoute::Direct,
        _ => MeshcoreRoute::TransportDirect,
    }
}

fn payload_type(bits: u8) -> MeshcorePayloadType {
    match bits {
        0 => MeshcorePayloadType::Request,
        1 => MeshcorePayloadType::Response,
        2 => MeshcorePayloadType::Text,
        3 => MeshcorePayloadType::Ack,
        4 => MeshcorePayloadType::Advert,
        5 => MeshcorePayloadType::GroupText,
        6 => MeshcorePayloadType::GroupData,
        7 => MeshcorePayloadType::AnonRequest,
        8 => MeshcorePayloadType::Path,
        9 => MeshcorePayloadType::Trace,
        10 => MeshcorePayloadType::Multipart,
        11 => MeshcorePayloadType::Control,
        15 => MeshcorePayloadType::RawCustom,
        _ => MeshcorePayloadType::Reserved,
    }
}
