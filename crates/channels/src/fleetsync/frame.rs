//! `FleetSync` blocks: 64 bits, 48 of data under a 15 bit CRC and a parity bit. `FleetSync II`
//! sends each block as four, every 16 bit quarter a BCH(15,7) word carrying one nibble.

use sdrmm_dsp::{
    SyndromeDecoder,
    coding::fec::{Acceptance, chase},
    crc_msb_bits,
};

use super::model::{FleetSyncMessage, FleetSyncVersion};

pub(crate) const SYNC_WORDS: [u64; 2] = [0xAAAA_23EB, 0xAAAA_052B];
pub(crate) const SYNC_BITS: u32 = 32;
pub(crate) const BLOCK_BITS: usize = 64;
pub(crate) const BLOCK_BYTES: usize = 6;
pub(crate) const MAX_BYTES: usize = 22 * BLOCK_BYTES;
const QUARTERS: usize = 4;
const QUARTER_BITS: usize = 16;
const CHASE_BITS: usize = 4;
/// The block check corrects nothing, so a repair may only overturn weak bits worth one.
const BLOCK_REACH: u32 = 1;
/// The check register adds its polynomial before it shifts, which is the usual shift-first
/// register with the polynomial `0x6815` moved up one bit.
const CRC_POLY: u32 = 0x6815 << 1;
const CRC_TAIL: u16 = 0x0002;
const DATA_BITS: u32 = 48;
/// The `FleetSync II` quarters are BCH(15,7) words sent last bit first, and this generator is
/// the reciprocal of the code's `x^8 + x^7 + x^6 + x^4 + 1`.
const BCH_GENERATOR: u128 = 0x117;
const BCH_BITS: u32 = 15;
const BCH_CORRECTABLE: u32 = 2;
const NIBBLE_SHIFT: u16 = 8;
const FLEET_BASE: u16 = 99;
const UNIT_BASE: u16 = 999;
const TEXT_COMMAND: u8 = 0x42;
const TEXT_HEADER: usize = 11;

/// The check word a block's last 16 bits must equal.
pub(crate) fn check(first: u32, second: u32) -> u16 {
    let data = (u64::from(first) << 16) | u64::from(second >> 16);
    let crc = crc_msb_bits(
        CRC_POLY,
        16,
        0,
        (0..DATA_BITS).rev().map(|k| data >> k & 1 == 1),
    ) as u16;
    let parity = (first.count_ones() + (second >> 1).count_ones()) & 1;
    (crc ^ CRC_TAIL).wrapping_add(parity as u16)
}

fn block_holds(first: u32, second: u32) -> bool {
    check(first, second) == second as u16
}

fn bytes_of(first: u32, second: u32) -> [u8; BLOCK_BYTES] {
    let [a, b, c, d] = first.to_be_bytes();
    let [e, f, _, _] = second.to_be_bytes();
    [a, b, c, d, e, f]
}

pub(crate) fn bch() -> SyndromeDecoder {
    SyndromeDecoder::new(BCH_GENERATOR, BCH_BITS, BCH_CORRECTABLE)
}

fn reversed(word: u16) -> u16 {
    word.reverse_bits() >> 1
}

/// The `FleetSync I` block nearest `block` whose check holds, by Chase search over its weakest
/// bits.
fn held_block(block: u64, reliability: &[f32; BLOCK_BITS]) -> Option<(u32, u32)> {
    let holds = |trial: u64| block_holds((trial >> 32) as u32, trial as u32).then_some(trial);
    let acceptance = Acceptance::Radius(BLOCK_REACH);
    let fixed = chase(block, reliability, CHASE_BITS, acceptance, holds)?;
    Some(((fixed >> 32) as u32, fixed as u32))
}

