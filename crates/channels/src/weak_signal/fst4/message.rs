use super::super::ftx::message::{CallBook, NTOKENS, call28, grid4, unpack};
use super::code::N;

/// FST4 scrambles its 77 payload bits with this sequence so a run of zeros never goes out.
const SCRAMBLE: [u8; 77] = [
    0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 1, 0, 0,
    1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1,
    1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 0, 1,
];
const PREFIX_LIMIT: u32 = 46_656;
const CALL_BITS: usize = 29;
const CQ_BIT: usize = 26;
const ASSUMED_MARGIN: f32 = 1.1;
const A36: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const MAX_DBM: i32 = 60;

/// A WSPR-style message as FST4W carries it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Beacon {
    pub(crate) text: String,
    pub(crate) callsign: String,
    pub(crate) grid: Option<String>,
    pub(crate) power_dbm: Option<i32>,
}

/// The 77-bit payload a decoded FST4 message holds, scrambling removed.
pub(super) fn standard_payload(bits: &[bool]) -> u128 {
    bits.iter()
        .zip(SCRAMBLE)
        .fold(0u128, |acc, (&bit, scramble)| {
            (acc << 1) | u128::from(u8::from(bit) ^ scramble)
        })
}

/// `llr` with the first call fixed to CQ, beyond anything the channel measured, as WSJT-X's
/// first a-priori pass.
pub(super) fn assume_cq(llr: &[f32; N]) -> [f32; N] {
    let magnitude = ASSUMED_MARGIN * llr.iter().fold(0.0f32, |most, value| most.max(value.abs()));
    let mut assumed = *llr;
    for (bit, slot) in assumed.iter_mut().take(CALL_BITS).enumerate() {
        let one = u8::from(bit == CQ_BIT) ^ SCRAMBLE[bit] == 1;
        *slot = if one { magnitude } else { -magnitude };
    }
    assumed
}

pub(super) fn standard_text(bits: &[bool], book: &mut CallBook) -> Option<String> {
    unpack(standard_payload(bits), book)
}

#[cfg(any(test, feature = "synth"))]
pub(super) fn scrambled(payload: u128) -> Vec<bool> {
    (0..77)
        .map(|index| ((payload >> (76 - index)) & 1 == 1) ^ (SCRAMBLE[index] == 1))
        .collect()
}

struct Reader<'a> {
    bits: &'a [bool],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, count: usize) -> u32 {
        let value = self.bits[self.at..self.at + count]
            .iter()
            .fold(0u32, |acc, &bit| (acc << 1) | u32::from(bit));
        self.at += count;
        value
    }
}

/// Reads the 50-bit message of WSJT-X's `i3=0 n3=6` type: a call with grid and power, a call
/// with prefix or suffix and power, or a hashed call with a six-character grid.
pub(super) fn beacon(bits: &[bool], book: &mut CallBook) -> Option<Beacon> {
    if bits.len() < 50 {
        return None;
    }
    match (bits[47], bits[48], bits[49]) {
        (_, _, true) => compound(bits, book),
        (_, false, false) => plain(bits, book),
        (false, true, false) => hashed(bits, book),
        _ => None,
    }
}

fn power(value: u32) -> Option<i32> {
    let dbm = (f64::from(value) * 10.0 / 3.0).round() as i32;
    (0..=MAX_DBM).contains(&dbm).then_some(dbm)
}

fn station(value: u32, book: &mut CallBook) -> Option<String> {
    let call = call28(value, book)?;
    let bytes = call.as_bytes();
    let plausible = call.starts_with('<')
        || (bytes.iter().any(u8::is_ascii_digit) && bytes.iter().any(u8::is_ascii_alphabetic));
    (plausible && !call.contains(' ')).then_some(call)
}

fn plain(bits: &[bool], book: &mut CallBook) -> Option<Beacon> {
    let mut reader = Reader { bits, at: 0 };
    let callsign = station(reader.take(28), book)?;
    let grid = reader.take(15);
    if grid >= 18 * 18 * 100 {
        return None;
    }
    let grid = grid4(grid);
    let power_dbm = power(reader.take(5))?;
    Some(Beacon {
        text: format!("{callsign} {grid} {power_dbm}"),
        callsign,
        grid: Some(grid),
        power_dbm: Some(power_dbm),
    })
}

fn compound(bits: &[bool], book: &mut CallBook) -> Option<Beacon> {
    let mut reader = Reader { bits, at: 0 };
    let base = station(reader.take(28), book)?;
    let affix = reader.take(16);
    let power_dbm = power(reader.take(5))?;
    let callsign = if affix < PREFIX_LIMIT {
        format!("{}/{base}", base36(affix, 3).trim_start_matches('0'))
    } else {
        format!("{base}/{}", suffix(affix - PREFIX_LIMIT)?)
    };
    book.remember(&callsign);
    Some(Beacon {
        text: format!("{callsign} {power_dbm}"),
        callsign,
        grid: None,
        power_dbm: Some(power_dbm),
    })
}

fn base36(mut value: u32, places: usize) -> String {
    let mut text = vec![b'0'; places];
    for slot in text.iter_mut().rev() {
        *slot = A36[(value % 36) as usize];
        value /= 36;
    }
    String::from_utf8_lossy(&text).into_owned()
}

