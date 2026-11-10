use crate::crypto::{Aes, Des, Rc4, aes_iv, hex, next_message_indicator, ofb};
use crate::dv::vocoder::{FULL_RATE_INFO_BYTES, FullRateFrame, pack_full, unpack_full};

pub(crate) const ALGORITHM_CLEAR: u8 = 0x80;
const ALGORITHM_DES: u8 = 0x81;
const ALGORITHM_AES_256: u8 = 0x84;
const ALGORITHM_AES_128: u8 = 0x89;
const ALGORITHM_ADP: u8 = 0xAA;

pub(crate) const FRAMES_PER_SUPERFRAME: usize = 18;
const FRAMES_PER_LDU: usize = 9;
const LOW_SPEED_DATA_BYTES: usize = 2;
/// Keystream ahead of the first voice frame: the link control word and its reserved bytes.
const LINK_CONTROL_BYTES: usize = 11;
const ADP_KEY_BYTES: usize = 5;
const ADP_DROP: usize = 256;
const DES_BLOCK: usize = 8;
const AES_BLOCK: usize = 16;
/// The voice frames' share of one LDU1 and LDU2 pair's keystream.
pub(crate) const SUPERFRAME_BYTES: usize =
    FRAMES_PER_SUPERFRAME * FULL_RATE_INFO_BYTES + 2 * LOW_SPEED_DATA_BYTES;
const OFB_SCRATCH: usize = AES_BLOCK + LINK_CONTROL_BYTES + SUPERFRAME_BYTES;

/// Where frame `index` of an LDU1 and LDU2 pair starts in its share of the keystream: eleven bytes
/// each, with the low speed data's two bytes ahead of the ninth frame of either LDU.
pub(crate) fn frame_offset(index: usize) -> usize {
    let low_speed_data = (index + 1) / FRAMES_PER_LDU;
    index * FULL_RATE_INFO_BYTES + low_speed_data * LOW_SPEED_DATA_BYTES
}

/// A P25 voice key, its algorithm read off its length: 10 hex digits for ADP, 16 for DES-OFB, 32
/// for AES-128 and 64 for AES-256.
pub(crate) enum CipherKey {
    Adp([u8; ADP_KEY_BYTES]),
    Des(Des),
    Aes128(Aes),
    Aes256(Aes),
}

impl CipherKey {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let bytes = hex(text.trim())?;
        match bytes.len() {
            ADP_KEY_BYTES => Some(Self::Adp(bytes.try_into().ok()?)),
            DES_BLOCK => Some(Self::Des(Des::new(bytes.try_into().ok()?))),
            16 => Aes::new(&bytes).map(Self::Aes128),
            32 => Aes::new(&bytes).map(Self::Aes256),
            _ => None,
        }
    }

    pub(crate) fn algorithm(&self) -> u8 {
        match self {
            Self::Adp(_) => ALGORITHM_ADP,
            Self::Des(_) => ALGORITHM_DES,
            Self::Aes128(_) => ALGORITHM_AES_128,
            Self::Aes256(_) => ALGORITHM_AES_256,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Adp(_) => "ADP",
            Self::Des(_) => "DES-OFB",
            Self::Aes128(_) => "AES-128",
            Self::Aes256(_) => "AES-256",
        }
    }

    /// The voice frames' keystream for the superframe `mi` starts.
    pub(crate) fn superframe(&self, mi: u64, stream: &mut [u8; SUPERFRAME_BYTES]) {
        match self {
            Self::Adp(key) => {
                let mut seed = [0u8; ADP_KEY_BYTES + 8];
                seed[..ADP_KEY_BYTES].copy_from_slice(key);
                seed[ADP_KEY_BYTES..].copy_from_slice(&mi.to_be_bytes());
                let mut cipher = Rc4::new(&seed);
                cipher.skip(ADP_DROP + LINK_CONTROL_BYTES);
                stream.fill(0);
                cipher.apply(stream);
            }
            Self::Des(des) => {
                let mut scratch = [0u8; OFB_SCRATCH];
                ofb(|block| des.encrypt(block), mi.to_be_bytes(), &mut scratch);
                let start = DES_BLOCK + LINK_CONTROL_BYTES;
                stream.copy_from_slice(&scratch[start..start + SUPERFRAME_BYTES]);
            }
            Self::Aes128(aes) | Self::Aes256(aes) => {
                let mut scratch = [0u8; OFB_SCRATCH];
                ofb(|block| aes.encrypt(block), aes_iv(mi), &mut scratch);
                let start = AES_BLOCK + LINK_CONTROL_BYTES;
                stream.copy_from_slice(&scratch[start..start + SUPERFRAME_BYTES]);
            }
        }
    }
}

