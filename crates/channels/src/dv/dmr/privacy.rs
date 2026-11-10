use blip25_vocoder::halfrate::frame::{Frame, pack_natural, unpack_natural};

use crate::crypto::{Aes, Rc4};

pub(crate) const FRAME_BYTES: usize = 7;
pub(crate) const ARC4_KEY_BYTES: usize = 5;
const FRAMES_PER_SUPERFRAME: usize = 18;
const SUPERFRAME_BYTES: usize = FRAMES_PER_SUPERFRAME * FRAME_BYTES;
const ARC4_DROP: usize = 256;
const AES_BLOCK: usize = 16;
const FRAMES_PER_BURST: usize = 3;
const MOTOROLA: u8 = 0x10;
const ALGORITHM_BITS: u8 = 0b111;
const ALGORITHM_ARC4: u8 = 1;
const ALGORITHM_AES_128: u8 = 4;
const ALGORITHM_AES_256: u8 = 5;
const MAX_KEYED_REPAIRS: u16 = 2;

/// The AMBE+2 frame a radio sends while nobody speaks, in natural bit order.
pub(crate) const SILENCE: [u8; FRAME_BYTES] = [0xF8, 0x01, 0xA9, 0x9F, 0x8C, 0xE0, 0x80];

/// Motorola Basic Privacy: one 16-bit key, masked and repeated over the first 48 bits of every
/// frame. The last bit stays clear.
pub(crate) fn basic_keystream(key: u16) -> [u8; FRAME_BYTES] {
    let stream = u64::from(key & 0xFF0F) << 32 | u64::from(key) << 16 | u64::from(key);
    let bytes = stream.to_be_bytes();
    [
        bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], 0,
    ]
}

/// Motorola masks silence like speech, so a masked silence frame carries the key three times
/// over. The two copies that agree with the first are 32 bits of proof no speech frame forges.
fn basic_key_from_silence(cipher: &[u8; FRAME_BYTES]) -> Option<u16> {
    let mask: [u8; FRAME_BYTES] = std::array::from_fn(|i| cipher[i] ^ SILENCE[i]);
    let high = u16::from_be_bytes([mask[0], mask[1]]);
    let middle = u16::from_be_bytes([mask[2], mask[3]]);
    let low = u16::from_be_bytes([mask[4], mask[5]]);
    let structured = low != 0 && middle == low && high == low & 0xFF0F && mask[6] & 0x80 == 0;
    structured.then_some(low)
}

/// The DMRA ARC4 message indicator of the next superframe: 32 steps of x^32 + x^4 + x^2 + 1.
pub(crate) fn next_message_indicator(mi: u32) -> u32 {
    (0..32).fold(mi, |lfsr, _| {
        let bit = (lfsr >> 31 ^ lfsr >> 3 ^ lfsr >> 1) & 1;
        lfsr << 1 | bit
    })
}

/// The DMRA AES IV: the message indicator, then 96 bits of x^32 + x^22 + x^2 + x + 1 run on it.
/// Its second word is the next superframe's message indicator.
pub(crate) fn aes_iv(mi: u32) -> [u8; AES_BLOCK] {
    let mut iv = [0; AES_BLOCK];
    iv[..4].copy_from_slice(&mi.to_be_bytes());
    let mut lfsr = mi;
    for position in 32..AES_BLOCK * 8 {
        let bit = (lfsr >> 31 ^ lfsr >> 21 ^ lfsr >> 1 ^ lfsr) & 1;
        lfsr = lfsr << 1 | bit;
        iv[position / 8] |= (bit as u8) << (7 - position % 8);
    }
    iv
}

/// An enhanced privacy key the operator holds, ready to run.
#[derive(Clone)]
pub(crate) enum CipherKey {
    Arc4([u8; ARC4_KEY_BYTES]),
    Aes128(Aes),
    Aes256(Aes),
}

impl CipherKey {
    /// Reads a key by its length: 10 hex digits for ARC4, 32 or 64 for AES.
    pub(crate) fn parse(hex: &str) -> Option<Self> {
        let bytes = crate::crypto::hex(hex)?;
        match bytes.len() {
            ARC4_KEY_BYTES => bytes.try_into().ok().map(Self::Arc4),
            16 => Aes::new(&bytes).map(Self::Aes128),
            32 => Aes::new(&bytes).map(Self::Aes256),
            _ => None,
        }
    }

