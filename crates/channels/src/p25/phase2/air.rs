use num_complex::Complex;

use super::demod::{dibit, unturn};

pub(crate) const ISCH_DIBITS: usize = 20;
pub(crate) const BODY_DIBITS: usize = 160;
pub(crate) const PACKET_DIBITS: usize = ISCH_DIBITS + BODY_DIBITS;
pub(crate) const SLOTS: usize = 12;

/// The S-ISCH: the outbound sync word before timeslots 2, 3, 6, 7, 10 and 11.
pub(crate) const S_ISCH: u64 = 0x57_5D57_F7FF;
const ISCH_MASK: u64 = (1 << (2 * ISCH_DIBITS)) - 1;
const S_ISCH_TOLERANCE: u32 = 4;
/// The I-ISCH is a (40,9,16) code.
const I_ISCH_TOLERANCE: u32 = 7;
/// Two superframes without a recognised ISCH and the grid is gone.
const MAX_MISSES: u32 = 2 * SLOTS as u32;

/// The logical channel each timeslot carries: alternating, with the last pair swapped.
pub(crate) const SLOT_CHANNEL: [u8; SLOTS] = [0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 0];

/// The body dibits holding the DUID, most significant first.
const DUID_DIBITS: [usize; 4] = [0, 37, 122, 159];
const DUID_CODEWORDS: [u8; 16] = [
    0x00, 0x17, 0x2E, 0x39, 0x4B, 0x5C, 0x65, 0x72, 0x8D, 0x9A, 0xA3, 0xB4, 0xC6, 0xD1, 0xE8, 0xFF,
];

const I_ISCH_GENERATOR: [u64; 9] = [
    0x88_16CE_36D7,
    0x20_1DFD_4F64,
    0x10_0F4B_1758,
    0x0C_00DE_D18E,
    0x02_0807_F7FF,
    0x09_048D_9B72,
    0x00_9DA3_A171,
    0x00_58CB_AA4E,
    0x00_343D_8597,
];
const I_ISCH_OFFSET: u64 = 0x18_4229_D461;
static I_ISCH_CODEWORDS: [u64; 128] = i_isch_codewords();

const fn i_isch_codewords() -> [u64; 128] {
    let mut table = [0u64; 128];
    let mut value = 0;
    while value < 128 {
        let mut word = I_ISCH_OFFSET;
        let mut bit = 0;
        while bit < 9 {
            if value >> (8 - bit) & 1 == 1 {
                word ^= I_ISCH_GENERATOR[bit];
            }
            bit += 1;
        }
        table[value] = word;
        value += 1;
    }
    table
}

/// What a burst carries, by its DUID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Burst {
    Voice4,
    Voice2,
    Sacch { scrambled: bool },
    Facch { scrambled: bool },
    Lcch { scrambled: bool },
    Datch,
    Reserved(u8),
    Unknown,
}

impl Burst {
    pub(crate) fn voice_frames(self) -> usize {
        match self {
            Self::Voice4 => 4,
            Self::Voice2 => 2,
            _ => 0,
        }
    }

    fn of(duid: u8) -> Self {
        match duid {
            0 => Self::Voice4,
            6 => Self::Voice2,
            3 => Self::Sacch { scrambled: true },
            12 => Self::Sacch { scrambled: false },
            9 => Self::Facch { scrambled: true },
            15 => Self::Facch { scrambled: false },
            4 => Self::Lcch { scrambled: true },
            13 => Self::Lcch { scrambled: false },
            10 => Self::Datch,
            other => Self::Reserved(other),
        }
    }
}

/// The DUID nearest an 8-bit codeword: the code has distance 4, so one error is corrected.
pub(crate) fn burst_of(codeword: u8) -> Burst {
    let mut best = None;
    for (duid, &candidate) in DUID_CODEWORDS.iter().enumerate() {
        if (codeword ^ candidate).count_ones() <= 1 {
            best = Some(duid as u8);
        }
    }
    best.map_or(Burst::Unknown, Burst::of)
}

/// An I-ISCH, which names the timeslot it opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InfoIsch {
    pub(crate) channel: u8,
    pub(crate) location: u8,
}

impl InfoIsch {
    pub(crate) fn slot(self) -> usize {
        usize::from(self.location) * 4 + usize::from(self.channel)
    }
}

pub(crate) fn info_isch(word: u64) -> Option<InfoIsch> {
    let (value, errors) = I_ISCH_CODEWORDS
        .iter()
        .enumerate()
        .map(|(value, &codeword)| (value, (codeword ^ word).count_ones()))
        .min_by_key(|&(_, errors)| errors)?;
    let isch = InfoIsch {
        channel: (value >> 5 & 3) as u8,
        location: (value >> 3 & 3) as u8,
    };
    (errors <= I_ISCH_TOLERANCE && isch.location < 3 && isch.channel < 2).then_some(isch)
}

