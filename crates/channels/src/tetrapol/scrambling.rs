use sdrmm_wire::TetrapolBand;

use super::coding::{Block, Decoded, Decoder, FRAME_BITS, Limits, SCRAMBLING_CODES};

const STRICT: Limits = Limits {
    voice: 2,
    first: 2,
    total: 4,
};
const LENIENT: Limits = Limits {
    voice: 4,
    first: 6,
    total: 12,
};
const VOTES_TO_LOCK: u8 = 2;

pub(crate) struct Scrambling {
    fixed: Option<u8>,
    locked: Option<u8>,
    candidate: Option<u8>,
    votes: u8,
}

impl Scrambling {
    pub(crate) fn new(fixed: Option<u8>) -> Self {
        Self {
            fixed,
            locked: None,
            candidate: None,
            votes: 0,
        }
    }

    pub(crate) fn set_fixed(&mut self, fixed: Option<u8>) {
        if fixed != self.fixed {
            *self = Self::new(fixed);
        }
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::new(self.fixed);
    }

    pub(crate) fn known(&self) -> bool {
        self.fixed.is_some() || self.locked.is_some()
    }

    pub(crate) fn decode(
        &mut self,
        decoder: &mut Decoder,
        frame: &[f32; FRAME_BITS],
        band: TetrapolBand,
    ) -> Option<(Decoded, u8)> {
        decoder.prepare(frame, band);
        if let Some(code) = self.fixed {
            return decoder.frame(frame, band, code, LENIENT).map(|d| (d, code));
        }
        if let Some(code) = self.locked
            && let Some(decoded) = decoder.frame(frame, band, code, LENIENT)
        {
            return Some((decoded, code));
        }
        let (decoded, code) = (0..SCRAMBLING_CODES as u8)
            .filter(|&code| Some(code) != self.locked)
            .find_map(|code| decoder.frame(frame, band, code, STRICT).map(|d| (d, code)))?;
        self.vote(code, &decoded);
        let trusted = matches!(decoded.block, Block::Data(_)) || self.locked == Some(code);
        trusted.then_some((decoded, code))
    }

    fn vote(&mut self, code: u8, decoded: &Decoded) {
        if self.candidate == Some(code) {
            self.votes += 1;
        } else {
            self.candidate = Some(code);
            self.votes = 1;
        }
        if matches!(decoded.block, Block::Data(_)) || self.votes >= VOTES_TO_LOCK {
            self.locked = Some(code);
            self.candidate = None;
            self.votes = 0;
        }
    }
}
