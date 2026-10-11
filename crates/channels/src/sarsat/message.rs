mod identity;
mod position;

use std::sync::LazyLock;

use sdrmm_dsp::SyndromeDecoder;
use sdrmm_wire::{SarsatMessage, SarsatProtocol};

pub(crate) const SHORT_BITS: usize = 88;
pub(crate) const LONG_BITS: usize = 120;
pub(crate) const PDF1_BITS: usize = 82;

const FIRST: u32 = 25;
const LAST: u32 = 144;
const PDF1_END: u32 = 106;
const SHORT_END: u32 = 112;
const PDF2_START: u32 = 107;
const PDF1_GENERATOR: u128 = 0b10_0110_1101_1001_1110_0011;
const PDF2_GENERATOR: u128 = 0b1_0101_0011_1001;
const PDF1_CORRECTABLE: u32 = 3;
const PDF2_CORRECTABLE: u32 = 2;

static PDF1_CODE: LazyLock<SyndromeDecoder> =
    LazyLock::new(|| SyndromeDecoder::new(PDF1_GENERATOR, PDF1_END - FIRST + 1, PDF1_CORRECTABLE));
static PDF2_CODE: LazyLock<SyndromeDecoder> =
    LazyLock::new(|| SyndromeDecoder::new(PDF2_GENERATOR, LAST - PDF2_START + 1, PDF2_CORRECTABLE));

pub(crate) fn prepare() {
    LazyLock::force(&PDF1_CODE);
    LazyLock::force(&PDF2_CODE);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Bits(u128);

impl Bits {
    #[cfg(test)]
    pub(crate) const ALL: Self = Self((1 << LONG_BITS) - 1);

    pub(crate) fn from_sent(sent: impl IntoIterator<Item = bool>) -> Self {
        Self(
            sent.into_iter()
                .take(LONG_BITS)
                .enumerate()
                .fold(0, |word, (k, bit)| {
                    word | u128::from(bit) << (LONG_BITS - 1 - k)
                }),
        )
    }

    const fn mask(first: u32, last: u32) -> u128 {
        (1 << (last - first + 1)) - 1
    }

    pub(crate) const fn range(self, first: u32, last: u32) -> u128 {
        self.0 >> (LAST - last) & Self::mask(first, last)
    }

    pub(crate) const fn get(self, first: u32, last: u32) -> u32 {
        self.range(first, last) as u32
    }

    pub(crate) const fn bit(self, n: u32) -> bool {
        self.range(n, n) == 1
    }

    pub(crate) const fn with(self, first: u32, last: u32, value: u128) -> Self {
        let shift = LAST - last;
        let mask = Self::mask(first, last) << shift;
        Self(self.0 & !mask | (value << shift & mask))
    }

    #[cfg(any(test, feature = "synth"))]
    pub(crate) fn sent(self, long: bool) -> impl Iterator<Item = bool> {
        let len = if long { LONG_BITS } else { SHORT_BITS };
        (0..len).map(move |k| self.0 >> (LONG_BITS - 1 - k) & 1 == 1)
    }
}

fn repair(
    code: &SyndromeDecoder,
    received: Bits,
    weak: Bits,
    first: u32,
    last: u32,
) -> Option<(Bits, u32)> {
    let word = received.range(first, last);
    let (fixed, errors) = code.decode(word)?;
    let doubtful = weak.range(first, last);
    ((fixed ^ word) & !doubtful == 0).then_some((received.with(first, last, fixed), errors))
}

pub(crate) fn pdf1(bits: Bits, weak: Bits) -> Option<(Bits, u32)> {
    repair(&PDF1_CODE, bits, weak, FIRST, PDF1_END)
}

fn pdf2(bits: Bits, weak: Bits) -> Option<(Bits, u32)> {
    repair(&PDF2_CODE, bits, weak, PDF2_START, LAST)
}

pub(crate) fn is_long(bits: Bits) -> bool {
    bits.bit(FIRST)
}

pub(crate) fn decode(bits: Bits, weak: Bits, self_test: bool) -> Option<SarsatMessage> {
    let (bits, pdf1_errors) = pdf1(bits, weak)?;
    let long = is_long(bits);
    let (bits, pdf2_ok, pdf2_errors) = match (long, pdf2(bits, weak)) {
        (false, _) => (bits, None, 0),
        (true, Some((fixed, errors))) => (fixed, Some(true), errors),
        (true, None) => (bits, Some(false), 0),
    };
    let protocol = identity::protocol(bits);
    let second = pdf2_ok == Some(true);
    let (lat, lon) = position::position(bits, protocol, second).unzip();
    Some(SarsatMessage {
        hex_id: hex_id(bits, protocol),
        message: message_hex(bits, long),
        long,
        self_test,
        country: bits.get(27, 36) as u16,
        protocol,
        beacon: identity::beacon(bits, protocol),
        identity: identity::identity(bits, protocol),
        lat,
        lon,
        homing: identity::homing(bits, protocol, second),
        errors_corrected: pdf1_errors + pdf2_errors,
        pdf2_ok,
    })
}

fn hex_id(bits: Bits, protocol: SarsatProtocol) -> String {
    let unlocated = position::defaults(protocol)
        .into_iter()
        .fold(bits, |bits, (first, last, value)| {
            bits.with(first, last, value)
        });
    format!("{:015X}", unlocated.range(26, 85))
}

fn message_hex(bits: Bits, long: bool) -> String {
    if long {
        format!("{:030X}", bits.range(FIRST, LAST))
    } else {
        format!("{:022X}", bits.range(FIRST, SHORT_END))
    }
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn seal(bits: Bits) -> Bits {
    let pdf1 = PDF1_CODE.encode(bits.range(FIRST, 85));
    let sealed = bits.with(FIRST, PDF1_END, pdf1);
    if is_long(sealed) {
        sealed.with(
            PDF2_START,
            LAST,
            PDF2_CODE.encode(bits.range(PDF2_START, 132)),
        )
    } else {
        sealed
    }
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn from_hex(hex: &str) -> Option<Bits> {
    let value = u128::from_str_radix(hex, 16).ok()?;
    let bits = u32::try_from(hex.len() * 4).ok()?;
    let shift = u32::try_from(LONG_BITS).ok()?.checked_sub(bits)?;
    Some(Bits(value << shift))
}

#[cfg(any(test, feature = "synth"))]
pub(crate) use position::encode as encode_position;

#[cfg(test)]
mod tests;
