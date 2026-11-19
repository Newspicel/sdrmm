//! GPS LNAV subframes: word parity, the handover word and the ephemeris of subframes 1 to 3.

pub(super) const SUBFRAME_BITS: usize = 300;
const WORD_BITS: usize = 30;
const PREAMBLE: u8 = 0x8B;
/// IS-GPS-200 fixes pi at 3.1415926535898; the gap to the full constant moves an orbit by microns.
const GPS_PI: f64 = std::f64::consts::PI;

/// One parity-checked subframe: the 24 data bits of each word, polarity removed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Subframe {
    pub id: u8,
    /// GPS seconds of week at the start of the next subframe, from the handover word.
    pub next_tow_s: u32,
    pub data: [u32; 10],
    pub raw: [u32; 10],
}

impl Subframe {
    /// GPS seconds of week at the leading edge of this subframe.
    pub fn start_tow_s(&self) -> u32 {
        const WEEK_S: u32 = 604_800;
        (self.next_tow_s + WEEK_S - 6) % WEEK_S
    }

    fn field(&self, word: usize, first: u32, len: u32) -> u32 {
        (self.data[word - 1] >> (25 - first - len)) & ((1 << len) - 1)
    }

    fn joined(&self, high: (usize, u32, u32), low: (usize, u32, u32)) -> u32 {
        (self.field(high.0, high.1, high.2) << low.2) | self.field(low.0, low.1, low.2)
    }
}

/// Decodes 300 bits that start at a preamble, either polarity, or nothing if a word fails parity.
pub(super) fn decode_subframe(bits: &[bool]) -> Option<Subframe> {
    let preamble = byte(bits.get(..8)?);
    if preamble != PREAMBLE && preamble != !PREAMBLE {
        return None;
    }
    let previous_d30 = preamble != PREAMBLE;
    [false, true]
        .into_iter()
        .find_map(|previous_d29| decode_words(bits, previous_d29, previous_d30))
}

fn decode_words(bits: &[bool], mut d29: bool, mut d30: bool) -> Option<Subframe> {
    let mut data = [0_u32; 10];
    let mut raw = [0_u32; 10];
    for (index, (data, raw)) in data.iter_mut().zip(&mut raw).enumerate() {
        let word = word(bits.get(index * WORD_BITS..(index + 1) * WORD_BITS)?);
        *data = check_word(word, d29, d30)?;
        *raw = word;
        d29 = word & 2 != 0;
        d30 = word & 1 != 0;
    }
    let id = ((data[1] >> 2) & 0x7) as u8;
    (1..=5).contains(&id).then_some(Subframe {
        id,
        next_tow_s: ((data[1] >> 7) & 0x1_FFFF) * 6,
        data,
        raw,
    })
}

fn byte(bits: &[bool]) -> u8 {
    bits.iter().fold(0, |v, &bit| (v << 1) | u8::from(bit))
}

fn word(bits: &[bool]) -> u32 {
    bits.iter().fold(0, |v, &bit| (v << 1) | u32::from(bit))
}

pub(super) fn check_word(raw: u32, previous_d29: bool, previous_d30: bool) -> Option<u32> {
    let transmitted = raw >> 6;
    let data = if previous_d30 {
        transmitted ^ 0xFF_FFFF
    } else {
        transmitted
    };
    let d = |n: u8| data >> (24 - n) & 1 != 0;
    let x = |values: &[u8]| values.iter().fold(false, |v, &n| v ^ d(n));
    let expected = [
        previous_d29 ^ x(&[1, 2, 3, 5, 6, 10, 11, 12, 13, 14, 17, 18, 20, 23]),
        previous_d30 ^ x(&[2, 3, 4, 6, 7, 11, 12, 13, 14, 15, 18, 19, 21, 24]),
        previous_d29 ^ x(&[1, 3, 4, 5, 7, 8, 12, 13, 14, 15, 16, 19, 20, 22]),
        previous_d30 ^ x(&[2, 4, 5, 6, 8, 9, 13, 14, 15, 16, 17, 20, 21, 23]),
        previous_d30 ^ x(&[1, 3, 5, 6, 7, 9, 10, 14, 15, 16, 17, 18, 21, 22, 24]),
        previous_d29 ^ x(&[3, 5, 6, 8, 9, 10, 11, 13, 15, 19, 22, 23, 24]),
    ];
    let parity = expected
        .iter()
        .fold(0_u32, |v, &bit| (v << 1) | u32::from(bit));
    (parity == raw & 0x3F).then_some(data)
}

