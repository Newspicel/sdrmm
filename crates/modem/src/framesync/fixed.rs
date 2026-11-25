use super::{SyncDistance, SyncPattern};

/// What one completed frame went through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Framed {
    /// Bits of this frame's sync word that differed from the word.
    pub sync_errors: u32,
    /// Whether the frame was kept by position alone: the word that opens it or the one after
    /// it was too damaged to vouch for it.
    pub flywheel: bool,
}

#[derive(Clone, Copy, Debug)]
struct Lock {
    at: usize,
    inverted: bool,
    /// Whether a second word has borne out the first; until then the search goes on.
    confirmed: bool,
    misses: u8,
    sync_errors: u32,
    /// How far the last word sat from the sync word, soft when the bits came soft.
    distance: f32,
}

/// The last 64 soft values twice over, so the newest `n` always lie in one slice.
#[derive(Clone, Copy, Debug)]
struct SoftWindow {
    values: [f32; 128],
    next: usize,
}

impl SoftWindow {
    const fn new() -> Self {
        Self {
            values: [0.0; 128],
            next: 0,
        }
    }

    fn push(&mut self, value: f32) {
        self.values[self.next] = value;
        self.values[self.next + 64] = value;
        self.next = (self.next + 1) % 64;
    }

    fn newest(&self) -> &[f32; 64] {
        let (_, newest) = self.values.split_at(self.next);
        newest.first_chunk().unwrap_or(&[0.0; 64])
    }
}

/// Fixed-length frames that each open with the same sync word, in a bit stream that may arrive
/// inverted. Once a word is found, the next is expected one frame later; a few damaged words
/// in a row are carried on position before the search starts over. A frame is handed out once
/// the word after it has been judged, so a lock no second word bears out yields nothing and
/// gives way to a match at least as good. Fed soft values, words are judged by
/// [`super::sync_distance`], so weak wrong bits cost less than confident ones.
pub struct FixedFramer {
    word: u64,
    mask: u64,
    word_bits: usize,
    frame_bits: usize,
    search_errors: u32,
    track_errors: u32,
    max_misses: u8,
    raw: u64,
    soft: Box<SoftWindow>,
    pattern: Box<SyncPattern>,
    soft_fed: bool,
    shift: u64,
    lock: Option<Lock>,
    building: Vec<u8>,
    done: Vec<u8>,
    pending: Option<Framed>,
    dropped: u32,
}

impl FixedFramer {
    /// `frame_bits` counts the sync word. Searching accepts up to `search_errors` wrong bits
    /// in a word, tracking up to `track_errors`, and `max_misses` worse words in a row.
    #[must_use]
    pub fn new(
        word: u64,
        word_bits: u32,
        frame_bits: usize,
        search_errors: u32,
        track_errors: u32,
        max_misses: u8,
    ) -> Self {
        let word_bits = word_bits.clamp(1, 64);
        let mask = u64::MAX >> (64 - word_bits);
        let frame_bits = frame_bits.max(word_bits as usize + 1);
        Self {
            word: word & mask,
            mask,
            word_bits: word_bits as usize,
            frame_bits,
            search_errors,
            track_errors,
            max_misses,
            raw: 0,
            soft: Box::new(SoftWindow::new()),
            pattern: Box::new(SyncPattern::new(word & mask, mask)),
            soft_fed: false,
            shift: 0,
            lock: None,
            building: vec![0; frame_bits.div_ceil(8)],
            done: vec![0; frame_bits.div_ceil(8)],
            pending: None,
            dropped: 0,
        }
    }

    pub fn reset(&mut self) {
        self.raw = 0;
        *self.soft = SoftWindow::new();
        self.shift = 0;
        self.lock = None;
        self.discard();
    }

    /// Frames read in full that were then thrown away, because the lock was lost or reset
    /// before the next word could vouch for them.
    #[must_use]
    pub const fn dropped(&self) -> u32 {
        self.dropped
    }

