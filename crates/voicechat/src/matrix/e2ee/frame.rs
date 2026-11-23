//! LiveKit frame encryption of Opus frames, as livekit client-sdk-js `FrameCryptor` does it.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes128Gcm, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;

pub(crate) const KEY_RING: usize = 256;
const SALT: &[u8] = b"LKFrameEncryptionKey";
const KEY_INFO: [u8; 128] = [0; 128];
const IV_LEN: usize = 12;
const TRAILER_LEN: usize = 2;
const CLEAR_LEN: usize = 1;
const TAG_LEN: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum CryptoError {
    #[error("frame too short")]
    Short,
    #[error("no key at index {0}")]
    NoKey(u8),
    #[error("frame did not authenticate")]
    Auth,
    #[error("cannot derive key")]
    Derive,
}

/// An AES-128-GCM frame key derived from shared key material.
pub(crate) struct FrameKey(Aes128Gcm);

impl FrameKey {
    /// HKDF-SHA256 over `material` of any length, as the LiveKit key provider does.
    pub(crate) fn derive(material: &[u8]) -> Result<Self, CryptoError> {
        let mut key = [0u8; 16];
        Hkdf::<Sha256>::new(Some(SALT), material)
            .expand(&KEY_INFO, &mut key)
            .map_err(|_| CryptoError::Derive)?;
        Ok(Self(Aes128Gcm::new(&key.into())))
    }
}

pub(crate) struct KeyRing {
    keys: Box<[Option<FrameKey>; KEY_RING]>,
}

impl KeyRing {
    pub(crate) fn new() -> Self {
        Self {
            keys: Box::new(std::array::from_fn(|_| None)),
        }
    }

    pub(crate) fn set(&mut self, index: u8, key: FrameKey) {
        self.keys[usize::from(index)] = Some(key);
    }

    pub(crate) fn get(&self, index: u8) -> Option<&FrameKey> {
        self.keys[usize::from(index)].as_ref()
    }
}

/// Encrypts the frames of one stream.
pub(crate) struct Sealer {
    ssrc: u32,
    sent: u32,
}

impl Sealer {
    /// `start` is the first frame count; LiveKit starts it at a random number below 0xffff.
    pub(crate) fn new(ssrc: u32, start: u32) -> Self {
        Self { ssrc, sent: start }
    }

    pub(crate) fn seal(
        &mut self,
        key: &FrameKey,
        index: u8,
        timestamp: u32,
        opus: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let (clear, plain) = opus.split_at(opus.len().min(CLEAR_LEN));
        let iv = self.next_iv(timestamp);
        let cipher = key
            .0
            .encrypt(
                &Nonce::from(iv),
                Payload {
                    msg: plain,
                    aad: clear,
                },
            )
            .map_err(|_| CryptoError::Auth)?;
        let mut frame = Vec::with_capacity(clear.len() + cipher.len() + IV_LEN + TRAILER_LEN);
        frame.extend_from_slice(clear);
        frame.extend_from_slice(&cipher);
        frame.extend_from_slice(&iv);
        frame.extend_from_slice(&[IV_LEN as u8, index]);
        Ok(frame)
    }

    fn next_iv(&mut self, timestamp: u32) -> [u8; IV_LEN] {
        let count = timestamp.wrapping_sub(self.sent % 0xffff);
        self.sent = self.sent.wrapping_add(1);
        let mut iv = [0u8; IV_LEN];
        iv[..4].copy_from_slice(&self.ssrc.to_be_bytes());
        iv[4..8].copy_from_slice(&timestamp.to_be_bytes());
        iv[8..].copy_from_slice(&count.to_be_bytes());
        iv
    }
}

