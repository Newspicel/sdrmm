//! Both directions of call audio: Opus, sealed with our media key in an encrypted room.

use std::borrow::Cow;
use std::collections::HashMap;
use std::time::Instant;

use crate::link::Ear;
use crate::matrix::e2ee::frame::{Sealer, open};
use crate::matrix::e2ee::keys::MediaKeys;
use crate::opus::{Decoder, Encoder};

/// `LiveKit` starts its frame count at a random number below this.
const FIRST_COUNT: u32 = 0xffff;

pub(crate) struct Media {
    keys: Option<MediaKeys>,
    sealer: Sealer,
    encoder: Encoder,
    decoders: HashMap<String, Decoder>,
    ear: Ear,
}

fn random_u32() -> Result<u32, String> {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes).map_err(|_| "No randomness".to_owned())?;
    Ok(u32::from_be_bytes(bytes))
}

impl Media {
    /// The sealer's SSRC only feeds the IV, which travels in each frame, so any number does.
    pub(crate) fn new(encrypted: bool, ear: Ear, now: Instant) -> Result<Self, String> {
        let keys = if encrypted {
            Some(MediaKeys::new(now).map_err(|error| error.to_string())?)
        } else {
            None
        };
        Ok(Self {
            keys,
            sealer: Sealer::new(random_u32()?, random_u32()? % FIRST_COUNT),
            encoder: Encoder::new().map_err(|error| format!("opus: {error}"))?,
            decoders: HashMap::new(),
            ear,
        })
    }

    pub(crate) fn keys(&mut self) -> Option<&mut MediaKeys> {
        self.keys.as_mut()
    }

    /// One 20 ms frame as Opus, sealed when the room is encrypted.
    pub(crate) fn outgoing(
        &mut self,
        frame: &[f32],
        timestamp: u32,
        now: Instant,
    ) -> Result<Vec<u8>, String> {
        let opus = self
            .encoder
            .encode(frame)
            .map_err(|error| format!("opus: {error}"))?;
        let Some(keys) = &self.keys else {
            return Ok(opus);
        };
        let (index, key) = keys.ours(now);
        self.sealer
            .seal(key, index, timestamp, &opus)
            .map_err(|error| error.to_string())
    }

    /// A heard frame as samples. One that will not open or decode counts as a drop.
    pub(crate) fn incoming(&mut self, identity: &str, payload: &[u8]) -> Option<Vec<f32>> {
        let Some(opus) = self.opened(identity, payload) else {
            self.ear.lose("undecryptable");
            return None;
        };
        let Some(decoder) = self.decoder(identity) else {
            self.ear.lose("undecodable");
            return None;
        };
        let Ok(pcm) = decoder.decode(&opus) else {
            self.ear.lose("undecodable");
            return None;
        };
        Some(pcm)
    }

    fn opened<'p>(&self, identity: &str, payload: &'p [u8]) -> Option<Cow<'p, [u8]>> {
        let Some(keys) = &self.keys else {
            return Some(Cow::Borrowed(payload));
        };
        let ring = keys.ring(identity)?;
        open(ring, payload).ok().map(Cow::Owned)
    }

    fn decoder(&mut self, identity: &str) -> Option<&mut Decoder> {
        if !self.decoders.contains_key(identity) {
            let decoder = Decoder::new().ok()?;
            self.decoders.insert(identity.to_owned(), decoder);
        }
        self.decoders.get_mut(identity)
    }

    pub(crate) fn forget(&mut self, identity: &str) {
        self.decoders.remove(identity);
    }
}
