//! The 40-bit LTR status word: sync, area, go-to and home repeaters, group, free repeater and a
//! seven-bit check. Its layout and check table follow `SDRTrunk`'s LTR decoder.

pub(crate) const WORD_BITS: u32 = 40;
/// The sync a repeater opens its words with; a radio sends the whole word inverted.
pub(crate) const SYNC: u64 = 0b1_0101_1000;
const SYNC_BITS: u32 = 9;
const CHECK_BITS: u32 = 7;
const PAYLOAD_BITS: u32 = WORD_BITS - SYNC_BITS - CHECK_BITS;
const CHECK_MASK: u8 = (1 << CHECK_BITS) - 1;
/// What each payload bit, area first, adds to the check.
const CHECKS: [u8; PAYLOAD_BITS as usize] = [
    0x38, 0x1C, 0x0E, 0x46, 0x23, 0x51, 0x68, 0x75, 0x7A, 0x3D, 0x1F, 0x4F, 0x26, 0x52, 0x29, 0x15,
    0x0B, 0x45, 0x62, 0x31, 0x19, 0x0D, 0x07, 0x43,
];
pub(crate) const MAX_REPEATER: u8 = 20;
/// The go-to repeater a word ending a call names.
const TURN_OFF: u8 = 31;
/// The group an idle repeater posts.
const IDLE_GROUP: u8 = 255;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Word {
    pub area: bool,
    pub goto: u8,
    pub home: u8,
    pub group: u8,
    pub free: u8,
    /// Sent by a radio rather than a repeater.
    pub inbound: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Meaning {
    Call,
    Idle,
    TurnOff,
    Other,
}

const fn repeater(number: u8) -> bool {
    1 <= number && number <= MAX_REPEATER
}

const fn field(word: u64, shift: u32, bits: u32) -> u8 {
    (word >> shift & ((1 << bits) - 1)) as u8
}

/// The check the payload of a 40-bit word calls for.
pub(crate) fn check(word: u64) -> u8 {
    let payload = word >> CHECK_BITS;
    CHECKS
        .iter()
        .enumerate()
        .filter(|&(bit, _)| payload >> (PAYLOAD_BITS as usize - 1 - bit) & 1 == 1)
        .fold(0, |sum, (_, &check)| sum ^ check)
}

impl Word {
    /// Reads the 40 bits last received, oldest first in the top bit.
    pub(crate) fn read(raw: u64) -> Option<Self> {
        let raw = raw & ((1 << WORD_BITS) - 1);
        let flipped = !raw & ((1 << WORD_BITS) - 1);
        let sent = field(raw, 0, CHECK_BITS);
        if raw >> (WORD_BITS - SYNC_BITS) == SYNC && check(raw) == sent {
            return Some(Self::of(raw, false));
        }
        let sent = field(flipped, 0, CHECK_BITS);
        let wanted = check(flipped);
        (flipped >> (WORD_BITS - SYNC_BITS) == SYNC
            && (wanted == sent || wanted ^ CHECK_MASK == sent))
            .then(|| Self::of(flipped, true))
    }

    fn of(raw: u64, inbound: bool) -> Self {
        Self {
            area: field(raw, 30, 1) == 1,
            goto: field(raw, 25, 5),
            home: field(raw, 20, 5),
            group: field(raw, 12, 8),
            free: field(raw, 7, 5),
            inbound,
        }
    }

    /// The word as a repeater sends it.
    #[must_use]
    pub fn encode(self) -> u64 {
        let word = SYNC << (WORD_BITS - SYNC_BITS)
            | u64::from(self.area) << 30
            | u64::from(self.goto & 0x1F) << 25
            | u64::from(self.home & 0x1F) << 20
            | u64::from(self.group) << 12
            | u64::from(self.free & 0x1F) << 7;
        word | u64::from(check(word))
    }

    pub(crate) const fn meaning(self) -> Meaning {
        let free = self.free <= MAX_REPEATER;
        if repeater(self.goto) && repeater(self.home) && free {
            if self.goto == self.free && self.group == IDLE_GROUP {
                Meaning::Idle
            } else {
                Meaning::Call
            }
        } else if self.goto == TURN_OFF && repeater(self.home) && free {
            Meaning::TurnOff
        } else {
            Meaning::Other
        }
    }

    /// The talkgroup as one number: area, home repeater and group.
    pub(crate) fn talkgroup(self) -> u32 {
        u32::from(self.area) << 13 | u32::from(self.home) << 8 | u32::from(self.group)
    }

    /// The talkgroup as LTR radios show it.
    pub(crate) fn talkgroup_name(self) -> String {
        format!("{}-{:02}-{:03}", u8::from(self.area), self.home, self.group)
    }
}