fn nibble(bch: &SyndromeDecoder, quarter: u16, reliability: &[f32]) -> Option<u16> {
    let repair = |trial: u32| {
        let (fixed, _) = bch.decode(u128::from(reversed(trial as u16)))?;
        Some(u32::from(reversed(fixed as u16)))
    };
    let fixed = chase(
        u32::from(quarter & 0x7FFF),
        &reliability[..BCH_BITS as usize],
        CHASE_BITS,
        Acceptance::Radius(BCH_CORRECTABLE),
        repair,
    )?;
    Some(fixed as u16 >> NIBBLE_SHIFT & 0xF)
}

/// The 16 data bits one `FleetSync II` block carries, its quarters taken low half first.
fn quarter_word(bch: &SyndromeDecoder, block: u64, reliability: &[f32; BLOCK_BITS]) -> Option<u16> {
    let order = [2, 3, 0, 1];
    order.iter().try_fold(0u16, |word, &quarter| {
        let at = quarter * QUARTER_BITS;
        let half = (block >> at) as u16;
        nibble(bch, half, &reliability[at..at + QUARTER_BITS]).map(|n| word << 4 | n)
    })
}

/// A message being gathered block by block after a sync word.
pub(crate) struct Assembly {
    bytes: [u8; MAX_BYTES],
    len: usize,
    quarters: [u16; QUARTERS],
    quarter: usize,
    version: Option<FleetSyncVersion>,
}

/// What one block did to the message.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    More,
    Done,
}

