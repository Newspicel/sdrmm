use sdrmm_wire::LoraKey;

use super::super::crypto;

pub(crate) const PUBLIC_SECRET: [u8; 16] = [
    0x8b, 0x33, 0x87, 0xe9, 0xc5, 0xcd, 0xea, 0x6a, 0xc9, 0xe5, 0xed, 0xba, 0xa1, 0x15, 0xcd, 0x72,
];
const PUBLIC_NAME: &str = "Public";
const SHORT_SECRET_LEN: usize = 16;
pub(super) const LONG_SECRET_LEN: usize = 32;

pub(super) struct Channel {
    pub(super) name: String,
    pub(super) secret: Vec<u8>,
    pub(super) hash: u8,
}

impl Channel {
    pub(super) fn new(name: &str, secret: &[u8]) -> Self {
        Self {
            name: name.to_owned(),
            secret: secret.to_vec(),
            hash: channel_hash(secret),
        }
    }
}

pub(crate) struct Keys {
    channels: Vec<Channel>,
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            channels: vec![Channel::new(PUBLIC_NAME, &PUBLIC_SECRET)],
        }
    }
}

impl Keys {
    pub(crate) fn new(keys: &[LoraKey]) -> Result<Self, String> {
        let mut parsed = Self::default();
        for key in keys {
            if let LoraKey::MeshcoreChannel { name, secret } = key {
                parsed.channels.push(user_channel(name, secret)?);
            }
        }
        Ok(parsed)
    }

    pub(super) fn matching(&self, hash: u8) -> impl Iterator<Item = &Channel> {
        self.channels
            .iter()
            .filter(move |channel| channel.hash == hash)
    }
}

pub(super) fn channel_hash(secret: &[u8]) -> u8 {
    crypto::sha256(&[secret])[0]
}

pub(crate) fn hashtag_secret(name: &str) -> [u8; SHORT_SECRET_LEN] {
    let digest = crypto::sha256(&[name.as_bytes()]);
    let mut secret = [0; SHORT_SECRET_LEN];
    secret.copy_from_slice(&digest[..SHORT_SECRET_LEN]);
    secret
}

fn user_channel(name: &str, secret: &str) -> Result<Channel, String> {
    let name = name.trim();
    let secret = secret.trim();
    if secret.is_empty() && name.starts_with('#') && name.len() > 1 {
        return Ok(Channel::new(name, &hashtag_secret(name)));
    }
    let bytes = parse_secret(secret)
        .ok_or_else(|| format!("MeshCore channel {name}: secret must be 16 or 32 bytes"))?;
    Ok(Channel::new(name, &bytes))
}

fn parse_secret(text: &str) -> Option<Vec<u8>> {
    let valid_len = |bytes: &Vec<u8>| matches!(bytes.len(), SHORT_SECRET_LEN | LONG_SECRET_LEN);
    let is_hex = matches!(text.len(), 32 | 64) && text.chars().all(|c| c.is_ascii_hexdigit());
    if is_hex {
        return crypto::hex(text).filter(valid_len);
    }
    crypto::base64(text).filter(valid_len)
}
