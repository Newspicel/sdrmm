use sdrmm_wire::MeshcoreContent;

use super::super::crypto::{self, Aes};
use super::keys::{Channel, Keys, LONG_SECRET_LEN};
use crate::datalink::hex;

pub(super) const MAC_LEN: usize = 2;
pub(super) const BLOCK_LEN: usize = 16;
const AES_KEY_LEN: usize = 16;
const TEXT_HEADER_LEN: usize = 5;
const DATA_HEADER_LEN: usize = 3;

pub(super) struct Sealed<'a> {
    pub(super) channel_hash: u8,
    pub(super) mac: &'a [u8; MAC_LEN],
    pub(super) ciphertext: &'a [u8],
}

impl<'a> Sealed<'a> {
    pub(super) fn parse(payload: &'a [u8]) -> Option<Self> {
        let (&channel_hash, rest) = payload.split_first()?;
        let (mac, ciphertext) = rest.split_first_chunk::<MAC_LEN>()?;
        is_block_aligned(ciphertext).then_some(Self {
            channel_hash,
            mac,
            ciphertext,
        })
    }

    fn open<'k>(&self, keys: &'k Keys) -> Option<(&'k Channel, Vec<u8>)> {
        keys.matching(self.channel_hash).find_map(|channel| {
            let plaintext = open_with(&channel.secret, self.mac, self.ciphertext)?;
            Some((channel, plaintext))
        })
    }
}

pub(super) fn is_block_aligned(ciphertext: &[u8]) -> bool {
    !ciphertext.is_empty() && ciphertext.len().is_multiple_of(BLOCK_LEN)
}

fn padded_key(secret: &[u8]) -> [u8; LONG_SECRET_LEN] {
    let mut key = [0; LONG_SECRET_LEN];
    for (slot, byte) in key.iter_mut().zip(secret) {
        *slot = *byte;
    }
    key
}

pub(super) fn mac(secret: &[u8], ciphertext: &[u8]) -> [u8; MAC_LEN] {
    let digest = crypto::hmac_sha256(&padded_key(secret), ciphertext);
    [digest[0], digest[1]]
}

pub(super) fn cipher(secret: &[u8]) -> Option<Aes> {
    Aes::new(secret.get(..AES_KEY_LEN)?)
}

fn open_with(secret: &[u8], expected_mac: &[u8; MAC_LEN], ciphertext: &[u8]) -> Option<Vec<u8>> {
    if mac(secret, ciphertext) != *expected_mac {
        return None;
    }
    let aes = cipher(secret)?;
    let mut plaintext = Vec::with_capacity(ciphertext.len());
    for chunk in ciphertext.as_chunks::<BLOCK_LEN>().0 {
        let mut block = *chunk;
        aes.decrypt(&mut block);
        plaintext.extend_from_slice(&block);
    }
    Some(plaintext)
}

pub(super) fn text(sealed: &Sealed<'_>, keys: &Keys) -> MeshcoreContent {
    let Some((channel, plaintext)) = sealed.open(keys) else {
        return MeshcoreContent::GroupText {
            channel_hash: sealed.channel_hash,
            channel: None,
            timestamp: None,
            sender: None,
            text: None,
        };
    };
    let timestamp = plaintext
        .first_chunk::<4>()
        .map(|bytes| u32::from_le_bytes(*bytes));
    let message = plaintext.get(TEXT_HEADER_LEN..).map(until_nul);
    let (sender, text) = message.map_or((None, None), split_sender);
    MeshcoreContent::GroupText {
        channel_hash: sealed.channel_hash,
        channel: Some(channel.name.clone()),
        timestamp,
        sender,
        text,
    }
}

pub(super) fn data(sealed: &Sealed<'_>, keys: &Keys) -> MeshcoreContent {
    let Some((channel, plaintext)) = sealed.open(keys) else {
        return MeshcoreContent::GroupData {
            channel_hash: sealed.channel_hash,
            channel: None,
            data_type: None,
            data: None,
        };
    };
    let data_type = plaintext
        .first_chunk::<2>()
        .map(|bytes| u16::from_le_bytes(*bytes));
    let data = plaintext.get(2).and_then(|&len| {
        plaintext
            .get(DATA_HEADER_LEN..DATA_HEADER_LEN + usize::from(len))
            .map(hex)
    });
    MeshcoreContent::GroupData {
        channel_hash: sealed.channel_hash,
        channel: Some(channel.name.clone()),
        data_type,
        data,
    }
}

fn until_nul(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(bytes.get(..end).unwrap_or_default()).into_owned()
}

fn split_sender(message: String) -> (Option<String>, Option<String>) {
    match message.split_once(": ") {
        Some((sender, text)) => (Some(sender.to_owned()), Some(text.to_owned())),
        None => (None, Some(message)),
    }
}
