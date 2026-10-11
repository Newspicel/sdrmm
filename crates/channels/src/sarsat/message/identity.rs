use sdrmm_wire::{SarsatBeacon, SarsatProtocol};

use super::Bits;

const BAUDOT: [(u8, char); 39] = [
    (0b111000, 'A'),
    (0b110011, 'B'),
    (0b101110, 'C'),
    (0b110010, 'D'),
    (0b110000, 'E'),
    (0b110110, 'F'),
    (0b101011, 'G'),
    (0b100101, 'H'),
    (0b101100, 'I'),
    (0b111010, 'J'),
    (0b111110, 'K'),
    (0b101001, 'L'),
    (0b100111, 'M'),
    (0b100110, 'N'),
    (0b100011, 'O'),
    (0b101101, 'P'),
    (0b111101, 'Q'),
    (0b101010, 'R'),
    (0b110100, 'S'),
    (0b100001, 'T'),
    (0b111100, 'U'),
    (0b101111, 'V'),
    (0b111001, 'W'),
    (0b110111, 'X'),
    (0b110101, 'Y'),
    (0b110001, 'Z'),
    (0b100100, ' '),
    (0b011000, '-'),
    (0b010111, '/'),
    (0b001101, '0'),
    (0b011101, '1'),
    (0b011001, '2'),
    (0b010000, '3'),
    (0b001010, '4'),
    (0b000001, '5'),
    (0b010101, '6'),
    (0b011100, '7'),
    (0b001100, '8'),
    (0b000011, '9'),
];

const SHORT_BAUDOT_LEAD: u32 = 0b100000;
const BCD_SPACE: u32 = 0b1010;

pub(super) fn protocol(bits: Bits) -> SarsatProtocol {
    if bits.bit(26) {
        return match bits.get(37, 39) {
            0 => SarsatProtocol::Orbitography,
            1 => SarsatProtocol::AviationUser,
            2 => SarsatProtocol::MaritimeUser,
            3 => SarsatProtocol::SerialUser,
            4 => SarsatProtocol::NationalUser,
            5 => SarsatProtocol::SecondGeneration,
            6 => SarsatProtocol::RadioCallSignUser,
            _ => SarsatProtocol::TestUser,
        };
    }
    if !super::is_long(bits) {
        return SarsatProtocol::Spare;
    }
    match bits.get(37, 40) {
        2..=7 => SarsatProtocol::StandardLocation,
        8 | 10 | 11 => SarsatProtocol::NationalLocation,
        9 => SarsatProtocol::EltDtLocation,
        12 => SarsatProtocol::ShipSecurity,
        13 => SarsatProtocol::RlsLocation,
        14 => SarsatProtocol::StandardTest,
        15 => SarsatProtocol::NationalTest,
        _ => SarsatProtocol::Spare,
    }
}

pub(super) fn beacon(bits: Bits, protocol: SarsatProtocol) -> SarsatBeacon {
    match protocol {
        SarsatProtocol::MaritimeUser | SarsatProtocol::RadioCallSignUser => SarsatBeacon::Epirb,
        SarsatProtocol::AviationUser => SarsatBeacon::Elt,
        SarsatProtocol::SerialUser => serial_beacon(bits.get(40, 42)),
        SarsatProtocol::TestUser | SarsatProtocol::StandardTest | SarsatProtocol::NationalTest => {
            SarsatBeacon::Test
        }
        SarsatProtocol::StandardLocation | SarsatProtocol::NationalLocation => {
            location_beacon(bits.get(37, 40))
        }
        SarsatProtocol::ShipSecurity => SarsatBeacon::ShipSecurity,
        SarsatProtocol::RlsLocation => rls_beacon(bits),
        SarsatProtocol::EltDtLocation if elt_dt_test(bits) => SarsatBeacon::Test,
        SarsatProtocol::EltDtLocation => SarsatBeacon::Elt,
        SarsatProtocol::NationalUser
        | SarsatProtocol::Orbitography
        | SarsatProtocol::SecondGeneration
        | SarsatProtocol::Spare => SarsatBeacon::Other,
    }
}

const fn serial_beacon(kind: u32) -> SarsatBeacon {
    match kind {
        0b000 | 0b001 | 0b011 => SarsatBeacon::Elt,
        0b010 | 0b100 => SarsatBeacon::Epirb,
        0b110 => SarsatBeacon::Plb,
        _ => SarsatBeacon::Other,
    }
}

const fn location_beacon(code: u32) -> SarsatBeacon {
    match code {
        2 | 6 | 10 => SarsatBeacon::Epirb,
        3..=5 | 8 => SarsatBeacon::Elt,
        7 | 11 => SarsatBeacon::Plb,
        _ => SarsatBeacon::Other,
    }
}

fn rls_beacon(bits: Bits) -> SarsatBeacon {
    match (bits.get(41, 42), rls_mmsi(bits)) {
        (0b11, _) => SarsatBeacon::Test,
        (0b10, _) => SarsatBeacon::Plb,
        (0b00, false) => SarsatBeacon::Elt,
        _ => SarsatBeacon::Epirb,
    }
}

fn rls_mmsi(bits: Bits) -> bool {
    bits.get(43, 46) == 0b1111
}

fn elt_dt_test(bits: Bits) -> bool {
    matches!(bits.get(43, 66), 0 | 0xFF_FFFF)
}

