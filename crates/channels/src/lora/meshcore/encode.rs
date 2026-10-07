use super::super::crypto;
use super::advert_data::{COORDINATE_SCALE, HAS_LOCATION, HAS_NAME, MAX_APP_DATA_LEN};
use super::frame::MAX_PAYLOAD_BYTES;
use super::group::{self, BLOCK_LEN, MAC_LEN};
use super::keys::channel_hash;

const FLOOD: u8 = 0x01;
const ADVERT: u8 = 0x04;
const GROUP_TEXT: u8 = 0x05;
const ZERO_HOPS: u8 = 0;
const MAX_GROUP_PLAINTEXT: usize = (MAX_PAYLOAD_BYTES - 1 - MAC_LEN) / BLOCK_LEN * BLOCK_LEN;

pub(crate) fn advert(
    seed: &[u8; 32],
    timestamp: u32,
    node_type: u8,
    name: &str,
    location: Option<(f64, f64)>,
) -> Vec<u8> {
    let public_key = crypto::ed25519_keypair(seed);
    let app_data = app_data(node_type, name, location);
    let message = [public_key.as_slice(), &timestamp.to_le_bytes(), &app_data].concat();
    let signature = crypto::ed25519_sign(seed, &message);
    let payload = [
        public_key.as_slice(),
        &timestamp.to_le_bytes(),
        &signature,
        &app_data,
    ]
    .concat();
    flood_packet(ADVERT, &payload)
}

pub(crate) fn group_text(secret: &[u8], timestamp: u32, sender: &str, text: &str) -> Vec<u8> {
    let mut plaintext = timestamp.to_le_bytes().to_vec();
    plaintext.push(0);
    plaintext.extend_from_slice(format!("{sender}: {text}").as_bytes());
    group_datagram(GROUP_TEXT, secret, &plaintext)
}

pub(super) fn group_datagram(payload_type: u8, secret: &[u8], plaintext: &[u8]) -> Vec<u8> {
    let plaintext = plaintext.get(..MAX_GROUP_PLAINTEXT).unwrap_or(plaintext);
    let ciphertext = encrypt(secret, plaintext);
    let mac = group::mac(secret, &ciphertext);
    let payload = [&[channel_hash(secret)], mac.as_slice(), &ciphertext].concat();
    flood_packet(payload_type, &payload)
}

fn flood_packet(payload_type: u8, payload: &[u8]) -> Vec<u8> {
    [&[(payload_type << 2) | FLOOD, ZERO_HOPS], payload].concat()
}

fn app_data(node_type: u8, name: &str, location: Option<(f64, f64)>) -> Vec<u8> {
    let mut data = vec![node_type & 0x0f];
    if let Some((lat, lon)) = location {
        data[0] |= HAS_LOCATION;
        data.extend_from_slice(&scaled(lat).to_le_bytes());
        data.extend_from_slice(&scaled(lon).to_le_bytes());
    }
    let name = utf8_prefix(name, MAX_APP_DATA_LEN.saturating_sub(data.len()));
    if !name.is_empty() {
        data[0] |= HAS_NAME;
        data.extend_from_slice(name.as_bytes());
    }
    data
}

fn scaled(degrees: f64) -> i32 {
    (degrees * COORDINATE_SCALE).round() as i32
}

fn utf8_prefix(text: &str, max_len: usize) -> &str {
    let end = (0..=max_len.min(text.len()))
        .rev()
        .find(|&end| text.is_char_boundary(end))
        .unwrap_or(0);
    text.get(..end).unwrap_or_default()
}

fn encrypt(secret: &[u8], plaintext: &[u8]) -> Vec<u8> {
    let Some(aes) = group::cipher(secret) else {
        return Vec::new();
    };
    let mut ciphertext = Vec::with_capacity(plaintext.len().next_multiple_of(BLOCK_LEN));
    for chunk in plaintext.chunks(BLOCK_LEN) {
        let mut block = [0; BLOCK_LEN];
        block
            .iter_mut()
            .zip(chunk)
            .for_each(|(slot, byte)| *slot = *byte);
        aes.encrypt(&mut block);
        ciphertext.extend_from_slice(&block);
    }
    ciphertext
}