pub(crate) fn open(ring: &KeyRing, frame: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if frame.len() < CLEAR_LEN + TAG_LEN + IV_LEN + TRAILER_LEN {
        return Err(CryptoError::Short);
    }
    let (body, trailer) = frame.split_at(frame.len() - TRAILER_LEN);
    let index = trailer[1];
    if usize::from(trailer[0]) != IV_LEN {
        return Err(CryptoError::Short);
    }
    let key = ring.get(index).ok_or(CryptoError::NoKey(index))?;
    let (sealed, iv) = body.split_at(body.len() - IV_LEN);
    let (clear, cipher) = sealed.split_at(CLEAR_LEN);
    let iv: [u8; IV_LEN] = iv.try_into().map_err(|_| CryptoError::Short)?;
    let plain = key
        .0
        .decrypt(
            &Nonce::from(iv),
            Payload {
                msg: cipher,
                aad: clear,
            },
        )
        .map_err(|_| CryptoError::Auth)?;
    let mut opus = Vec::with_capacity(clear.len() + plain.len());
    opus.extend_from_slice(clear);
    opus.extend_from_slice(&plain);
    Ok(opus)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MATERIAL: &str = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
    const OPUS: &str =
        "01080f161d242b323940474e555c636a71787f868d949ba2a9b0b7bec5ccd3dae1e8eff6fd040b12";
    const FRAME: &str = "01252c9b5746779654994326dd0653942ad2af6cc1c24f3f452b5438240a18f73b9e7b101debd20af3f0904d1e2919d0467efe916c8c2b6201020304001122330011222c0c03";

    fn unhex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    fn ring(index: u8, material: &[u8]) -> KeyRing {
        let mut ring = KeyRing::new();
        ring.set(index, FrameKey::derive(material).unwrap());
        ring
    }

    fn sealed(ring: &KeyRing, index: u8, opus: &[u8]) -> Vec<u8> {
        Sealer::new(1, 0)
            .seal(ring.get(index).unwrap(), index, 960, opus)
            .unwrap()
    }

    #[test]
    fn a_frame_round_trips() {
        let ring = ring(2, b"secret");
        let opus: Vec<u8> = (0..80).collect();
        assert_eq!(open(&ring, &sealed(&ring, 2, &opus)).unwrap(), opus);
    }

    #[test]
    fn the_first_byte_stays_clear() {
        let ring = ring(0, b"secret");
        let opus = [0xfc, 1, 2, 3, 4];
        let frame = sealed(&ring, 0, &opus);
        assert_eq!(frame[0], 0xfc);
        assert_ne!(&frame[1..5], &opus[1..]);
        assert_eq!(frame[frame.len() - 2..], [12, 0]);
    }

    #[test]
    fn a_wrong_key_fails() {
        let frame = sealed(&ring(1, b"secret"), 1, &[1, 2, 3]);
        assert_eq!(open(&ring(1, b"other"), &frame), Err(CryptoError::Auth));
    }

    #[test]
    fn a_missing_index_names_it() {
        let frame = sealed(&ring(1, b"secret"), 1, &[1, 2, 3]);
        assert_eq!(
            open(&ring(2, b"secret"), &frame),
            Err(CryptoError::NoKey(1))
        );
    }

    #[test]
    fn a_short_frame_is_refused() {
        let ring = ring(0, b"secret");
        assert_eq!(open(&ring, &[]), Err(CryptoError::Short));
        assert_eq!(open(&ring, &[0; 30]), Err(CryptoError::Short));
    }

    #[test]
    fn a_tampered_frame_fails() {
        let ring = ring(0, b"secret");
        let mut frame = sealed(&ring, 0, &[1, 2, 3, 4]);
        frame[2] ^= 1;
        assert_eq!(open(&ring, &frame), Err(CryptoError::Auth));
    }

    #[test]
    fn livekit_vector_opens() {
        let ring = ring(3, &unhex(MATERIAL));
        assert_eq!(open(&ring, &unhex(FRAME)).unwrap(), unhex(OPUS));
    }

    #[test]
    fn sealing_matches_the_livekit_vector() {
        let ring = ring(3, &unhex(MATERIAL));
        let frame = Sealer::new(0x0102_0304, 7)
            .seal(ring.get(3).unwrap(), 3, 0x0011_2233, &unhex(OPUS))
            .unwrap();
        assert_eq!(frame, unhex(FRAME));
    }
}