    fn discard(&mut self) {
        if self.pending.take().is_some() {
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    #[must_use]
    pub fn locked(&self) -> bool {
        self.lock.is_some()
    }

    /// The last frame handed out, sync word first, MSB first, the last byte zero-padded.
    #[must_use]
    pub fn frame(&self) -> &[u8] {
        &self.done
    }

    /// Takes one bit; returns when it hands out a frame, now readable from [`Self::frame`].
    pub fn push(&mut self, bit: bool) -> Option<Framed> {
        self.soft_fed = false;
        self.step(bit)
    }

    /// Takes one soft bit, positive is one; otherwise as [`Self::push`].
    pub fn push_soft(&mut self, value: f32) -> Option<Framed> {
        self.soft.push(value);
        self.soft_fed = true;
        self.step(value > 0.0)
    }

    fn step(&mut self, bit: bool) -> Option<Framed> {
        self.raw = (self.raw << 1 | u64::from(bit)) & self.mask;
        let lock = self.lock;
        let searching = match lock {
            None => Some(self.search_errors as f32),
            Some(lock) if !lock.confirmed && lock.at + 1 != self.word_bits => {
                Some(lock.distance.min(self.search_errors as f32))
            }
            Some(_) => None,
        };
        if let Some((distance, inverted)) = searching.and_then(|limit| self.found(limit)) {
            self.start(distance, inverted);
            return None;
        }
        self.fill(bit, lock?)
    }

    /// The last word's distance from the sync word, upright and inverted.
    fn distance(&self) -> SyncDistance {
        if self.soft_fed {
            self.pattern.distance(self.soft.newest())
        } else {
            SyncDistance::hard(self.raw, self.word, self.mask)
        }
    }

    /// The sync word's distance and polarity, when the last bits hold it within `limit`.
    fn found(&self, limit: f32) -> Option<(f32, bool)> {
        let best = self.distance().best();
        (best.0 <= limit).then_some(best)
    }

    fn hard_errors(&self, inverted: bool) -> u32 {
        let bits = if inverted { !self.raw } else { self.raw };
        ((bits ^ self.word) & self.mask).count_ones()
    }

    fn start(&mut self, distance: f32, inverted: bool) {
        self.shift = if inverted {
            !self.raw & self.mask
        } else {
            self.raw
        };
        self.write_word();
        self.discard();
        self.lock = Some(Lock {
            at: self.word_bits,
            inverted,
            confirmed: false,
            misses: 0,
            sync_errors: self.hard_errors(inverted),
            distance,
        });
    }

    fn fill(&mut self, bit: bool, mut lock: Lock) -> Option<Framed> {
        let bit = bit ^ lock.inverted;
        self.shift = (self.shift << 1 | u64::from(bit)) & self.mask;
        self.set(lock.at, bit);
        lock.at += 1;
        let mut handed = None;
        if lock.at == self.word_bits {
            if !self.check_word(&mut lock) {
                self.lock = None;
                self.discard();
                return None;
            }
            let missed = lock.misses > 0;
            handed = self.pending.take().map(|framed| Framed {
                flywheel: framed.flywheel || missed,
                ..framed
            });
        } else if lock.at == self.frame_bits {
            std::mem::swap(&mut self.building, &mut self.done);
            self.pending = Some(Framed {
                sync_errors: lock.sync_errors,
                flywheel: lock.misses > 0,
            });
            lock.at = 0;
        }
        self.lock = Some(lock);
        handed
    }

    /// Judges the word that opens a frame; `false` when the lock is lost.
    fn check_word(&mut self, lock: &mut Lock) -> bool {
        let distance = self.distance();
        let (upright, flipped) = if lock.inverted {
            (distance.inverted, distance.upright)
        } else {
            (distance.upright, distance.inverted)
        };
        let limit = self.track_errors as f32;
        lock.distance = if upright <= limit {
            lock.misses = 0;
            upright
        } else if flipped <= limit {
            self.flip_word();
            lock.misses = 0;
            lock.inverted = !lock.inverted;
            flipped
        } else if !lock.confirmed || lock.misses >= self.max_misses {
            return false;
        } else {
            lock.misses += 1;
            upright
        };
        lock.sync_errors = self.hard_errors(lock.inverted);
        lock.confirmed |= lock.misses == 0;
        true
    }

    /// The stream turned over: the word just taken is the complement of what it should be.
    fn flip_word(&mut self) {
        self.shift = !self.shift & self.mask;
        self.write_word();
    }

    fn write_word(&mut self) {
        for index in 0..self.word_bits {
            self.set(index, self.shift >> (self.word_bits - 1 - index) & 1 == 1);
        }
    }

    fn set(&mut self, index: usize, bit: bool) {
        let byte = &mut self.building[index / 8];
        let mask = 0x80 >> (index % 8);
        if bit {
            *byte |= mask;
        } else {
            *byte &= !mask;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORD: u64 = 0x1ACF_FC1D;
    const FRAME_BITS: usize = 64;

    fn bits_of(bytes: &[u8]) -> Vec<bool> {
        bytes
            .iter()
            .flat_map(|byte| (0..8).rev().map(move |shift| byte >> shift & 1 == 1))
            .collect()
    }

    fn frames(framer: &mut FixedFramer, bits: &[bool]) -> Vec<(Vec<u8>, Framed)> {
        let mut out = Vec::new();
        for &bit in bits {
            if let Some(framed) = framer.push(bit) {
                out.push((framer.frame().to_vec(), framed));
            }
        }
        out
    }

    fn stream(payloads: &[[u8; 4]]) -> Vec<u8> {
        let mut bytes = vec![0x55, 0x0F];
        for payload in payloads {
            bytes.extend_from_slice(&[0x1A, 0xCF, 0xFC, 0x1D]);
            bytes.extend_from_slice(payload);
        }
        bytes.extend_from_slice(&[0x1A, 0xCF, 0xFC, 0x1D]);
        bytes
    }

    #[test]
    fn frames_follow_the_word_through_leading_noise() {
        let mut framer = FixedFramer::new(WORD, 32, FRAME_BITS, 0, 4, 2);
        let got = frames(
            &mut framer,
            &bits_of(&stream(&[[1, 2, 3, 4], [5, 6, 7, 8]])),
        );
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].0, [0x1A, 0xCF, 0xFC, 0x1D, 1, 2, 3, 4]);
        assert_eq!(got[1].0[4..], [5, 6, 7, 8]);
        assert!(!got[1].1.flywheel);
    }

    #[test]
    fn a_frame_cut_off_by_a_reset_is_counted() {
        let mut framer = FixedFramer::new(WORD, 32, FRAME_BITS, 0, 4, 2);
        let bits = bits_of(&stream(&[[1, 2, 3, 4]]));
        let got = frames(&mut framer, &bits[..bits.len() - 32]);
        assert!(got.is_empty());
        assert_eq!(framer.dropped(), 0);
        framer.reset();
        assert_eq!(framer.dropped(), 1);
    }

    #[test]
    fn an_inverted_stream_comes_out_upright() {
        let mut framer = FixedFramer::new(WORD, 32, FRAME_BITS, 0, 4, 2);
        let inverted: Vec<bool> = bits_of(&stream(&[[9, 8, 7, 6]]))
            .into_iter()
            .map(|bit| !bit)
            .collect();
        let got = frames(&mut framer, &inverted);
        assert_eq!(got[0].0, [0x1A, 0xCF, 0xFC, 0x1D, 9, 8, 7, 6]);
    }

    #[test]
    fn a_stream_that_turns_over_is_followed() {
        let mut framer = FixedFramer::new(WORD, 32, FRAME_BITS, 0, 4, 2);
        let mut bits = bits_of(&stream(&[[1, 1, 1, 1], [2, 2, 2, 2], [3, 3, 3, 3]]));
        for bit in &mut bits[16 + FRAME_BITS..] {
            *bit = !*bit;
        }
        let got = frames(&mut framer, &bits);
        let payloads: Vec<u8> = got.iter().map(|(frame, _)| frame[4]).collect();
        assert_eq!(payloads, [1, 2, 3]);
        assert_eq!(got[2].0[..4], [0x1A, 0xCF, 0xFC, 0x1D]);
        assert!(!got[2].1.flywheel);
    }

    fn payloads(bytes: &[u8], max_misses: u8) -> Vec<(u8, bool)> {
        let mut framer = FixedFramer::new(WORD, 32, FRAME_BITS, 0, 2, max_misses);
        frames(&mut framer, &bits_of(bytes))
            .iter()
            .map(|(frame, framed)| (frame[4], framed.flywheel))
            .collect()
    }

    #[test]
    fn a_damaged_word_is_carried_then_dropped() {
        let mut bytes = stream(&[[1; 4], [2; 4], [3; 4], [4; 4], [5; 4]]);
        bytes[18] ^= 0xFF;
        let got = payloads(&bytes, 1);
        assert_eq!(
            got,
            [(1, false), (2, true), (3, true), (4, false), (5, false)]
        );
        bytes[26] ^= 0xFF;
        assert_eq!(payloads(&bytes, 1), [(1, false), (2, true), (5, false)]);
    }

    #[test]
    fn weak_wrong_bits_in_the_words_do_not_hide_them_from_soft_search() {
        let bits = bits_of(&stream(&[[1, 2, 3, 4], [5, 6, 7, 8]]));
        let mut soft: Vec<f32> = bits
            .iter()
            .map(|&bit| if bit { 1.0 } else { -1.0 })
            .collect();
        for word in 0..3 {
            for at in [3, 19] {
                let at = 16 + word * FRAME_BITS + at;
                soft[at] *= -0.2;
            }
        }
        let hard: Vec<bool> = soft.iter().map(|&value| value > 0.0).collect();
        assert!(frames(&mut FixedFramer::new(WORD, 32, FRAME_BITS, 1, 1, 0), &hard).is_empty());
        let mut framer = FixedFramer::new(WORD, 32, FRAME_BITS, 1, 1, 0);
        let got: Vec<(u8, Framed)> = soft
            .iter()
            .filter_map(|&value| {
                framer
                    .push_soft(value)
                    .map(|framed| (framer.frame()[4], framed))
            })
            .collect();
        let payloads: Vec<u8> = got.iter().map(|(payload, _)| *payload).collect();
        assert_eq!(payloads, [1, 5]);
        assert_eq!(got[0].1.sync_errors, 2);
        assert_eq!(framer.frame()[4..], [5, 6, 7, 8]);
    }

    #[test]
    fn a_lock_no_second_word_bore_out_gives_way() {
        let mut framer = FixedFramer::new(WORD, 32, FRAME_BITS, 0, 2, 1);
        let mut bytes = vec![0x0F, 0x1A, 0xCF, 0xFC, 0x1D, 0x77];
        bytes.extend(&stream(&[[6; 4], [7; 4]])[2..]);
        let got = frames(&mut framer, &bits_of(&bytes));
        let payloads: Vec<u8> = got.iter().map(|(frame, _)| frame[4]).collect();
        assert_eq!(payloads, [6, 7]);
    }
}