/// Broadcast orbit and clock of one satellite, IS-GPS-200 table 20-III, in SI units and radians.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ephemeris {
    pub week: u16,
    pub health: u8,
    pub iodc: u16,
    pub iode: u8,
    pub tgd: f64,
    pub toc: f64,
    pub af0: f64,
    pub af1: f64,
    pub af2: f64,
    pub crs: f64,
    pub delta_n: f64,
    pub m0: f64,
    pub cuc: f64,
    pub e: f64,
    pub cus: f64,
    pub sqrt_a: f64,
    pub toe: f64,
    pub cic: f64,
    pub omega0: f64,
    pub cis: f64,
    pub i0: f64,
    pub crc: f64,
    pub omega: f64,
    pub omega_dot: f64,
    pub idot: f64,
}

fn signed(value: u32, len: u32) -> f64 {
    f64::from(((value << (32 - len)) as i32) >> (32 - len))
}

fn scaled(value: f64, power: i32) -> f64 {
    value * 2f64.powi(power)
}

impl Ephemeris {
    /// Joins subframes 1, 2 and 3 when their issue of data agrees.
    pub(super) fn from_subframes(one: &Subframe, two: &Subframe, three: &Subframe) -> Option<Self> {
        if (one.id, two.id, three.id) != (1, 2, 3) {
            return None;
        }
        let iodc = one.joined((3, 23, 2), (8, 1, 8)) as u16;
        let iode = two.field(3, 1, 8) as u8;
        if three.field(10, 1, 8) as u8 != iode || iodc & 0xFF != u16::from(iode) {
            return None;
        }
        let mut eph = Self {
            week: one.field(3, 1, 10) as u16,
            health: one.field(3, 17, 6) as u8,
            iodc,
            iode,
            ..Self::default()
        };
        eph.clock(one);
        eph.orbit(two);
        eph.orientation(three);
        Some(eph)
    }

    fn clock(&mut self, one: &Subframe) {
        self.tgd = scaled(signed(one.field(7, 17, 8), 8), -31);
        self.toc = scaled(f64::from(one.field(8, 9, 16)), 4);
        self.af2 = scaled(signed(one.field(9, 1, 8), 8), -55);
        self.af1 = scaled(signed(one.field(9, 9, 16), 16), -43);
        self.af0 = scaled(signed(one.field(10, 1, 22), 22), -31);
    }

    fn orbit(&mut self, two: &Subframe) {
        self.crs = scaled(signed(two.field(3, 9, 16), 16), -5);
        self.delta_n = scaled(signed(two.field(4, 1, 16), 16), -43) * GPS_PI;
        self.m0 = scaled(signed(two.joined((4, 17, 8), (5, 1, 24)), 32), -31) * GPS_PI;
        self.cuc = scaled(signed(two.field(6, 1, 16), 16), -29);
        self.e = scaled(f64::from(two.joined((6, 17, 8), (7, 1, 24))), -33);
        self.cus = scaled(signed(two.field(8, 1, 16), 16), -29);
        self.sqrt_a = scaled(f64::from(two.joined((8, 17, 8), (9, 1, 24))), -19);
        self.toe = scaled(f64::from(two.field(10, 1, 16)), 4);
    }

    fn orientation(&mut self, three: &Subframe) {
        self.cic = scaled(signed(three.field(3, 1, 16), 16), -29);
        self.omega0 = scaled(signed(three.joined((3, 17, 8), (4, 1, 24)), 32), -31) * GPS_PI;
        self.cis = scaled(signed(three.field(5, 1, 16), 16), -29);
        self.i0 = scaled(signed(three.joined((5, 17, 8), (6, 1, 24)), 32), -31) * GPS_PI;
        self.crc = scaled(signed(three.field(7, 1, 16), 16), -5);
        self.omega = scaled(signed(three.joined((7, 17, 8), (8, 1, 24)), 32), -31) * GPS_PI;
        self.omega_dot = scaled(signed(three.field(9, 1, 24), 24), -43) * GPS_PI;
        self.idot = scaled(signed(three.field(10, 9, 14), 14), -43) * GPS_PI;
    }
}

