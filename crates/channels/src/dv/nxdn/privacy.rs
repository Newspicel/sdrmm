use blip25_vocoder::halfrate::{pack_natural, unpack_natural};

use crate::crypto::{Aes, Des, aes_iv, hex, ofb};
use crate::dv::vocoder::HalfRateFrame;

pub(crate) const CIPHER_CLEAR: u8 = 0;
const CIPHER_SCRAMBLER: u8 = 1;
const CIPHER_DES: u8 = 2;
const CIPHER_AES: u8 = 3;

pub(crate) const FRAME_BITS: usize = 49;
pub(crate) const FRAMES_PER_SUPERFRAME: usize = 16;
/// One IV keys two superframes.
const SUPERFRAMES_PER_IV: usize = 2;
const STREAM_BITS: usize = SUPERFRAMES_PER_IV * FRAMES_PER_SUPERFRAME * FRAME_BITS;
pub(crate) const STREAM_BYTES: usize = STREAM_BITS.div_ceil(8);
const SCRAMBLER_MASK: u16 = 0x7FFF;
const DES_BLOCK: usize = 8;
const AES_BLOCK: usize = 16;
const OFB_SCRATCH: usize = AES_BLOCK + STREAM_BYTES;

/// An NXDN voice key, its cipher read off its length: up to 4 hex digits for the scrambler, 16
/// for DES-OFB and 64 for AES-256.
pub(crate) enum CipherKey {
    Scrambler(u16),
    Des(Des),
    Aes(Aes),
}

impl CipherKey {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if (1..=4).contains(&text.len()) {
            let seed = u16::from_str_radix(text, 16).ok()?;
            return (seed & SCRAMBLER_MASK != 0 && seed <= SCRAMBLER_MASK)
                .then_some(Self::Scrambler(seed));
        }
        let bytes = hex(text)?;
        match bytes.len() {
            DES_BLOCK => Some(Self::Des(Des::new(bytes.try_into().ok()?))),
            32 => Aes::new(&bytes).map(Self::Aes),
            _ => None,
        }
    }

    pub(crate) fn cipher(&self) -> u8 {
        match self {
            Self::Scrambler(_) => CIPHER_SCRAMBLER,
            Self::Des(_) => CIPHER_DES,
            Self::Aes(_) => CIPHER_AES,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Scrambler(_) => "Scrambler",
            Self::Des(_) => "DES-OFB",
            Self::Aes(_) => "AES-256",
        }
    }

    /// The keystream bits from the start of a superframe (scrambler) or an IV (DES, AES).
    pub(crate) fn stream(&self, iv: u64, stream: &mut [u8; STREAM_BYTES]) {
        match self {
            Self::Scrambler(seed) => scrambler(*seed, stream),
            Self::Des(des) => {
                let mut scratch = [0u8; OFB_SCRATCH];
                ofb(|block| des.encrypt(block), iv.to_be_bytes(), &mut scratch);
                stream.copy_from_slice(&scratch[DES_BLOCK..DES_BLOCK + STREAM_BYTES]);
            }
            Self::Aes(aes) => {
                let mut scratch = [0u8; OFB_SCRATCH];
                ofb(|block| aes.encrypt(block), aes_iv(iv), &mut scratch);
                stream.copy_from_slice(&scratch[AES_BLOCK..AES_BLOCK + STREAM_BYTES]);
            }
        }
    }
}

/// The voice scrambler: the 15-bit LFSR x^15 + x^14 + 1 seeded with the key, its low bit out.
fn scrambler(seed: u16, stream: &mut [u8; STREAM_BYTES]) {
    stream.fill(0);
    let mut lfsr = seed & SCRAMBLER_MASK;
    for bit in 0..STREAM_BITS {
        stream[bit / 8] |= ((lfsr & 1) as u8) << (7 - bit % 8);
        let feedback = (lfsr >> 1 ^ lfsr) & 1;
        lfsr = lfsr >> 1 | feedback << 14;
    }
}

pub(crate) type CipherKeys = Vec<(u8, CipherKey)>;

