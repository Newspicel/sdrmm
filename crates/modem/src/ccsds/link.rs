use sdrmm_dsp::coding::fec::{
    ccsds_rs::{Basis, CcsdsReedSolomon},
    conv7::ConvCode,
};

pub const ASM: u32 = 0x1ACF_FC1D;
pub const ASM_BYTES: usize = 4;
pub const CADU_BYTES: usize = 1024;
pub const CODED_BYTES: usize = CADU_BYTES - ASM_BYTES;
pub const INTERLEAVE: usize = 4;
pub const VCDU_BYTES: usize = 892;
pub const CADU_BITS: usize = CADU_BYTES * 8;
pub const CODED_BITS: usize = CADU_BITS * 2;
pub const ASM_CODED_BITS: usize = ASM_BYTES * 8 * 2;
pub const ENCODER_MEMORY: usize = 6;

pub const VCDU_HEADER: usize = 6;
pub const MPDU_HEADER: usize = 2;
pub const NO_PACKET_START: u16 = 0x7FF;
pub const FILL_VCID: u8 = 63;
pub const VCID_COUNT: usize = 64;
pub const COUNTER_MASK: u32 = 0xFF_FFFF;

pub const PACKET_HEADER: usize = 6;
pub const SEQUENCE_MODULO: u32 = 1 << 14;
pub const IDLE_APID: u16 = 0x7FF;

const POLY_FIRST: u16 = 0o171;
const POLY_SECOND: u16 = 0o133;

#[must_use]
pub fn conv_code() -> ConvCode {
    ConvCode::new(&[POLY_FIRST, POLY_SECOND])
}

#[must_use]
pub fn reed_solomon(basis: Basis) -> CcsdsReedSolomon {
    CcsdsReedSolomon::with_basis(basis)
}

/// The CCSDS pseudo-noise sequence, `x^8 + x^7 + x^5 + x^3 + 1` from all ones, over one CADU.
pub const PN: [u8; CODED_BYTES] = pn_sequence();

const fn pn_sequence() -> [u8; CODED_BYTES] {
    let mut out = [0u8; CODED_BYTES];
    let mut recent: u8 = 0xFF;
    let mut index = 0;
    while index < 8 * CODED_BYTES {
        let bit = if index < 8 {
            1
        } else {
            let next = (recent ^ recent >> 2 ^ recent >> 4 ^ recent >> 7) & 1;
            recent = recent << 1 | next;
            next
        };
        out[index / 8] |= bit << (7 - index % 8);
        index += 1;
    }
    out
}

/// XORs `bytes`, a CADU after its sync marker, with [`PN`]. The same call derandomizes.
pub fn randomize(bytes: &mut [u8]) {
    for (byte, &pn) in bytes.iter_mut().zip(PN.iter().cycle()) {
        *byte ^= pn;
    }
}

/// Packets missed between sequence counts `previous` and `next`.
#[must_use]
pub fn sequence_gap(previous: u16, next: u16) -> u32 {
    (u32::from(next) + SEQUENCE_MODULO - u32::from(previous) - 1) % SEQUENCE_MODULO
}

pub fn asm_bits() -> impl Iterator<Item = bool> {
    (0..32).rev().map(|shift| ASM >> shift & 1 == 1)
}

/// How the bits are differentially coded ahead of the convolutional encoder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Precoding {
    /// Sent as they are: Meteor, GOES LRIT, GK-2A.
    #[default]
    Nrzl,
    /// A one flips the line, a zero keeps it: GOES HRIT.
    Nrzm,
}

impl Precoding {
    /// Codes `bits` in place, starting from line state `state`.
    pub fn encode(self, bits: &mut [bool], mut state: bool) {
        if self == Self::Nrzl {
            return;
        }
        for bit in bits {
            state ^= *bit;
            *bit = state;
        }
    }
}

/// How a receiver's symbol pairs relate to the encoder's: QPSK can swap and invert either rail,
/// BPSK can invert both or start a pair one symbol late, and some links invert the second output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairMap {
    pub swap: bool,
    pub negate_first: bool,
    pub negate_second: bool,
}

impl PairMap {
    pub const ALL: [Self; 8] = {
        let mut all = [Self {
            swap: false,
            negate_first: false,
            negate_second: false,
        }; 8];
        let mut index = 0;
        while index < 8 {
            all[index] = Self {
                swap: index & 4 != 0,
                negate_first: index & 1 != 0,
                negate_second: index & 2 != 0,
            };
            index += 1;
        }
        all
    };

    #[must_use]
    pub fn apply<T: Copy + std::ops::Neg<Output = T>>(self, first: T, second: T) -> (T, T) {
        let (a, b) = if self.swap {
            (second, first)
        } else {
            (first, second)
        };
        (
            if self.negate_first { -a } else { a },
            if self.negate_second { -b } else { b },
        )
    }