/// Subframes 1 to 3 as they arrive, joined into an ephemeris once all three share an issue.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct EphemerisCollector {
    parts: [Option<Subframe>; 3],
}

impl EphemerisCollector {
    pub fn offer(&mut self, subframe: &Subframe) -> Option<Ephemeris> {
        let slot = usize::from(subframe.id).checked_sub(1)?;
        *self.parts.get_mut(slot)? = Some(*subframe);
        let [Some(one), Some(two), Some(three)] = &self.parts else {
            return None;
        };
        Ephemeris::from_subframes(one, two, three)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// Inverse of `decode_subframe`: transmits 24-bit words with the parity IS-GPS-200 asks for.
    pub(in crate::gnss) fn encode_subframe(data: [u32; 10]) -> Vec<bool> {
        let mut d29 = false;
        let mut d30 = false;
        let mut bits = Vec::with_capacity(SUBFRAME_BITS);
        for value in data {
            let transmitted = if d30 { value ^ 0xFF_FFFF } else { value };
            let raw = (0..64)
                .map(|parity| transmitted << 6 | parity)
                .find(|&candidate| check_word(candidate, d29, d30) == Some(value))
                .expect("one parity pattern matches");
            bits.extend((0..30).rev().map(|shift| raw >> shift & 1 != 0));
            d29 = raw & 2 != 0;
            d30 = raw & 1 != 0;
        }
        bits
    }

    const GPS_SPS_TABLE_2_14_PARITY_TERMS: [&[u8]; 6] = [
        &[1, 2, 3, 5, 6, 10, 11, 12, 13, 14, 17, 18, 20, 23],
        &[2, 3, 4, 6, 7, 11, 12, 13, 14, 15, 18, 19, 21, 24],
        &[1, 3, 4, 5, 7, 8, 12, 13, 14, 15, 16, 19, 20, 22],
        &[2, 4, 5, 6, 8, 9, 13, 14, 15, 16, 17, 20, 21, 23],
        &[1, 3, 5, 6, 7, 9, 10, 14, 15, 16, 17, 18, 21, 22, 24],
        &[3, 5, 6, 8, 9, 10, 11, 13, 15, 19, 22, 23, 24],
    ];

    const GPS_SPS_TABLE_2_14_TAKES_PREVIOUS_D29: [bool; 6] =
        [true, false, true, false, false, true];

    fn published_parity(data: u32, previous_d29: bool, previous_d30: bool) -> u32 {
        GPS_SPS_TABLE_2_14_PARITY_TERMS
            .iter()
            .zip(GPS_SPS_TABLE_2_14_TAKES_PREVIOUS_D29)
            .fold(0, |word, (terms, takes_d29)| {
                let seed = if takes_d29 {
                    previous_d29
                } else {
                    previous_d30
                };
                let bit = terms
                    .iter()
                    .fold(seed, |value, &n| value ^ (data >> (24 - n) & 1 != 0));
                (word << 1) | u32::from(bit)
            })
    }

    #[test]
    fn word_parity_follows_the_published_parity_encoding_equations() {
        let mut data = 0x5A_3C7Fu32;
        for round in 0..64 {
            data = data.rotate_left(7) ^ 0x0A_C3B1;
            data &= 0xFF_FFFF;
            let previous_d29 = round & 1 == 1;
            let previous_d30 = round & 2 == 2;
            let parity = published_parity(data, previous_d29, previous_d30);
            let transmitted = if previous_d30 { data ^ 0xFF_FFFF } else { data };
            let raw = transmitted << 6 | parity;
            assert_eq!(
                check_word(raw, previous_d29, previous_d30),
                Some(data),
                "{data:06X} parity {parity:02X}"
            );
            for bit in 0..30 {
                let flipped = raw ^ (1 << bit);
                assert!(
                    check_word(flipped, previous_d29, previous_d30).is_none(),
                    "{data:06X} survived a flip of bit {bit}"
                );
            }
        }
    }

    #[test]
    fn a_subframe_reports_its_id_time_and_survives_inversion_but_not_a_flipped_bit() {
        let mut data = [0_u32; 10];
        data[0] = u32::from(PREAMBLE) << 16;
        data[1] = (100 << 7) | (1 << 2);
        data[2] = 512 << 14;
        let bits = encode_subframe(data);
        let frame = decode_subframe(&bits).expect("valid GPS NAV subframe");
        assert_eq!(frame.id, 1);
        assert_eq!(frame.next_tow_s, 600);
        assert_eq!(frame.start_tow_s(), 594);
        assert_eq!(frame.data, data);
        let inverted: Vec<bool> = bits.iter().map(|bit| !bit).collect();
        assert_eq!(decode_subframe(&inverted).map(|f| f.data), Some(data));
        let mut corrupt = bits;
        corrupt[35] = !corrupt[35];
        assert!(decode_subframe(&corrupt).is_none());
    }

    fn put(data: &mut [u32; 10], word: usize, first: u32, len: u32, value: u32) {
        let shift = 25 - first - len;
        let mask = ((1 << len) - 1) << shift;
        data[word - 1] = (data[word - 1] & !mask) | ((value << shift) & mask);
    }

    fn subframe(id: u8, fill: impl Fn(&mut [u32; 10])) -> Subframe {
        let mut data = [0; 10];
        put(&mut data, 2, 20, 3, u32::from(id));
        fill(&mut data);
        Subframe {
            id,
            next_tow_s: 6,
            data,
            raw: [0; 10],
        }
    }

    #[test]
    fn ephemeris_fields_follow_the_is_gps_200_layout() {
        let one = subframe(1, |d| {
            put(d, 3, 1, 10, 345);
            put(d, 3, 23, 2, 0b10);
            put(d, 7, 17, 8, 0xFF);
            put(d, 8, 1, 8, 77);
            put(d, 8, 9, 16, 22_500);
            put(d, 9, 9, 16, 0xFFFE);
            put(d, 10, 1, 22, 1 << 20);
        });
        let two = subframe(2, |d| {
            put(d, 3, 1, 8, 77);
            put(d, 3, 9, 16, 160);
            put(d, 4, 17, 8, 0x40);
            put(d, 6, 17, 8, 0x01);
            put(d, 8, 17, 8, 0xA1);
            put(d, 9, 1, 24, 0x0D_8C00);
            put(d, 10, 1, 16, 22_500);
        });
        let three = subframe(3, |d| {
            put(d, 9, 1, 24, 0xFF_FFFF);
            put(d, 10, 1, 8, 77);
            put(d, 10, 9, 14, 0x3FFF);
        });
        let eph = Ephemeris::from_subframes(&one, &two, &three).expect("issues agree");
        assert_eq!((eph.week, eph.iodc, eph.iode), (345, 0x24D, 77));
        assert_eq!(eph.tgd, -(2f64.powi(-31)));
        assert_eq!((eph.toc, eph.toe), (360_000.0, 360_000.0));
        assert_eq!(eph.af1, -2.0 * 2f64.powi(-43));
        assert_eq!(eph.af0, 2f64.powi(-11));
        assert_eq!(eph.crs, 5.0);
        assert_eq!(eph.m0, 0.5 * GPS_PI);
        assert_eq!(eph.e, 2f64.powi(-9));
        assert_eq!(eph.sqrt_a, f64::from(0xA10D_8C00_u32) * 2f64.powi(-19));
        assert_eq!(eph.omega_dot, -(2f64.powi(-43)) * GPS_PI);
        assert_eq!(eph.idot, -(2f64.powi(-43)) * GPS_PI);
        let stale = subframe(3, |d| put(d, 10, 1, 8, 78));
        assert!(Ephemeris::from_subframes(&one, &two, &stale).is_none());
    }
}