pub(crate) type CipherKeys = Vec<(u16, CipherKey)>;

/// The P25 voice privacy of one call: the encryption sync of the header or the last LDU2 names the
/// next superframe's algorithm, key and MI, and each LDU1 starts that superframe's keystream.
pub(super) struct Privacy {
    sync: Option<Sync>,
    keystream: Option<usize>,
    stream: [u8; SUPERFRAME_BYTES],
}

#[derive(Clone, Copy)]
struct Sync {
    algorithm: u8,
    key_id: u16,
    mi: u64,
}

impl Privacy {
    pub(super) fn new() -> Self {
        Self {
            sync: None,
            keystream: None,
            stream: [0; SUPERFRAME_BYTES],
        }
    }

    pub(super) fn reset(&mut self) {
        self.sync = None;
        self.keystream = None;
    }

    /// Takes the encryption sync of a header or LDU2 for the superframe that follows it.
    pub(super) fn sync(&mut self, algorithm: u8, key_id: u16, mi: u64) {
        self.sync = Some(Sync {
            algorithm,
            key_id,
            mi,
        });
    }

    /// An LDU2 whose encryption sync did not decode: the next MI is the LFSR's next step.
    pub(super) fn sync_lost(&mut self) {
        if let Some(sync) = &mut self.sync {
            sync.mi = next_message_indicator(sync.mi);
        }
    }

    /// Undoes the cipher on frame `index` (0 to 17) of the LDU1 and LDU2 pair. False while the
    /// call stays muted: no sync yet or no key for it.
    pub(super) fn decipher(
        &mut self,
        index: usize,
        frame: &mut FullRateFrame,
        keys: &CipherKeys,
    ) -> bool {
        if index == 0 {
            self.keystream = self.key_index(keys);
            if let (Some(key), Some(sync)) = (self.keystream, self.sync) {
                keys[key].1.superframe(sync.mi, &mut self.stream);
            }
        }
        if self.keystream.is_none() {
            return false;
        }
        let offset = frame_offset(index);
        let mut bytes = pack_full(&frame.info);
        for (byte, key) in bytes.iter_mut().zip(&self.stream[offset..]) {
            *byte ^= key;
        }
        frame.info = unpack_full(&bytes);
        true
    }

    fn key_index(&self, keys: &CipherKeys) -> Option<usize> {
        let sync = self.sync?;
        keys.iter()
            .position(|(id, key)| *id == sync.key_id && key.algorithm() == sync.algorithm)
    }