    fn apply_bits(self, first: bool, second: bool) -> (bool, bool) {
        let (a, b) = if self.swap {
            (second, first)
        } else {
            (first, second)
        };
        (a ^ self.negate_first, b ^ self.negate_second)
    }

    fn preimage(self, wanted: (bool, bool)) -> (bool, bool) {
        [(false, false), (false, true), (true, false), (true, true)]
            .into_iter()
            .find(|&(a, b)| self.apply_bits(a, b) == wanted)
            .unwrap_or(wanted)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AsmPattern {
    pub map: PairMap,
    pub value: u64,
    pub mask: u64,
}

impl AsmPattern {
    #[must_use]
    pub fn mismatches(&self, window: u64) -> u32 {
        ((window ^ self.value) & self.mask).count_ones()
    }
}

#[must_use]
pub fn coded_asm() -> u64 {
    coded_asm_with(Precoding::Nrzl)
}

#[must_use]
pub fn coded_asm_with(precoding: Precoding) -> u64 {
    let code = conv_code();
    let mut bits: Vec<bool> = asm_bits().collect();
    precoding.encode(&mut bits, false);
    let mut coded = Vec::with_capacity(ASM_CODED_BITS);
    code.encode(&bits, &mut coded);
    coded
        .iter()
        .fold(0u64, |acc, &bit| acc << 1 | u64::from(bit))
}

#[must_use]
pub fn asm_patterns() -> [AsmPattern; 8] {
    asm_patterns_with(Precoding::Nrzl)
}

/// The coded ASM under every pair map. The other NRZ-M start state is the inverted pattern,
/// which the maps already hold.
#[must_use]
pub fn asm_patterns_with(precoding: Precoding) -> [AsmPattern; 8] {
    let coded = coded_asm_with(precoding);
    let mask = u64::MAX >> (2 * ENCODER_MEMORY);
    PairMap::ALL.map(|map| {
        let mut value = 0u64;
        for pair in 0..ASM_CODED_BITS / 2 {
            let shift = ASM_CODED_BITS - 2 - 2 * pair;
            let wanted = (coded >> (shift + 1) & 1 == 1, coded >> shift & 1 == 1);
            let (a, b) = map.preimage(wanted);
            value |= (u64::from(a) << 1 | u64::from(b)) << shift;
        }
        AsmPattern {
            map,
            value: value & mask,
            mask,
        }
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VcduHeader {
    pub spacecraft: u8,
    pub vcid: u8,
    pub counter: u32,
}

impl VcduHeader {
    #[must_use]
    pub fn parse(vcdu: &[u8]) -> Option<Self> {
        let &[a, b, c, d, e, ..] = vcdu else {
            return None;
        };
        Some(Self {
            spacecraft: (a & 0x3F) << 2 | b >> 6,
            vcid: b & 0x3F,
            counter: u32::from(c) << 16 | u32::from(d) << 8 | u32::from(e),
        })
    }

    pub fn write(self, vcdu: &mut [u8]) {
        if let Some(header) = vcdu.get_mut(..VCDU_HEADER) {
            header.copy_from_slice(&[
                0x40 | self.spacecraft >> 2,
                (self.spacecraft & 0x03) << 6 | self.vcid & 0x3F,
                (self.counter >> 16) as u8,
                (self.counter >> 8) as u8,
                self.counter as u8,
                0,
            ]);
        }
    }

    #[must_use]
    pub fn follows(self, previous: u32) -> bool {
        self.counter == (previous + 1) & COUNTER_MASK
    }
}

/// Where the multiplexing PDU sits in a VCDU: Meteor and `MetOp` put a two-byte insert zone
/// before it, GOES and GK-2A none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MpduLayout {
    pub insert_zone: usize,
}

impl MpduLayout {
    #[must_use]
    pub const fn header_at(self) -> usize {
        VCDU_HEADER + self.insert_zone
    }

    #[must_use]
    pub const fn zone_at(self) -> usize {
        self.header_at() + MPDU_HEADER
    }

    #[must_use]
    pub const fn zone_len(self) -> usize {
        VCDU_BYTES - self.zone_at()
    }

    #[must_use]
    pub fn first_header(self, vcdu: &[u8]) -> Option<u16> {
        let &[high, low] = vcdu.get(self.header_at()..self.zone_at())? else {
            return None;
        };
        Some(u16::from(high & 0x07) << 8 | u16::from(low))
    }

    pub fn write_first_header(self, vcdu: &mut [u8], first_header: u16) {
        if let Some(header) = vcdu.get_mut(VCDU_HEADER..self.zone_at()) {
            let (insert, pointer) = header.split_at_mut(self.insert_zone);
            insert.fill(0);
            pointer.copy_from_slice(&[(first_header >> 8) as u8 & 0x07, first_header as u8]);
        }
    }
}

/// The two sequence flags of a space packet: where it falls in a segmented user data unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment {
    Continuation,
    First,
    Last,
    Unsegmented,
}

impl Segment {
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0 => Self::Continuation,
            1 => Self::First,
            2 => Self::Last,
            _ => Self::Unsegmented,
        }
    }

    #[must_use]
    pub const fn bits(self) -> u8 {
        match self {
            Self::Continuation => 0,
            Self::First => 1,
            Self::Last => 2,
            Self::Unsegmented => 3,
        }
    }

    #[must_use]
    pub const fn starts(self) -> bool {
        matches!(self, Self::First | Self::Unsegmented)
    }

    #[must_use]
    pub const fn ends(self) -> bool {
        matches!(self, Self::Last | Self::Unsegmented)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PacketHeader {
    pub secondary: bool,
    pub apid: u16,
    pub segment: Segment,
    pub sequence: u16,
    pub length: usize,
}

impl PacketHeader {
    #[must_use]
    pub fn parse(header: &[u8]) -> Option<Self> {
        let &[a, b, c, d, e, f, ..] = header else {
            return None;
        };
        Some(Self {
            secondary: a & 0x08 != 0,
            apid: u16::from(a & 0x07) << 8 | u16::from(b),
            segment: Segment::from_bits(c >> 6),
            sequence: u16::from(c & 0x3F) << 8 | u16::from(d),
            length: (usize::from(e) << 8 | usize::from(f)) + 1,
        })
    }

    pub fn write(&self, header: &mut [u8]) {
        let length = self.length.saturating_sub(1);
        if let Some(header) = header.get_mut(..PACKET_HEADER) {
            header.copy_from_slice(&[
                u8::from(self.secondary) << 3 | (self.apid >> 8) as u8 & 0x07,
                self.apid as u8,
                self.segment.bits() << 6 | (self.sequence >> 8) as u8 & 0x3F,
                self.sequence as u8,
                (length >> 8) as u8,
                length as u8,
            ]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pn_sequence_starts_like_ccsds() {
        assert_eq!(&PN[..8], &[0xFF, 0x48, 0x0E, 0xC0, 0x9A, 0x0D, 0x70, 0xBC]);
        assert_eq!(PN[255..263], PN[..8]);
        let mut bytes = [0u8; CODED_BYTES + 8];
        randomize(&mut bytes);
        assert_eq!(bytes[CODED_BYTES..], bytes[..8]);
    }

    #[test]
    fn sequence_gaps_wrap() {
        assert_eq!(sequence_gap(16_383, 0), 0);
        assert_eq!(sequence_gap(16_383, 2), 2);
        assert_eq!(sequence_gap(5, 5), 16_383);
    }

    #[test]
    fn the_coded_asm_matches_the_meteor_correlator_word() {
        let mask = u64::MAX >> (2 * ENCODER_MEMORY);
        assert_eq!(coded_asm() & mask, !0xFCA2_B63D_B00D_9794u64 & mask);
    }

    #[test]
    fn every_pair_map_gives_a_distinct_pattern() {
        let patterns = asm_patterns();
        for (index, a) in patterns.iter().enumerate() {
            for b in &patterns[index + 1..] {
                assert!(a.mismatches(b.value) >= 20);
            }
        }
    }

    #[test]
    fn headers_round_trip() {
        let mut vcdu = [0u8; 10];
        let header = VcduHeader {
            spacecraft: 0x93,
            vcid: 5,
            counter: 0x12_3456,
        };
        header.write(&mut vcdu);
        assert_eq!(VcduHeader::parse(&vcdu), Some(header));
        assert_eq!(VcduHeader::parse(&vcdu[..4]), None);
        let layout = MpduLayout { insert_zone: 2 };
        layout.write_first_header(&mut vcdu, 0x2AB);
        assert_eq!(layout.first_header(&vcdu), Some(0x2AB));
        assert_eq!(layout.first_header(&vcdu[..9]), None);
        assert_eq!(layout.zone_len(), 882);
        assert_eq!(MpduLayout { insert_zone: 0 }.zone_len(), 884);
        let mut bytes = [0u8; 6];
        let packet = PacketHeader {
            secondary: true,
            apid: 65,
            segment: Segment::First,
            sequence: 0x3FFF,
            length: 900,
        };
        packet.write(&mut bytes);
        assert_eq!(PacketHeader::parse(&bytes), Some(packet));
        assert_eq!(PacketHeader::parse(&bytes[..5]), None);
    }

    #[test]
    fn the_counter_wraps() {
        let header = VcduHeader {
            spacecraft: 0,
            vcid: 1,
            counter: 0,
        };
        assert!(header.follows(COUNTER_MASK));
        assert!(!header.follows(5));
    }
}
