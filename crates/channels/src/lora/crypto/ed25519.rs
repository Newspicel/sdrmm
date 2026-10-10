mod field;
mod point;
mod scalar;

use point::Point;
use sha2::{Digest, Sha512};

pub(crate) fn verify(public_key: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
    let (r_bytes, s_bytes) = split_signature(signature);
    if !scalar::is_canonical(&s_bytes) {
        return false;
    }
    let (Some(public_point), Some(r_point)) = (Point::decode(public_key), Point::decode(&r_bytes))
    else {
        return false;
    };
    let challenge = challenge(&r_bytes, public_key, message);
    Point::base()
        .multiply(&s_bytes)
        .equals(r_point.add(public_point.multiply(&challenge)))
}

fn split_signature(signature: &[u8; 64]) -> ([u8; 32], [u8; 32]) {
    let mut r_bytes = [0u8; 32];
    let mut s_bytes = [0u8; 32];
    r_bytes.copy_from_slice(&signature[..32]);
    s_bytes.copy_from_slice(&signature[32..]);
    (r_bytes, s_bytes)
}

fn challenge(r_bytes: &[u8; 32], public_key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    scalar::reduce_wide(&sha512(&[r_bytes, public_key, message]))
}

fn sha512(parts: &[&[u8]]) -> [u8; 64] {
    let mut hasher = Sha512::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

#[cfg(any(test, feature = "synth"))]
fn expand_seed(seed: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let digest = sha512(&[seed]);
    let mut secret_scalar = [0u8; 32];
    let mut prefix = [0u8; 32];
    secret_scalar.copy_from_slice(&digest[..32]);
    prefix.copy_from_slice(&digest[32..]);
    secret_scalar[0] &= 0xf8;
    secret_scalar[31] &= 0x7f;
    secret_scalar[31] |= 0x40;
    (secret_scalar, prefix)
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn keypair(seed: &[u8; 32]) -> [u8; 32] {
    let (secret_scalar, _) = expand_seed(seed);
    Point::base().multiply(&secret_scalar).encode()
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn sign(seed: &[u8; 32], message: &[u8]) -> [u8; 64] {
    let base = Point::base();
    let (secret_scalar, prefix) = expand_seed(seed);
    let public_key = base.multiply(&secret_scalar).encode();
    let nonce = scalar::reduce_wide(&sha512(&[&prefix, message]));
    let r_bytes = base.multiply(&nonce).encode();
    let challenge = challenge(&r_bytes, &public_key, message);
    let s_bytes = scalar::multiply_add(&challenge, &secret_scalar, &nonce);
    let mut signature = [0u8; 64];
    signature[..32].copy_from_slice(&r_bytes);
    signature[32..].copy_from_slice(&s_bytes);
    signature
}

#[cfg(test)]
mod tests;