    pub(crate) fn algorithm(&self) -> u8 {
        match self {
            Self::Arc4(_) => ALGORITHM_ARC4,
            Self::Aes128(_) => ALGORITHM_AES_128,
            Self::Aes256(_) => ALGORITHM_AES_256,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Arc4(_) => "ARC4",
            Self::Aes128(_) => "AES-128",
            Self::Aes256(_) => "AES-256",
        }
    }

    fn next_message_indicator(&self, mi: u32) -> u32 {
        match self {
            Self::Arc4(_) => next_message_indicator(mi),
            Self::Aes128(_) | Self::Aes256(_) => {
                let iv = aes_iv(mi);
                u32::from_be_bytes([iv[4], iv[5], iv[6], iv[7]])
            }
        }
    }

    /// The keystream of one superframe, seven bytes per vocoder frame. ARC4 drops its first 256
    /// bytes, AES in output feedback mode its first block.
    fn superframe(&self, mi: u32, stream: &mut [u8; SUPERFRAME_BYTES]) {
        stream.fill(0);
        match self {
            Self::Arc4(key) => {
                let mut seed = [0; ARC4_KEY_BYTES + 4];
                seed[..ARC4_KEY_BYTES].copy_from_slice(key);
                seed[ARC4_KEY_BYTES..].copy_from_slice(&mi.to_be_bytes());
                let mut cipher = Rc4::new(&seed);
                cipher.skip(ARC4_DROP);
                cipher.apply(stream);
            }
            Self::Aes128(aes) | Self::Aes256(aes) => {
                let mut block = aes_iv(mi);
                aes.encrypt(&mut block);
                for chunk in stream.chunks_mut(AES_BLOCK) {
                    aes.encrypt(&mut block);
                    chunk.copy_from_slice(&block[..chunk.len()]);
                }
            }
        }
    }
}

/// The keystream of a keyed call, a superframe at a time, stepping its message indicator along.
pub(crate) struct Keystream {
    key: CipherKey,
    mi: u32,
    stream: [u8; SUPERFRAME_BYTES],
}

impl Keystream {
    pub(crate) fn new(key: CipherKey, mi: u32) -> Self {
        let mut stream = [0; SUPERFRAME_BYTES];
        key.superframe(mi, &mut stream);
        Self { key, mi, stream }
    }

    pub(crate) fn next_superframe(&mut self) {
        self.mi = self.key.next_message_indicator(self.mi);
        self.key.superframe(self.mi, &mut self.stream);
    }

    /// The keystream of the `index`th vocoder frame of the current superframe.
    pub(crate) fn frame(&self, index: usize) -> [u8; FRAME_BYTES] {
        let at = index.min(FRAMES_PER_SUPERFRAME - 1) * FRAME_BYTES;
        std::array::from_fn(|i| self.stream[at + i])
    }
}

fn xor(bytes: &mut [u8; FRAME_BYTES], stream: &[u8; FRAME_BYTES]) {
    for (byte, mask) in bytes.iter_mut().zip(stream) {
        *byte ^= mask;
    }
}

enum Cipher {
    Unknown,
    Basic(Option<u16>),
    Keyed,
    Locked,
}

/// What a slot knows about how its current call is enciphered, and the state to undo it.
pub(super) struct Privacy {
    cipher: Cipher,
    keystream: Option<Keystream>,
    last_position: Option<usize>,
}

impl Privacy {
    pub(super) fn new() -> Self {
        Self {
            cipher: Cipher::Unknown,
            keystream: None,
            last_position: None,
        }
    }

    pub(super) fn reset(&mut self) {
        self.cipher = Cipher::Unknown;
        self.last_position = None;
    }

    /// Link control without a PI header: Motorola's encrypted voice user is Basic Privacy.
    pub(super) fn link_control(&mut self, encrypted: bool, fid: Option<u8>) {
        if !encrypted {
            self.reset();
            return;
        }
        if matches!(self.cipher, Cipher::Unknown) {
            self.cipher = if fid == Some(MOTOROLA) {
                Cipher::Basic(None)
            } else {
                Cipher::Locked
            };
        }
    }