pub(super) fn identity(bits: Bits, protocol: SarsatProtocol) -> Option<String> {
    let country = bits.get(27, 36);
    match protocol {
        SarsatProtocol::MaritimeUser => Some(maritime(bits, country)),
        SarsatProtocol::RadioCallSignUser => Some(format!("Call sign {}", call_sign(bits))),
        SarsatProtocol::AviationUser => Some(format!("Reg {}", baudot(bits, 40, 7).trim())),
        SarsatProtocol::SerialUser => Some(serial_user(bits)),
        SarsatProtocol::StandardLocation => Some(standard(bits, country)),
        SarsatProtocol::ShipSecurity => Some(mmsi(country, bits.get(41, 60))),
        SarsatProtocol::NationalLocation => Some(format!("ID {}", bits.get(41, 58))),
        SarsatProtocol::RlsLocation => Some(rls(bits, country)),
        SarsatProtocol::EltDtLocation => elt_dt(bits),
        SarsatProtocol::NationalUser
        | SarsatProtocol::TestUser
        | SarsatProtocol::Orbitography
        | SarsatProtocol::SecondGeneration
        | SarsatProtocol::StandardTest
        | SarsatProtocol::NationalTest
        | SarsatProtocol::Spare => None,
    }
}

fn mmsi(country: u32, trailing: u32) -> String {
    format!("MMSI {country:03}{trailing:06}")
}

fn tac(tac: u32, serial: u32) -> String {
    format!("TAC {tac} S/N {serial}")
}

fn operator(designator: &str, serial: u32) -> String {
    format!("{} S/N {serial}", designator.trim())
}

fn aircraft(address: u32) -> String {
    format!("Aircraft {address:06X}")
}

fn maritime(bits: Bits, country: u32) -> String {
    let text = baudot(bits, 40, 6);
    let trimmed = text.trim();
    match trimmed.parse::<u32>() {
        Ok(trailing) if trimmed.len() == 6 => mmsi(country, trailing),
        _ => format!("Call sign {trimmed}"),
    }
}

fn call_sign(bits: Bits) -> String {
    let mut text = baudot(bits, 40, 4);
    for at in [64, 68, 72] {
        match bits.get(at, at + 3) {
            BCD_SPACE => text.push(' '),
            digit => text.extend(char::from_digit(digit, 10)),
        }
    }
    text.trim().to_owned()
}

fn serial_user(bits: Bits) -> String {
    match bits.get(40, 42) {
        0b011 => aircraft(bits.get(44, 67)),
        0b001 => operator(&baudot(bits, 44, 3), bits.get(62, 73)),
        _ if bits.bit(43) => tac(bits.get(74, 83), bits.get(44, 63)),
        _ => format!("S/N {}", bits.get(44, 63)),
    }
}

fn standard(bits: Bits, country: u32) -> String {
    match bits.get(37, 40) {
        2 => mmsi(country, bits.get(41, 60)),
        3 => aircraft(bits.get(41, 64)),
        5 => operator(&short_baudot(bits, 41, 3), bits.get(56, 64)),
        _ => tac(bits.get(41, 50), bits.get(51, 64)),
    }
}

fn rls(bits: Bits, country: u32) -> String {
    if rls_mmsi(bits) {
        return mmsi(country, bits.get(47, 66));
    }
    let series = match bits.get(41, 42) {
        0b00 => 2_000,
        0b01 => 1_000,
        0b10 => 3_000,
        _ => 0,
    };
    tac(series + bits.get(43, 52), bits.get(53, 66))
}

fn elt_dt(bits: Bits) -> Option<String> {
    if elt_dt_test(bits) {
        return None;
    }
    match bits.get(41, 42) {
        0b00 => Some(aircraft(bits.get(43, 66))),
        0b01 => Some(operator(&short_baudot(bits, 43, 3), bits.get(58, 66))),
        0b10 => Some(tac(bits.get(43, 52), bits.get(53, 66))),
        _ => None,
    }
}

fn baudot_char(code: u32) -> char {
    BAUDOT
        .iter()
        .find(|&&(known, _)| u32::from(known) == code)
        .map_or('?', |&(_, letter)| letter)
}

fn baudot(bits: Bits, first: u32, count: u32) -> String {
    (0..count)
        .map(|k| baudot_char(bits.get(first + 6 * k, first + 6 * k + 5)))
        .collect()
}

fn short_baudot(bits: Bits, first: u32, count: u32) -> String {
    (0..count)
        .map(|k| baudot_char(SHORT_BAUDOT_LEAD | bits.get(first + 5 * k, first + 5 * k + 4)))
        .collect()
}

pub(super) fn homing(bits: Bits, protocol: SarsatProtocol, second: bool) -> Option<bool> {
    match protocol {
        SarsatProtocol::MaritimeUser
        | SarsatProtocol::RadioCallSignUser
        | SarsatProtocol::AviationUser
        | SarsatProtocol::SerialUser => Some(bits.get(84, 85) == 0b01),
        SarsatProtocol::StandardLocation
        | SarsatProtocol::NationalLocation
        | SarsatProtocol::ShipSecurity
            if second =>
        {
            Some(bits.bit(112))
        }
        SarsatProtocol::RlsLocation if second => Some(bits.bit(108)),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) fn baudot_code(letter: char) -> Option<u32> {
    BAUDOT
        .iter()
        .find(|&&(_, known)| known == letter)
        .map(|&(code, _)| u32::from(code))
}