/// The NXDN voice privacy of one call: the voice call message names the cipher and key, VCALL_IV
/// the IV of the next two superframes, and each voice frame takes 49 keystream bits by its place.
pub(super) struct Privacy {
    call: Option<(u8, u8)>,
    pending_iv: Option<u64>,
    key: Option<usize>,
    superframe: usize,
    stream: [u8; STREAM_BYTES],
}

impl Privacy {
    pub(super) fn new() -> Self {
        Self {
            call: None,
            pending_iv: None,
            key: None,
            superframe: 0,
            stream: [0; STREAM_BYTES],
        }
    }

    pub(super) fn reset(&mut self) {
        self.call = None;
        self.pending_iv = None;
        self.key = None;
    }

    /// The cipher and key ID of a voice call message; cipher 0 is a clear call.
    pub(super) fn call(&mut self, cipher: u8, key_id: u8) {
        if cipher == CIPHER_CLEAR {
            self.reset();
        } else if self.call != Some((cipher, key_id)) {
            self.call = Some((cipher, key_id));
            self.key = None;
        }
    }

    pub(super) fn encrypted(&self) -> Option<bool> {
        self.call.map(|_| true)
    }

    pub(super) fn iv(&mut self, iv: u64) {
        self.pending_iv = Some(iv);
    }

    /// A new superframe begins: the scrambler starts over, and DES or AES takes a pending IV or
    /// moves on to the second superframe of the last one.
    pub(super) fn superframe(&mut self, keys: &CipherKeys) {
        let Some((cipher, key_id)) = self.call else {
            return;
        };
        let found = keys
            .iter()
            .position(|(id, key)| *id == key_id && key.cipher() == cipher);
        if cipher == CIPHER_SCRAMBLER {
            self.key = found;
            self.superframe = 0;
            if let Some(index) = found {
                keys[index].1.stream(0, &mut self.stream);
            }
            return;
        }
        if let Some(iv) = self.pending_iv.take() {
            self.key = found;
            self.superframe = 0;
            if let Some(index) = found {
                keys[index].1.stream(iv, &mut self.stream);
            }
        } else if self.key.is_some() {
            self.superframe += 1;
            if self.superframe >= SUPERFRAMES_PER_IV {
                self.key = None;
            }
        }
    }

    /// Undoes the cipher on the voice frame at `position` (0 to 15) of the superframe. False while
    /// the call stays muted for want of its key or IV.
    pub(super) fn decipher(&self, position: usize, frame: &mut HalfRateFrame) -> bool {
        if self.key.is_none() {
            return false;
        }
        let start = (self.superframe * FRAMES_PER_SUPERFRAME + position) * FRAME_BITS;
        let mut bytes = pack_natural(&frame.info);
        for bit in 0..FRAME_BITS {
            let at = start + bit;
            let key = self.stream[at / 8] >> (7 - at % 8) & 1;
            bytes[bit / 8] ^= key << (7 - bit % 8);
        }
        frame.info = unpack_natural(&bytes);
        true
    }