fn suffix(value: u32) -> Option<String> {
    let char_at = |index: u32| char::from(A36[index as usize]);
    Some(match value {
        0..=35 => char_at(value).to_string(),
        36..=1_295 => format!("{}{}", char_at(value / 36), char_at(value % 36)),
        1_296..=12_959 => format!(
            "{}{}{}",
            char_at(value / 360),
            char_at(value / 10 % 36),
            char_at(value % 10)
        ),
        _ => return None,
    })
}

fn hashed(bits: &[bool], book: &mut CallBook) -> Option<Beacon> {
    let mut reader = Reader { bits, at: 0 };
    let callsign = call28(reader.take(22) + NTOKENS, book)?;
    let grid = grid6(reader.take(25))?;
    Some(Beacon {
        text: format!("{callsign} {grid}"),
        callsign,
        grid: Some(grid),
        power_dbm: None,
    })
}

/// A locator packed with 25 values per subsquare letter, 24 twice meaning none.
fn grid6(value: u32) -> Option<String> {
    let radices = [18u32, 18, 10, 10, 25, 25];
    let mut places = [0u32; 6];
    let mut rest = value;
    for (place, &radix) in places.iter_mut().zip(&radices).rev() {
        *place = rest % radix;
        rest /= radix;
    }
    if rest != 0 || places[4] > 24 || places[5] > 24 {
        return None;
    }
    let letter = |offset: u32| char::from(b'A' + offset as u8);
    let digit = |offset: u32| char::from(b'0' + offset as u8);
    let mut grid = format!(
        "{}{}{}{}",
        letter(places[0]),
        letter(places[1]),
        digit(places[2]),
        digit(places[3])
    );
    if places[4] != 24 || places[5] != 24 {
        grid.push(letter(places[4]));
        grid.push(letter(places[5]));
    }
    Some(grid)
}

/// Packs `CALL GRID DBM`, the plain beacon, into its 50 bits.
#[cfg(any(test, feature = "synth"))]
pub(super) fn pack_beacon(text: &str) -> Option<Vec<bool>> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let [call, grid, dbm] = words.as_slice() else {
        return None;
    };
    let call = super::super::ftx::message::pack28(call)?;
    let grid = grid.as_bytes();
    if grid.len() != 4 {
        return None;
    }
    let grid = u32::from(grid[0].checked_sub(b'A')?) * 1_800
        + u32::from(grid[1].checked_sub(b'A')?) * 100
        + u32::from(grid[2].checked_sub(b'0')?) * 10
        + u32::from(grid[3].checked_sub(b'0')?);
    let dbm: f64 = dbm.parse::<i32>().ok()?.clamp(0, MAX_DBM).into();
    let power = (0.3 * dbm).round() as u32;
    let mut bits = Vec::with_capacity(50);
    for (value, width) in [(call, 28), (grid, 15), (power, 5), (0, 2)] {
        bits.extend((0..width).rev().map(|shift| value >> shift & 1 == 1));
    }
    Some(bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_beacon_round_trips_through_its_bits() {
        let mut book = CallBook::default();
        let bits = pack_beacon("DL0HOT JO60 30").expect("packs");
        assert_eq!(bits.len(), 50);
        let read = beacon(&bits, &mut book).expect("unpacks");
        assert_eq!(read.text, "DL0HOT JO60 30");
        assert_eq!(read.grid.as_deref(), Some("JO60"));
        assert_eq!(read.power_dbm, Some(30));
        assert_eq!(read.callsign, "DL0HOT");
    }

    #[test]
    fn power_follows_wsjtx_rounding() {
        assert_eq!(power(11), Some(37));
        assert_eq!(power(9), Some(30));
        assert_eq!(power(19), None);
    }

    #[test]
    fn a_hashed_call_reads_back_from_the_book() {
        let mut book = CallBook::default();
        book.remember("K1ABC");
        let hash = super::super::super::ftx::message::hash22("K1ABC").expect("hashes");
        let grid = ((((5 * 18 + 13) * 10 + 4) * 10 + 2) * 25 + 7) * 25 + 11;
        let mut bits = Vec::new();
        for (value, width) in [(hash, 22), (grid, 25), (2, 3)] {
            bits.extend((0..width).rev().map(|shift| value >> shift & 1 == 1));
        }
        let read = beacon(&bits, &mut book).expect("unpacks");
        assert_eq!(read.text, "<K1ABC> FN42HL");
        assert_eq!(read.power_dbm, None);
    }

    #[test]
    fn a_suffixed_call_carries_its_power() {
        let mut book = CallBook::default();
        let call = super::super::super::ftx::message::pack28("K1ABC").expect("packs");
        let mut bits = Vec::new();
        for (value, width) in [(call, 28), (PREFIX_LIMIT + 25, 16), (11, 5), (1, 1)] {
            bits.extend((0..width).rev().map(|shift| value >> shift & 1 == 1));
        }
        assert_eq!(
            beacon(&bits, &mut book).expect("unpacks").text,
            "K1ABC/P 37"
        );
    }

    #[test]
    fn scrambling_is_undone() {
        let payload = 0x1234_5678_9abc_def0_1234u128 & ((1 << 77) - 1);
        assert_eq!(standard_payload(&scrambled(payload)), payload);
    }
}