fn is_sync(word: u64) -> bool {
    (word ^ S_ISCH).count_ones() <= S_ISCH_TOLERANCE
}

/// One timeslot as received: its ISCH and body as differential phasors.
pub(crate) struct Packet {
    pub(crate) phasors: [Complex<f32>; PACKET_DIBITS],
    pub(crate) slot: usize,
}

impl Packet {
    pub(crate) fn body(&self) -> &[Complex<f32>] {
        &self.phasors[ISCH_DIBITS..]
    }

    pub(crate) fn burst(&self) -> Burst {
        let body = self.body();
        let codeword = DUID_DIBITS
            .iter()
            .fold(0u8, |word, &at| word << 2 | dibit(body[at]));
        burst_of(codeword)
    }

    pub(crate) fn channel(&self) -> u8 {
        SLOT_CHANNEL[self.slot]
    }
}

/// Finds the S-ISCH under any quarter-turn rotation, then cuts the stream into timeslots on
/// that grid, naming each by its I-ISCH or by counting from the last one.
pub(crate) struct Framer {
    recent: [Complex<f32>; ISCH_DIBITS],
    recent_at: usize,
    words: [u64; 4],
    locked: bool,
    rotation: u8,
    fill: usize,
    next_slot: Option<usize>,
    misses: u32,
    packet: Packet,
}

impl Framer {
    pub(crate) fn new() -> Self {
        Self {
            recent: [Complex::new(0.0, 0.0); ISCH_DIBITS],
            recent_at: 0,
            words: [0; 4],
            locked: false,
            rotation: 0,
            fill: 0,
            next_slot: None,
            misses: 0,
            packet: Packet {
                phasors: [Complex::new(0.0, 0.0); PACKET_DIBITS],
                slot: 0,
            },
        }
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::new();
    }

    /// Takes one phasor; true when it completes a named timeslot, which [`Self::packet`] holds.
    pub(crate) fn push(&mut self, phasor: Complex<f32>) -> bool {
        self.recent[self.recent_at] = phasor;
        self.recent_at = (self.recent_at + 1) % ISCH_DIBITS;
        for (quarters, word) in self.words.iter_mut().enumerate() {
            let turned = unturn(phasor, quarters as u8);
            *word = (*word << 2 | u64::from(dibit(turned))) & ISCH_MASK;
        }
        if let Some(rotation) = self.off_grid_sync() {
            self.anchor(rotation);
            return false;
        }
        if !self.locked {
            return false;
        }
        self.packet.phasors[self.fill] = unturn(phasor, self.rotation);
        self.fill += 1;
        self.fill == PACKET_DIBITS && self.complete()
    }

    pub(crate) fn packet(&self) -> &Packet {
        &self.packet
    }

    /// A sync word the current grid would not cut: any while hunting, and while locked one
    /// that turns up where the grid lost its ISCH.
    fn off_grid_sync(&self) -> Option<u8> {
        if self.locked && self.misses == 0 {
            return None;
        }
        (0..4u8).find(|&quarters| {
            let on_grid = self.locked && quarters == self.rotation && self.fill == ISCH_DIBITS - 1;
            is_sync(self.words[usize::from(quarters)]) && !on_grid
        })
    }

    fn anchor(&mut self, rotation: u8) {
        self.locked = true;
        self.rotation = rotation;
        self.next_slot = None;
        self.misses = 0;
        for index in 0..ISCH_DIBITS {
            let phasor = self.recent[(self.recent_at + index) % ISCH_DIBITS];
            self.packet.phasors[index] = unturn(phasor, rotation);
        }
        self.fill = ISCH_DIBITS;
    }

    fn complete(&mut self) -> bool {
        self.fill = 0;
        let word = self.packet.phasors[..ISCH_DIBITS]
            .iter()
            .fold(0u64, |word, &phasor| word << 2 | u64::from(dibit(phasor)));
        let slot = if let Some(isch) = info_isch(word) {
            self.misses = 0;
            Some(isch.slot())
        } else if is_sync(word) {
            self.misses = 0;
            self.next_slot
        } else {
            self.misses += 1;
            if self.misses >= MAX_MISSES {
                self.locked = false;
            }
            self.next_slot
        };
        self.next_slot = slot.map(|slot| (slot + 1) % SLOTS);
        let Some(slot) = slot else {
            return false;
        };
        self.packet.slot = slot;
        true
    }
}

#[cfg(test)]
pub(crate) fn info_isch_codeword(value: usize) -> u64 {
    I_ISCH_CODEWORDS[value]
}

#[cfg(test)]
pub(crate) fn duid_codeword(duid: u8) -> u8 {
    DUID_CODEWORDS[usize::from(duid & 0xF)]
}