    pub(super) fn label(&self, keys: &CipherKeys) -> Option<String> {
        self.key.map(|index| keys[index].1.name().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> CipherKey {
        CipherKey::parse(text).expect("key")
    }

    fn silent_frame() -> HalfRateFrame {
        HalfRateFrame {
            info: [0; 4],
            errors: [0; 4],
        }
    }

    #[test]
    fn keys_take_their_cipher_from_their_length() {
        assert_eq!(key("0001").cipher(), CIPHER_SCRAMBLER);
        assert_eq!(key("ABCDEF0123456789").cipher(), CIPHER_DES);
        assert_eq!(key(&"AB".repeat(32)).cipher(), CIPHER_AES);
        assert!(CipherKey::parse("0000").is_none());
        assert!(CipherKey::parse("8000").is_none());
        assert!(CipherKey::parse(&"AB".repeat(16)).is_none());
    }

    #[test]
    fn the_scrambler_shifts_its_low_bit_out_first() {
        let mut stream = [0u8; STREAM_BYTES];
        scrambler(1, &mut stream);
        assert_eq!(stream[0], 0b1000_0000);
        assert_eq!(stream[1], 0b0000_0001);
    }

    #[test]
    fn the_scrambler_starts_over_every_superframe() {
        let keys = vec![(1, key("0001"))];
        let mut privacy = Privacy::new();
        privacy.call(CIPHER_SCRAMBLER, 1);
        privacy.superframe(&keys);
        let mut first = silent_frame();
        assert!(privacy.decipher(0, &mut first));
        privacy.superframe(&keys);
        let mut again = silent_frame();
        assert!(privacy.decipher(0, &mut again));
        assert_eq!(first.info, again.info);
    }

    #[test]
    fn des_and_aes_wait_for_an_iv_and_last_two_superframes() {
        let keys = vec![(1, key("ABCDEF0123456789"))];
        let mut privacy = Privacy::new();
        privacy.call(CIPHER_DES, 1);
        privacy.superframe(&keys);
        assert!(!privacy.decipher(0, &mut silent_frame()));
        privacy.iv(0x0123_4567_89AB_CDEF);
        privacy.superframe(&keys);
        assert!(privacy.decipher(0, &mut silent_frame()));
        assert_eq!(privacy.label(&keys).as_deref(), Some("DES-OFB"));
        privacy.superframe(&keys);
        assert!(privacy.decipher(15, &mut silent_frame()));
        privacy.superframe(&keys);
        assert!(!privacy.decipher(0, &mut silent_frame()));
    }

    #[test]
    fn a_call_without_its_key_stays_muted() {
        let keys = vec![(2, key("0001"))];
        let mut privacy = Privacy::new();
        privacy.call(CIPHER_SCRAMBLER, 1);
        privacy.superframe(&keys);
        assert!(!privacy.decipher(0, &mut silent_frame()));
        assert_eq!(privacy.encrypted(), Some(true));
    }

    #[test]
    #[ignore = "needs KNOWN_KEY_SAMPLES=<known-key-mbe-samples checkout>"]
    fn known_key_samples_decipher() {
        use crate::dv::known_keys::{Sample, samples, tone_matches};
        use crate::dv::vocoder::{
            AMBE_3600_INTERLEAVE, MbeDecoder, half_frame, half_rate_code_vectors,
        };
        use crate::{AUDIO_RATE, ChannelOutputs};

        fn decode(sample: &Sample, key_text: &str) -> (f64, bool) {
            let key_id = sample.key_id.expect("key id") as u8;
            let keys = vec![(key_id, key(key_text))];
            let mut privacy = Privacy::new();
            let mut vocoder = MbeDecoder::half_rate();
            let mut out = ChannelOutputs::default();
            let mut quarter = 0;
            for (index, frame) in sample.frames.iter().enumerate() {
                if let (Some(cipher), Some(key_id)) = (frame.algorithm, frame.key_id) {
                    privacy.call(cipher, key_id as u8);
                }
                if let Some(iv) = frame.mi {
                    privacy.iv(iv);
                }
                if let Some(tag) = &frame.tag {
                    quarter = tag
                        .trim_start_matches("SACCH ")
                        .parse::<usize>()
                        .expect("sacch tag")
                        - 1;
                    if quarter == 0 {
                        privacy.superframe(&keys);
                    }
                }
                let bits: [bool; 72] =
                    std::array::from_fn(|i| frame.bytes[i / 8] >> (7 - i % 8) & 1 == 1);
                let mut decoded = half_frame(half_rate_code_vectors(&bits, &AMBE_3600_INTERLEAVE));
                if privacy.decipher(quarter * 4 + index % 4, &mut decoded) {
                    vocoder.play_half(&decoded, &mut out);
                } else {
                    MbeDecoder::mute(&mut out);
                }
            }
            tone_matches(sample, &out.audio_pcm, AUDIO_RATE as usize)
        }

        let all = samples("nxdn_");
        assert!(!all.is_empty(), "no nxdn samples");
        let mut failed = false;
        for sample in &all {
            let right = decode(sample, &sample.key);
            let mut wrong = sample.key.clone();
            wrong.replace_range(
                wrong.len() - 1..,
                if wrong.ends_with('2') { "3" } else { "2" },
            );
            let wrong = decode(sample, &wrong);
            println!(
                "{}: right key {:.0} Hz, wrong key {:.0} Hz",
                sample.name, right.0, wrong.0
            );
            failed |= !right.1 || wrong.1;
        }
        assert!(!failed, "a sample did not decode");
    }
}
