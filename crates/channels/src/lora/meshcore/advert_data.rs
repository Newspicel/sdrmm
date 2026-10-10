use sdrmm_wire::{MeshcoreContent, MeshcoreNodeType};

use super::super::crypto;
use crate::datalink::hex;

pub(super) const PUBLIC_KEY_LEN: usize = 32;
pub(super) const SIGNATURE_LEN: usize = 64;
pub(super) const MAX_APP_DATA_LEN: usize = 32;
pub(super) const HAS_LOCATION: u8 = 0x10;
const HAS_FEATURE_1: u8 = 0x20;
const HAS_FEATURE_2: u8 = 0x40;
pub(super) const HAS_NAME: u8 = 0x80;
pub(super) const COORDINATE_SCALE: f64 = 1e6;

struct AppData {
    node_type: MeshcoreNodeType,
    location: Option<(f64, f64)>,
    name: Option<String>,
}

pub(super) fn content(payload: &[u8]) -> Option<MeshcoreContent> {
    let (public_key, rest) = payload.split_first_chunk::<PUBLIC_KEY_LEN>()?;
    let (timestamp, rest) = rest.split_first_chunk::<4>()?;
    let (signature, app_data) = rest.split_first_chunk::<SIGNATURE_LEN>()?;
    let signed_app_data = app_data.get(..MAX_APP_DATA_LEN).unwrap_or(app_data);
    let parsed = parse_app_data(signed_app_data)?;
    let message = [public_key.as_slice(), timestamp, signed_app_data].concat();
    let (lat, lon) = parsed.location.unzip();
    Some(MeshcoreContent::Advert {
        public_key: hex(public_key),
        timestamp: u32::from_le_bytes(*timestamp),
        node_type: parsed.node_type,
        name: parsed.name,
        lat,
        lon,
        signature_ok: crypto::ed25519_verify(public_key, &message, signature),
    })
}

fn parse_app_data(app_data: &[u8]) -> Option<AppData> {
    let (&flags, mut rest) = app_data.split_first()?;
    let mut location = None;
    if flags & HAS_LOCATION != 0 {
        let (lat, after_lat) = rest.split_first_chunk::<4>()?;
        let (lon, after_lon) = after_lat.split_first_chunk::<4>()?;
        location = Some((coordinate(lat), coordinate(lon)));
        rest = after_lon;
    }
    for feature in [HAS_FEATURE_1, HAS_FEATURE_2] {
        if flags & feature != 0 {
            rest = rest.get(2..)?;
        }
    }
    Some(AppData {
        node_type: node_type(flags & 0x0f),
        location,
        name: (flags & HAS_NAME != 0).then(|| name(rest)).flatten(),
    })
}

fn coordinate(bytes: &[u8; 4]) -> f64 {
    f64::from(i32::from_le_bytes(*bytes)) / COORDINATE_SCALE
}

fn name(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim_matches('\0');
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn node_type(bits: u8) -> MeshcoreNodeType {
    match bits {
        0 => MeshcoreNodeType::None,
        1 => MeshcoreNodeType::Chat,
        2 => MeshcoreNodeType::Repeater,
        3 => MeshcoreNodeType::Room,
        4 => MeshcoreNodeType::Sensor,
        _ => MeshcoreNodeType::Other,
    }
}