    pub(super) fn pi_header(
        &mut self,
        algorithm: u8,
        key_id: u8,
        mi: u32,
        keys: &[(u8, CipherKey)],
    ) {
        let key = keys
            .iter()
            .find(|(id, key)| *id == key_id && key.algorithm() == algorithm & ALGORITHM_BITS);
        self.keystream = key.map(|(_, key)| Keystream::new(key.clone(), mi));
        self.cipher = if self.keystream.is_some() {
            Cipher::Keyed
        } else {
            Cipher::Locked
        };
        self.last_position = None;
    }

    /// Undoes the cipher on one vocoder frame, `frame` of burst `burst` in its superframe.
    /// Returns whether the frame is fit to play.
    pub(super) fn decipher(&mut self, burst: usize, frame: usize, decoded: &mut Frame) -> bool {
        let position = burst * FRAMES_PER_BURST + frame;
        let new_superframe = self.last_position.is_some_and(|last| position <= last);
        self.last_position = Some(position);
        let lost = decoded.errors[0] == u8::MAX;
        let mut bytes = pack_natural(&decoded.info);
        let playable = match &mut self.cipher {
            Cipher::Basic(key) => {
                if !lost
                    && decoded.error_total() == 0
                    && let Some(found) = basic_key_from_silence(&bytes)
                {
                    *key = Some(found);
                }
                if let Some(key) = key {
                    xor(&mut bytes, &basic_keystream(*key));
                }
                key.is_some()
            }
            Cipher::Keyed => {
                let Some(keystream) = &mut self.keystream else {
                    return false;
                };
                if new_superframe {
                    keystream.next_superframe();
                }
                if bytes != SILENCE {
                    xor(&mut bytes, &keystream.frame(position));
                }
                decoded.error_total() <= MAX_KEYED_REPAIRS
            }
            Cipher::Unknown | Cipher::Locked => false,
        };
        decoded.info = unpack_natural(&bytes);
        playable || lost
    }