    /// Names the cipher once the call is being deciphered.
    pub(super) fn label(&self, keys: &CipherKeys) -> Option<String> {
        self.keystream.map(|key| keys[key].1.name().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> CipherKey {
        CipherKey::parse(text).expect("key")
    }

    #[test]
    fn frames_skip_the_low_speed_data_ahead_of_each_ninth_frame() {
        assert_eq!(frame_offset(0), 0);
        assert_eq!(frame_offset(7), 77);
        assert_eq!(frame_offset(8), 90);
        assert_eq!(frame_offset(9), 101);
        assert_eq!(frame_offset(16), 178);
        assert_eq!(frame_offset(17), 191);
        assert_eq!(frame_offset(17) + FULL_RATE_INFO_BYTES, SUPERFRAME_BYTES);
    }

    #[test]
    fn keys_take_their_algorithm_from_their_length() {
        assert_eq!(key("deadbeef69").algorithm(), ALGORITHM_ADP);
        assert_eq!(key("465BA1A7F83D10C2").algorithm(), ALGORITHM_DES);
        assert_eq!(key(&"11".repeat(16)).algorithm(), ALGORITHM_AES_128);
        assert_eq!(key(&"22".repeat(32)).algorithm(), ALGORITHM_AES_256);
        assert!(CipherKey::parse("123456").is_none());
    }

    #[test]
    fn adp_drops_the_rc4_lead_and_the_link_control() {
        let mut stream = [0u8; SUPERFRAME_BYTES];
        key("deadbeef69").superframe(0x0123_4567_89AB_CDEF, &mut stream);
        let mut seed = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x69];
        seed.extend(0x0123_4567_89AB_CDEFu64.to_be_bytes());
        let mut rc4 = Rc4::new(&seed);
        rc4.skip(267);
        assert_eq!(stream[0], rc4.next_byte());
    }

    #[test]
    fn des_ofb_discards_its_first_block_and_the_link_control() {
        let mut stream = [0u8; SUPERFRAME_BYTES];
        key("0123456789ABCDEF").superframe(0x1234_5678_90AB_CDEF, &mut stream);
        let des = Des::new([0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF]);
        let mut reference = [0u8; 24];
        ofb(
            |block| des.encrypt(block),
            0x1234_5678_90AB_CDEFu64.to_be_bytes(),
            &mut reference,
        );
        assert_eq!(stream[..5], reference[19..24]);
    }

    #[test]
    fn a_superframe_without_its_key_stays_muted() {
        let keys = vec![(7, key("deadbeef69"))];
        let mut privacy = Privacy::new();
        privacy.sync(ALGORITHM_ADP, 8, 1);
        let mut frame = FullRateFrame {
            info: [0; 8],
            errors: [0; 8],
        };
        assert!(!privacy.decipher(0, &mut frame, &keys));
        privacy.sync(ALGORITHM_DES, 7, 1);
        assert!(!privacy.decipher(0, &mut frame, &keys));
        privacy.sync(ALGORITHM_ADP, 7, 1);
        assert!(privacy.decipher(0, &mut frame, &keys));
        assert_eq!(privacy.label(&keys).as_deref(), Some("ADP"));
    }

    #[test]
    #[ignore = "needs KNOWN_KEY_SAMPLES=<known-key-mbe-samples checkout>"]
    fn known_key_samples_decipher() {
        use crate::dv::known_keys::{MIN_FULL_RATE_CORRELATION, Sample, correlation, samples};
        use crate::dv::vocoder::{MbeDecoder, full_frame};
        use crate::{AUDIO_RATE, ChannelOutputs};

        fn decode(sample: &Sample, key_text: &str) -> f64 {
            let key_id = sample
                .key_id
                .or_else(|| sample.frames.iter().find_map(|frame| frame.key_id))
                .expect("key id");
            let keys = vec![(key_id, key(key_text))];
            let mut privacy = Privacy::new();
            let mut vocoder = MbeDecoder::full_rate();
            let mut out = ChannelOutputs::default();
            for (index, frame) in sample.frames.iter().enumerate() {
                if let (Some(algorithm), Some(key_id), Some(mi)) =
                    (frame.algorithm, frame.key_id, frame.mi)
                {
                    privacy.sync(algorithm, key_id, mi);
                }
                let dibits: [u8; 72] =
                    std::array::from_fn(|i| frame.bytes[i / 4] >> (6 - 2 * (i % 4)) & 0b11);
                let mut decoded = full_frame(&dibits);
                if privacy.decipher(index % FRAMES_PER_SUPERFRAME, &mut decoded, &keys) {
                    vocoder.play_full(&decoded, &mut out);
                } else {
                    MbeDecoder::mute(&mut out);
                }
            }
            correlation(sample, &out.audio_pcm, AUDIO_RATE as usize)
        }

        let all = samples("p25_");
        assert!(!all.is_empty(), "no p25 samples");
        let mut failed = false;
        for sample in &all {
            let right = decode(sample, &sample.key);
            let mut wrong = sample.key.clone();
            wrong.replace_range(..1, if wrong.starts_with('0') { "1" } else { "0" });
            let wrong = decode(sample, &wrong);
            println!(
                "{}: right key {right:.3}, wrong key {wrong:.3}",
                sample.name
            );
            failed |= right <= MIN_FULL_RATE_CORRELATION;
        }
        assert!(!failed, "a sample did not decode");
    }

    #[test]
    fn a_lost_sync_steps_the_message_indicator() {
        let mut privacy = Privacy::new();
        privacy.sync(ALGORITHM_ADP, 7, 0x0123_4567_89AB_CDEF);
        privacy.sync_lost();
        assert_eq!(
            privacy.sync.map(|sync| sync.mi),
            Some(next_message_indicator(0x0123_4567_89AB_CDEF))
        );
    }
}
