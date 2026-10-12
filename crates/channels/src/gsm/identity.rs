use std::fmt::Write;

use sdrmm_wire::{GsmIdentity, GsmIdentityKind};

const FILLER: u8 = 0xF;

pub(crate) fn mobile_identity(value: &[u8]) -> Option<GsmIdentity> {
    let (&first, rest) = value.split_first()?;
    let kind = match first & 7 {
        1 => GsmIdentityKind::Imsi,
        2 => GsmIdentityKind::Imei,
        3 => GsmIdentityKind::Imeisv,
        4 => GsmIdentityKind::Tmsi,
        _ => return None,
    };
    if kind == GsmIdentityKind::Tmsi {
        return tmsi(rest.get(..4)?);
    }
    let odd = first & 8 != 0;
    let mut digits = String::with_capacity(1 + 2 * rest.len());
    push_digit(&mut digits, first >> 4)?;
    for (k, &octet) in rest.iter().enumerate() {
        push_digit(&mut digits, octet & 0xF)?;
        let last = k + 1 == rest.len();
        if last && !odd && octet >> 4 == FILLER {
            break;
        }
        push_digit(&mut digits, octet >> 4)?;
    }
    Some(GsmIdentity {
        kind,
        value: digits,
    })
}

pub(crate) fn tmsi(octets: &[u8]) -> Option<GsmIdentity> {
    let octets: [u8; 4] = octets.try_into().ok()?;
    if octets == [0xFF; 4] {
        return None;
    }
    let mut value = String::with_capacity(8);
    for octet in octets {
        let _ = write!(value, "{octet:02X}");
    }
    Some(GsmIdentity {
        kind: GsmIdentityKind::Tmsi,
        value,
    })
}

pub(crate) fn tagged_identity(octets: &[u8], tag: u8) -> Option<GsmIdentity> {
    match octets {
        [found, length, rest @ ..] if *found == tag => {
            mobile_identity(rest.get(..usize::from(*length))?)
        }
        _ => None,
    }
}

pub(crate) fn sized_identity(octets: &[u8]) -> Option<GsmIdentity> {
    let (&length, rest) = octets.split_first()?;
    mobile_identity(rest.get(..usize::from(length))?)
}

fn push_digit(digits: &mut String, nibble: u8) -> Option<()> {
    digits.push(char::from_digit(u32::from(nibble), 10)?);
    Some(())
}

pub(crate) fn plmn(octets: &[u8]) -> Option<(String, String)> {
    let [a, b, c] = octets.get(..3)?.try_into().ok()?;
    let mut mcc = String::with_capacity(3);
    for nibble in [a & 0xF, a >> 4, b & 0xF] {
        push_digit(&mut mcc, nibble)?;
    }
    let mut mnc = String::with_capacity(3);
    push_digit(&mut mnc, c & 0xF)?;
    push_digit(&mut mnc, c >> 4)?;
    if b >> 4 != FILLER {
        push_digit(&mut mnc, b >> 4)?;
    }
    Some((mcc, mnc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_from_the_capture_decode() {
        let imsi = mobile_identity(&[0x29, 0x43, 0x51, 0x09, 0x28, 0x05, 0x46, 0x12]).unwrap();
        assert_eq!(imsi.kind, GsmIdentityKind::Imsi);
        assert_eq!(imsi.value, "234159082506421");
        let tmsi = mobile_identity(&[0xF4, 0x01, 0x5C, 0x57, 0x03]).unwrap();
        assert_eq!(tmsi.label(), "TMSI 015C5703");
        assert_eq!(mobile_identity(&[0xF0]), None);
    }

    #[test]
    fn an_even_identity_drops_its_filler() {
        let imei = mobile_identity(&[0x32, 0x54, 0xF6]).unwrap();
        assert_eq!(imei.kind, GsmIdentityKind::Imei);
        assert_eq!(imei.value, "3456");
    }

    #[test]
    fn location_areas_name_two_and_three_digit_networks() {
        assert_eq!(
            plmn(&[0x62, 0xF2, 0x20]),
            Some(("262".to_owned(), "02".to_owned()))
        );
        assert_eq!(
            plmn(&[0x13, 0x00, 0x62]),
            Some(("310".to_owned(), "260".to_owned()))
        );
    }
}
