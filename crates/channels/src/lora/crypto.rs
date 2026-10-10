mod aes;
mod ed25519;
mod text;

#[cfg(test)]
mod tests;

use sha2::{Digest, Sha256};

pub(crate) use aes::Aes;
pub(crate) use ed25519::verify as ed25519_verify;
#[cfg(any(test, feature = "synth"))]
pub(crate) use ed25519::{keypair as ed25519_keypair, sign as ed25519_sign};
pub(crate) use text::{base64, hex};

const AES_BLOCK: usize = 16;
const SHA256_BLOCK: usize = 64;

pub(crate) fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

pub(crate) fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut block_key = [0u8; SHA256_BLOCK];
    if key.len() > SHA256_BLOCK {
        block_key[..32].copy_from_slice(&sha256(&[key]));
    } else {
        block_key[..key.len()].copy_from_slice(key);
    }
    let inner_pad = block_key.map(|byte| byte ^ 0x36);
    let outer_pad = block_key.map(|byte| byte ^ 0x5c);
    let inner = sha256(&[&inner_pad, message]);
    sha256(&[&outer_pad, &inner])
}

pub(crate) fn cmac(aes: &Aes, message: &[u8]) -> [u8; 16] {
    let mut subkey = [0u8; AES_BLOCK];
    aes.encrypt(&mut subkey);
    let first_subkey = double_block(subkey);
    let second_subkey = double_block(first_subkey);
    let split = message.len().saturating_sub(1) / AES_BLOCK * AES_BLOCK;
    let (head, tail) = message.split_at(split);
    let mut state = [0u8; AES_BLOCK];
    for chunk in head.as_chunks::<AES_BLOCK>().0 {
        xor_into(&mut state, chunk);
        aes.encrypt(&mut state);
    }
    let mut last = [0u8; AES_BLOCK];
    last[..tail.len()].copy_from_slice(tail);
    if tail.len() == AES_BLOCK {
        xor_into(&mut last, &first_subkey);
    } else {
        last[tail.len()] = 0x80;
        xor_into(&mut last, &second_subkey);
    }
    xor_into(&mut state, &last);
    aes.encrypt(&mut state);
    state
}

fn double_block(block: [u8; AES_BLOCK]) -> [u8; AES_BLOCK] {
    let value = u128::from_be_bytes(block);
    let reduction = if value >> 127 != 0 { 0x87 } else { 0 };
    ((value << 1) ^ reduction).to_be_bytes()
}

fn xor_into(target: &mut [u8], source: &[u8]) {
    for (byte, other) in target.iter_mut().zip(source) {
        *byte ^= other;
    }
}

pub(crate) fn ctr(aes: &Aes, nonce: &[u8; 16], data: &mut [u8]) {
    let mut counter = *nonce;
    for chunk in data.chunks_mut(AES_BLOCK) {
        let mut keystream = counter;
        aes.encrypt(&mut keystream);
        xor_into(chunk, &keystream);
        increment_counter(&mut counter);
    }
}

fn increment_counter(counter: &mut [u8; AES_BLOCK]) {
    let mut low = [0u8; 4];
    low.copy_from_slice(&counter[12..]);
    let next = u32::from_be_bytes(low).wrapping_add(1);
    counter[12..].copy_from_slice(&next.to_be_bytes());
}