    /// Names the cipher once the slot is undoing it.
    pub(super) fn label(&self) -> Option<String> {
        match &self.cipher {
            Cipher::Basic(Some(key)) => Some(format!("Basic Privacy {key:04X}")),
            Cipher::Keyed => self
                .keystream
                .as_ref()
                .map(|keystream| keystream.key.name().to_owned()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(bytes: [u8; FRAME_BYTES]) -> Frame {
        Frame {
            info: unpack_natural(&bytes),
            errors: [0; 4],
        }
    }

    fn masked(bytes: [u8; FRAME_BYTES], stream: [u8; FRAME_BYTES]) -> Frame {
        let mut bytes = bytes;
        xor(&mut bytes, &stream);
        clean(bytes)
    }

    #[test]
    fn the_basic_keystream_repeats_the_key_under_its_mask() {
        assert_eq!(
            basic_keystream(0x1F2E),
            [0x1F, 0x0E, 0x1F, 0x2E, 0x1F, 0x2E, 0]
        );
    }

    #[test]
    fn a_masked_silence_frame_gives_up_the_basic_key() {
        let mut privacy = Privacy::new();
        privacy.link_control(true, Some(MOTOROLA));
        let mut frame = masked(SILENCE, basic_keystream(0xE300));
        assert!(privacy.decipher(0, 0, &mut frame));
        assert_eq!(pack_natural(&frame.info), SILENCE);
        assert_eq!(privacy.label().as_deref(), Some("Basic Privacy E300"));
    }

    #[test]
    fn basic_privacy_stays_muted_until_silence_shows_the_key() {
        let mut privacy = Privacy::new();
        privacy.link_control(true, Some(MOTOROLA));
        let speech = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0x80];
        let mut frame = masked(speech, basic_keystream(0x2503));
        assert!(!privacy.decipher(0, 0, &mut frame));
        assert_eq!(privacy.label(), None);
    }

    #[test]
    fn a_clear_silence_frame_is_not_mistaken_for_a_key() {
        assert_eq!(basic_key_from_silence(&SILENCE), None);
    }

    #[test]
    fn only_motorola_link_control_implies_basic_privacy() {
        let mut privacy = Privacy::new();
        privacy.link_control(true, Some(0x68));
        let mut frame = masked(SILENCE, basic_keystream(0xE300));
        assert!(!privacy.decipher(0, 0, &mut frame));
    }

    #[test]
    fn the_message_indicator_steps_through_the_dmra_lfsr() {
        assert_eq!(next_message_indicator(0), 0);
        assert_eq!(next_message_indicator(1), 0x4514_5144);
        assert_eq!(next_message_indicator(0xDEAD_BEEF), 0xD65F_AFEB);
    }

    fn key(hex: &str) -> CipherKey {
        CipherKey::parse(hex).expect("key")
    }

    #[test]
    fn arc4_frames_follow_one_another_after_the_dropped_bytes() {
        let mut whole = Rc4::new(&[0x11, 0x22, 0x33, 0x44, 0x55, 0xDE, 0xAD, 0xBE, 0xEF]);
        whole.skip(ARC4_DROP + 2 * FRAME_BYTES);
        let mut expected = [0; FRAME_BYTES];
        whole.apply(&mut expected);
        let keystream = Keystream::new(key("1122334455"), 0xDEAD_BEEF);
        assert_eq!(keystream.frame(2), expected);
    }

    #[test]
    fn the_aes_iv_extends_the_message_indicator() {
        assert_eq!(
            aes_iv(0xDEAD_BEEF),
            [
                0xDE, 0xAD, 0xBE, 0xEF, 0x3B, 0x1E, 0xD8, 0x02, 0xB6, 0x93, 0xC3, 0xCD, 0x21, 0x7D,
                0x29, 0xD6
            ]
        );
    }

    #[test]
    fn aes_256_frames_run_in_output_feedback_after_one_dropped_block() {
        let keystream = Keystream::new(
            key("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"),
            0xDEAD_BEEF,
        );
        assert_eq!(
            keystream.frame(0),
            [0x6E, 0x9B, 0x78, 0x1A, 0x6D, 0x68, 0xEE]
        );
        assert_eq!(
            keystream.frame(1),
            [0x91, 0x12, 0x22, 0x2F, 0x48, 0x2F, 0x61]
        );
        assert_eq!(
            keystream.frame(2),
            [0xD5, 0x1C, 0x99, 0xF3, 0xB3, 0x85, 0x55]
        );
    }

    #[test]
    fn aes_steps_to_the_message_indicator_inside_its_iv() {
        let mut keystream = Keystream::new(key(&"ab".repeat(16)), 0xDEAD_BEEF);
        keystream.next_superframe();
        assert_eq!(
            keystream.frame(0),
            Keystream::new(key(&"ab".repeat(16)), 0x3B1E_D802).frame(0)
        );
    }

    #[test]
    fn keys_are_read_by_their_length() {
        assert_eq!(key("1122334455").algorithm(), ALGORITHM_ARC4);
        assert_eq!(key(&"00".repeat(16)).algorithm(), ALGORITHM_AES_128);
        assert_eq!(key(&"00".repeat(32)).algorithm(), ALGORITHM_AES_256);
        assert!(CipherKey::parse("11223344").is_none());
    }

    #[test]
    fn keyed_calls_send_silence_in_the_clear() {
        let mut privacy = Privacy::new();
        privacy.pi_header(0x21, 7, 0x0102_0304, &[(7, key("0102030405"))]);
        let mut frame = clean(SILENCE);
        assert!(privacy.decipher(0, 0, &mut frame));
        assert_eq!(pack_natural(&frame.info), SILENCE);
        assert_eq!(privacy.label().as_deref(), Some("ARC4"));
    }

    #[test]
    fn a_call_stays_locked_without_its_key_id_or_algorithm() {
        for (algorithm, key_id) in [(1, 9), (5, 7)] {
            let mut privacy = Privacy::new();
            privacy.pi_header(algorithm, key_id, 0x0102_0304, &[(7, key("0102030405"))]);
            let mut frame = clean(SILENCE);
            assert!(!privacy.decipher(0, 0, &mut frame));
            assert_eq!(privacy.label(), None);
        }
    }
}
