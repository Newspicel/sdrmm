mod content;
mod ports;
mod proto;
#[cfg(any(test, feature = "synth"))]
mod synth;
mod telemetry;
#[cfg(test)]
mod tests;

use sdrmm_wire::{LoraKey, MeshtasticEncryption, MeshtasticPacket};

use self::content::Data;
use super::crypto::{self, Aes};

#[cfg(any(test, feature = "synth"))]
pub(crate) use synth::{position, text};

const HEADER_LEN: usize = 16;
const PKI_OVERHEAD: usize = 12;
const DEFAULT_PSK: [u8; 16] = [
    0xd4, 0xf1, 0xbb, 0x3a, 0x20, 0x29, 0x07, 0x59, 0xf0, 0xbc, 0xff, 0xab, 0xcf, 0x4e, 0x69, 0x01,
];
const DEFAULT_PSK_INDEX: u8 = 1;
const PRESET_CHANNELS: [&str; 9] = [
    "LongFast",
    "MediumFast",
    "ShortFast",
    "LongSlow",
    "MediumSlow",
    "ShortSlow",
    "ShortTurbo",
    "LongTurbo",
    "LongMod",
];
const HOP_LIMIT: u8 = 0x07;
const WANT_ACK: u8 = 0x08;
const VIA_MQTT: u8 = 0x10;
const HOP_START_SHIFT: u8 = 5;

struct Channel {
    name: String,
    hash: u8,
    cipher: Option<Aes>,
}

impl Channel {
    fn new(name: &str, psk: &[u8]) -> Option<Self> {
        let key = expand_psk(psk)?;
        let cipher = if key.is_empty() {
            None
        } else {
            Some(Aes::new(&key)?)
        };
        Some(Self {
            name: name.to_owned(),
            hash: channel_hash(name, &key),
            cipher,
        })
    }

    fn open(&self, header: &Header, body: &[u8]) -> Option<Data> {
        if self.hash != header.channel_hash {
            return None;
        }
        let mut plain = body.to_vec();
        if let Some(cipher) = &self.cipher {
            crypto::ctr(cipher, &nonce(header.id, header.from), &mut plain);
        }
        content::data(&plain)
    }

    fn encryption(&self) -> MeshtasticEncryption {
        if self.cipher.is_some() {
            MeshtasticEncryption::Channel
        } else {
            MeshtasticEncryption::Open
        }
    }
}

pub(crate) struct Keys {
    channels: Vec<Channel>,
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            channels: PRESET_CHANNELS
                .iter()
                .filter_map(|name| Channel::new(name, &[DEFAULT_PSK_INDEX]))
                .collect(),
        }
    }
}

impl Keys {
    pub(crate) fn new(keys: &[LoraKey]) -> Result<Self, String> {
        let mut parsed = Self::default();
        for key in keys {
            if let LoraKey::MeshtasticChannel { name, psk } = key {
                parsed.channels.push(user_channel(name, psk)?);
            }
        }
        Ok(parsed)
    }
}

fn user_channel(name: &str, psk: &str) -> Result<Channel, String> {
    let psk = psk.trim();
    let bytes = if psk.is_empty() {
        Vec::new()
    } else {
        crypto::base64(psk)
            .ok_or_else(|| format!("Meshtastic channel {name}: PSK is not base64"))?
    };
    Channel::new(name, &bytes)
        .ok_or_else(|| format!("Meshtastic channel {name}: PSK is longer than 32 bytes"))
}

fn expand_psk(psk: &[u8]) -> Option<Vec<u8>> {
    match psk {
        [] | [0] => Some(Vec::new()),
        [index] => {
            let mut key = DEFAULT_PSK.to_vec();
            if let Some(last) = key.last_mut() {
                *last = last.wrapping_add(index - 1);
            }
            Some(key)
        }
        _ if psk.len() <= 16 => Some(padded(psk, 16)),
        _ if psk.len() <= 32 => Some(padded(psk, 32)),
        _ => None,
    }
}

fn padded(psk: &[u8], len: usize) -> Vec<u8> {
    let mut key = psk.to_vec();
    key.resize(len, 0);
    key
}

fn xor_all(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |hash, byte| hash ^ byte)
}

fn channel_hash(name: &str, key: &[u8]) -> u8 {
    xor_all(name.as_bytes()) ^ xor_all(key)
}

fn nonce(id: u32, from: u32) -> [u8; 16] {
    let mut nonce = [0; 16];
    nonce[..8].copy_from_slice(&u64::from(id).to_le_bytes());
    nonce[8..12].copy_from_slice(&from.to_le_bytes());
    nonce
}

struct Header {
    to: u32,
    from: u32,
    id: u32,
    flags: u8,
    channel_hash: u8,
    next_hop: u8,
    relay_node: u8,
}

fn header(bytes: &[u8]) -> Option<Header> {
    let word = |at: usize| {
        bytes
            .get(at..at + 4)
            .and_then(|word| word.try_into().ok())
            .map(u32::from_le_bytes)
    };
    Some(Header {
        to: word(0)?,
        from: word(4)?,
        id: word(8)?,
        flags: *bytes.get(12)?,
        channel_hash: *bytes.get(13)?,
        next_hop: *bytes.get(14)?,
        relay_node: *bytes.get(15)?,
    })
}

fn packet(header: &Header) -> MeshtasticPacket {
    MeshtasticPacket {
        to: header.to,
        from: header.from,
        id: header.id,
        hop_limit: header.flags & HOP_LIMIT,
        hop_start: header.flags >> HOP_START_SHIFT,
        want_ack: header.flags & WANT_ACK != 0,
        via_mqtt: header.flags & VIA_MQTT != 0,
        channel_hash: header.channel_hash,
        next_hop: header.next_hop,
        relay_node: header.relay_node,
        encryption: MeshtasticEncryption::Unknown,
        channel: None,
        port: None,
        port_name: None,
        request_id: None,
        reply_id: None,
        content: None,
    }
}

fn looks_like_pki(header: &Header, body: &[u8]) -> bool {
    header.channel_hash == 0
        && header.to != MeshtasticPacket::BROADCAST
        && body.len() > PKI_OVERHEAD
}

pub(crate) fn decode(payload: &[u8], keys: &Keys) -> Option<MeshtasticPacket> {
    let header = header(payload)?;
    let body = payload.get(HEADER_LEN..)?;
    let mut packet = packet(&header);
    let opened = keys
        .channels
        .iter()
        .find_map(|channel| channel.open(&header, body).map(|data| (channel, data)));
    match opened {
        Some((channel, data)) => {
            packet.encryption = channel.encryption();
            packet.channel = Some(channel.name.clone());
            packet.port = Some(data.port);
            packet.port_name = ports::port_name(data.port).map(str::to_owned);
            packet.request_id = data.request_id;
            packet.reply_id = data.reply_id;
            packet.content = Some(data.content);
        }
        None if looks_like_pki(&header, body) => packet.encryption = MeshtasticEncryption::Pki,
        None => {}
    }
    Some(packet)
}