impl Assembly {
    pub(crate) const fn new() -> Self {
        Self {
            bytes: [0; MAX_BYTES],
            len: 0,
            quarters: [0; QUARTERS],
            quarter: 0,
            version: None,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.len = 0;
        self.quarter = 0;
        self.version = None;
    }

    /// Takes one block, bit `i` of `block` with reliability `reliability[i]`.
    pub(crate) fn push(
        &mut self,
        bch: &SyndromeDecoder,
        block: u64,
        reliability: &[f32; BLOCK_BITS],
    ) -> Step {
        if self.quarter == 0
            && let Some((first, second)) = held_block(block, reliability)
        {
            return self.append(bytes_of(first, second), FleetSyncVersion::One);
        }
        let Some(word) = quarter_word(bch, block, reliability) else {
            return Step::Done;
        };
        self.quarters[self.quarter] = word;
        self.quarter += 1;
        if self.quarter < QUARTERS {
            return Step::More;
        }
        self.quarter = 0;
        let [a, b, c, d] = self.quarters.map(u32::from);
        let (first, second) = (a << 16 | b, c << 16 | d);
        if block_holds(first, second) {
            self.append(bytes_of(first, second), FleetSyncVersion::Two)
        } else {
            Step::Done
        }
    }

    fn append(&mut self, bytes: [u8; BLOCK_BYTES], version: FleetSyncVersion) -> Step {
        self.version.get_or_insert(version);
        self.bytes[self.len..self.len + BLOCK_BYTES].copy_from_slice(&bytes);
        self.len += BLOCK_BYTES;
        if self.len + BLOCK_BYTES > MAX_BYTES {
            Step::Done
        } else {
            Step::More
        }
    }

    /// The gathered message, once a block has held.
    pub(crate) fn message(&self, baud: u32) -> Option<FleetSyncMessage> {
        let version = self.version?;
        Some(parse(&self.bytes[..self.len], version, baud))
    }
}

fn fleet(byte: u8) -> Option<u16> {
    Some(u16::from(byte) + FLEET_BASE).filter(|&fleet| fleet != FLEET_BASE)
}

fn unit(raw: u16) -> Option<u16> {
    Some(raw + UNIT_BASE).filter(|&unit| unit != UNIT_BASE)
}

fn parse(bytes: &[u8], version: FleetSyncVersion, baud: u32) -> FleetSyncMessage {
    let command = bytes[1] & 0xFE;
    let wide = bytes[1] & 1 == 1;
    let from_fleet = fleet(bytes[2]);
    let to_fleet = if wide {
        bytes.get(6).copied().and_then(fleet)
    } else {
        from_fleet
    };
    let (text, text_cut) = if command == TEXT_COMMAND {
        text(bytes)
    } else {
        (None, false)
    };
    FleetSyncMessage {
        version,
        baud,
        flags: bytes[0],
        command,
        from_fleet,
        from_unit: unit(u16::from(bytes[3]) << 4 | u16::from(bytes[4] >> 4)),
        to_fleet,
        to_unit: unit(u16::from(bytes[4] & 0x0F) << 8 | u16::from(bytes[5])),
        text,
        truncated: text_cut || (wide && bytes.len() <= 6),
    }
}

/// The text of a message, each block's six bytes sent in reverse, and whether it was cut.
fn text(bytes: &[u8]) -> (Option<String>, bool) {
    let Some(&[high, low]) = bytes.get(TEXT_HEADER - 2..TEXT_HEADER) else {
        return (None, true);
    };
    let length = usize::from(u16::from_be_bytes([high, low]));
    let mut text = String::with_capacity(length);
    for i in 0..length {
        let at = TEXT_HEADER + BLOCK_BYTES * (i / BLOCK_BYTES + 1) - i % BLOCK_BYTES;
        let Some(&byte) = bytes.get(at) else {
            return (Some(text), true);
        };
        text.push(if byte.is_ascii_graphic() || byte == b' ' {
            char::from(byte)
        } else {
            '.'
        });
    }
    (Some(text), false)
}

/// The 64 bits of one `FleetSync I` block holding `bytes`.
#[cfg(test)]
pub(crate) fn encode(bytes: [u8; BLOCK_BYTES]) -> (u32, u32) {
    let first = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let top = u32::from_be_bytes([bytes[4], bytes[5], 0, 0]);
    let crc = check(first, top) & 0xFFFE;
    let parity = (first.count_ones() + top.count_ones() + (crc >> 1).count_ones()) & 1;
    (first, top | u32::from(crc) | parity)
}

/// The four `FleetSync II` blocks of one `FleetSync I` block.
#[cfg(test)]
pub(crate) fn encode_fs2(first: u32, second: u32) -> [(u32, u32); QUARTERS] {
    let bch = bch();
    let quarter = |nibble: u32| -> u32 {
        (0..1u16 << BCH_BITS)
            .find(|&word| {
                bch.remainder(u128::from(reversed(word))) == 0
                    && u32::from(word >> NIBBLE_SHIFT & 0xF) == nibble
            })
            .map_or(0, u32::from)
    };
    let words = [first >> 16, first & 0xFFFF, second >> 16, second & 0xFFFF];
    words.map(|word| {
        let n = [
            word >> 12 & 0xF,
            word >> 8 & 0xF,
            word >> 4 & 0xF,
            word & 0xF,
        ]
        .map(quarter);
        (n[1] << 16 | n[0], n[3] << 16 | n[2])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANI_START: (u32, u32) = (0xFE80_0830, 0x5305_9B6C);
    const ANI_END: (u32, u32) = (0xFC80_0830, 0x5305_7E59);

    #[test]
    fn the_block_check_holds_on_recorded_blocks() {
        for (first, second) in [ANI_START, ANI_END] {
            assert!(block_holds(first, second), "{first:08X} {second:08X}");
            assert!(!block_holds(first ^ 0x100, second));
        }
    }

    #[test]
    fn encoded_blocks_check() {
        assert_eq!(encode(bytes_of(ANI_START.0, ANI_START.1)), ANI_START);
        assert_eq!(encode(bytes_of(ANI_END.0, ANI_END.1)), ANI_END);
    }

    const STRONG: [f32; BLOCK_BITS] = [1.0; BLOCK_BITS];

    fn joined(first: u32, second: u32) -> u64 {
        u64::from(first) << 32 | u64::from(second)
    }

    fn weak(bits: &[usize]) -> [f32; BLOCK_BITS] {
        let mut reliability = STRONG;
        for &bit in bits {
            reliability[bit] = 0.05;
        }
        reliability
    }

    #[test]
    fn a_fleetsync_ii_quarter_repairs_two_flipped_bits() {
        let bch = bch();
        let (first, second) = encode_fs2(ANI_START.0, ANI_START.1)[1];
        assert_eq!(
            quarter_word(&bch, joined(first, second), &STRONG),
            Some(0x0830)
        );
        let broken = joined(first ^ 0x0009, second ^ 0x2400_0000);
        assert_eq!(quarter_word(&bch, broken, &STRONG), Some(0x0830));
    }

    #[test]
    fn three_weak_errors_in_a_quarter_are_repaired_softly() {
        let bch = bch();
        let (first, second) = encode_fs2(ANI_START.0, ANI_START.1)[1];
        let flips = [1, 6, 12];
        let broken = flips
            .iter()
            .fold(joined(first, second), |word, &bit| word ^ 1 << bit);
        assert_ne!(quarter_word(&bch, broken, &STRONG), Some(0x0830));
        assert_eq!(quarter_word(&bch, broken, &weak(&flips)), Some(0x0830));
    }

    #[test]
    fn weak_errors_in_a_fleetsync_i_block_are_repaired_softly() {
        let flips = [2, 23, 40];
        let broken = flips
            .iter()
            .fold(joined(ANI_END.0, ANI_END.1), |word, &bit| word ^ 1 << bit);
        let mut hard = Assembly::new();
        hard.push(&bch(), broken, &STRONG);
        assert_eq!(hard.message(1_200), None);
        let mut soft = Assembly::new();
        assert_eq!(soft.push(&bch(), broken, &weak(&flips)), Step::More);
        let message = soft.message(1_200).expect("a message");
        assert_eq!(message.version, FleetSyncVersion::One);
        assert_eq!(
            (message.from_fleet, message.from_unit),
            (Some(107), Some(1772))
        );
    }

    #[test]
    fn four_quarters_rebuild_the_block() {
        let bch = bch();
        let mut assembly = Assembly::new();
        for (first, second) in encode_fs2(ANI_START.0, ANI_START.1) {
            assert_eq!(
                assembly.push(&bch, joined(first, second), &STRONG),
                Step::More
            );
        }
        let message = assembly.message(1_200).expect("a message");
        assert_eq!(message.version, FleetSyncVersion::Two);
        assert_eq!(
            (message.from_fleet, message.from_unit),
            (Some(107), Some(1772))
        );
    }

    #[test]
    fn an_ani_names_fleet_and_unit() {
        let mut assembly = Assembly::new();
        assert_eq!(
            assembly.push(&bch(), joined(ANI_END.0, ANI_END.1), &STRONG),
            Step::More
        );
        let message = assembly.message(1_200).expect("a message");
        assert_eq!(message.command, 0x80);
        assert_eq!(
            (message.from_fleet, message.from_unit),
            (Some(107), Some(1772))
        );
        assert_eq!((message.to_fleet, message.to_unit), (Some(107), Some(1772)));
        assert!(!message.truncated);
    }

    #[test]
    fn text_blocks_read_back_to_front() {
        let mut bytes = vec![0x00, 0x42, 0x08, 0x30, 0x53, 0x05, 0, 0, 0, 0x00, 0x07, 0];
        bytes.extend_from_slice(b"OLLEH ");
        bytes.extend_from_slice(b"     !");
        let message = parse(&bytes, FleetSyncVersion::One, 1_200);
        assert_eq!(message.text.as_deref(), Some(" HELLO!"));
        assert!(!message.truncated);
        let cut = parse(&bytes[..18], FleetSyncVersion::One, 1_200);
        assert_eq!(cut.text.as_deref(), Some(" HELLO"));
        assert!(cut.truncated);
    }
}
