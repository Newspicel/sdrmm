use sdrmm_wire::SarsatBeacon;

use super::*;

const SPEC_SHORT: &str = "56E6804002202009655250";
const SPEC_PDF1_BCH: u128 = 0b0_0101_1001_0101_0100_1001;
const SPEC_PDF2_DATA: u128 = 0b10_0101_0111_0000_0000_0001_0111;
const SPEC_PDF2_BCH: u128 = 0b0001_0101_0001;

fn spec_short() -> Bits {
    from_hex(SPEC_SHORT).expect("hex")
}

fn standard_location(lat: f64, lon: f64) -> Bits {
    let bits = Bits::default()
        .with(25, 25, 1)
        .with(27, 36, 366)
        .with(37, 40, 0b0010)
        .with(41, 60, 123_456)
        .with(61, 64, 1)
        .with(107, 110, 0b1101)
        .with(112, 112, 1);
    seal(encode_position(
        bits,
        SarsatProtocol::StandardLocation,
        lat,
        lon,
    ))
}

fn close(got: Option<f64>, want: f64) {
    let got = got.expect("position");
    assert!((got - want).abs() <= 1.0 / 900.0, "{got} vs {want}");
}

#[test]
fn the_published_short_message_checks_and_names_its_beacon() {
    let bits = spec_short();
    assert_eq!(bits.range(86, 106), SPEC_PDF1_BCH);
    assert_eq!(seal(bits), bits);
    let message = decode(bits, Bits::ALL, false).expect("valid");
    assert_eq!(message.hex_id, "ADCD00800440401");
    assert_eq!(message.message, SPEC_SHORT);
    assert!(!message.long);
    assert_eq!(message.country, 366);
    assert_eq!(message.protocol, SarsatProtocol::SerialUser);
    assert_eq!(message.beacon, SarsatBeacon::Epirb);
    assert_eq!(message.identity.as_deref(), Some("S/N 8193"));
    assert_eq!(message.homing, Some(true));
    assert_eq!(message.position(), None);
    assert_eq!(message.pdf2_ok, None);
    assert_eq!(message.errors_corrected, 0);
}

#[test]
fn three_flipped_bits_are_repaired_and_a_fourth_rejects_the_frame() {
    let clean = spec_short();
    let three = Bits(clean.0 ^ (1 << 119 | 1 << 70 | 1 << 40));
    let message = decode(three, Bits::ALL, true).expect("repaired");
    assert_eq!(message.hex_id, "ADCD00800440401");
    assert_eq!(message.errors_corrected, 3);
    assert!(message.self_test);
    assert_eq!(decode(Bits(three.0 ^ 1 << 60), Bits::ALL, false), None);
}

#[test]
fn a_repair_must_only_touch_doubtful_bits() {
    let clean = spec_short();
    let flipped = 1 << 119 | 1 << 70;
    let received = Bits(clean.0 ^ flipped);
    assert_eq!(
        decode(received, Bits(flipped), false).map(|m| m.errors_corrected),
        Some(2)
    );
    assert_eq!(decode(received, Bits(1 << 119), false), None);
    assert_eq!(
        decode(clean, Bits::default(), false).map(|m| m.errors_corrected),
        Some(0)
    );
}

#[test]
fn the_published_second_field_matches_its_bch_and_position() {
    let long = spec_short().with(25, 25, 1).with(107, 132, SPEC_PDF2_DATA);
    let sealed = seal(long);
    assert_eq!(sealed.range(133, 144), SPEC_PDF2_BCH);
    let message = decode(sealed, Bits::ALL, false).expect("valid");
    assert!(message.long);
    assert_eq!(message.pdf2_ok, Some(true));
    assert_eq!(message.message.len(), 30);
    close(message.lat, 43.0 + 32.0 / 60.0);
    close(message.lon, 1.0 + 28.0 / 60.0);
}

#[test]
fn a_standard_location_round_trips_to_four_seconds() {
    let bits = standard_location(-33.8568, 151.2153);
    let message = decode(bits, Bits::ALL, false).expect("valid");
    assert_eq!(message.protocol, SarsatProtocol::StandardLocation);
    assert_eq!(message.beacon, SarsatBeacon::Epirb);
    assert_eq!(message.identity.as_deref(), Some("MMSI 366123456"));
    assert_eq!(message.homing, Some(true));
    close(message.lat, -33.8568);
    close(message.lon, 151.2153);
    let elsewhere = decode(standard_location(10.0, -20.0), Bits::ALL, false).expect("valid");
    assert_eq!(message.hex_id, elsewhere.hex_id);
    assert_eq!(message.hex_id, "2DC43C4802FFBFF");
}

#[test]
fn a_failed_second_field_keeps_the_coarse_fix_and_is_flagged() {
    let bits = standard_location(48.1173, 11.5167);
    let broken = Bits(bits.0 ^ 0b111 << 15);
    let message = decode(broken, Bits::ALL, false).expect("pdf-1 still valid");
    assert_eq!(message.pdf2_ok, Some(false));
    assert_eq!(message.homing, None);
    close(message.lat, 48.0);
    close(message.lon, 11.5);
}

#[test]
fn national_rls_and_elt_dt_positions_round_trip() {
    let national = Bits::default()
        .with(25, 25, 1)
        .with(27, 36, 227)
        .with(37, 40, 0b1011)
        .with(41, 58, 4_242)
        .with(107, 109, 0b110);
    let rls = Bits::default()
        .with(25, 25, 1)
        .with(27, 36, 257)
        .with(37, 40, 0b1101)
        .with(41, 42, 0b10)
        .with(43, 52, 42)
        .with(53, 66, 777);
    let elt_dt = Bits::default()
        .with(25, 25, 1)
        .with(27, 36, 211)
        .with(37, 40, 0b1001)
        .with(43, 66, 0x3C_6444);
    for (bits, protocol, beacon, identity) in [
        (
            national,
            SarsatProtocol::NationalLocation,
            SarsatBeacon::Plb,
            "ID 4242",
        ),
        (
            rls,
            SarsatProtocol::RlsLocation,
            SarsatBeacon::Plb,
            "TAC 3042 S/N 777",
        ),
        (
            elt_dt,
            SarsatProtocol::EltDtLocation,
            SarsatBeacon::Elt,
            "Aircraft 3C6444",
        ),
    ] {
        let sealed = seal(encode_position(bits, protocol, 59.9139, -10.7522));
        let message = decode(sealed, Bits::ALL, false).expect("valid");
        assert_eq!(message.protocol, protocol);
        assert_eq!(message.beacon, beacon);
        assert_eq!(message.identity.as_deref(), Some(identity));
        close(message.lat, 59.9139);
        close(message.lon, -10.7522);
    }
}

#[test]
fn an_aviation_user_reads_its_registration() {
    let registration = "  D-EABC".chars().skip(1);
    let bits = registration.enumerate().fold(
        Bits::default()
            .with(26, 26, 1)
            .with(27, 36, 211)
            .with(37, 39, 0b001),
        |bits, (k, letter)| {
            let at = 40 + 6 * k as u32;
            bits.with(
                at,
                at + 5,
                u128::from(identity::baudot_code(letter).expect("code")),
            )
        },
    );
    let message = decode(seal(bits), Bits::ALL, false).expect("valid");
    assert_eq!(message.beacon, SarsatBeacon::Elt);
    assert_eq!(message.identity.as_deref(), Some("Reg D-EABC"));
    assert_eq!(message.homing, Some(false));
}

#[test]
fn a_location_protocol_in_a_short_frame_is_spare() {
    let bits = seal(Bits::default().with(27, 36, 366).with(37, 40, 0b0010));
    let message = decode(bits, Bits::ALL, false).expect("valid");
    assert_eq!(message.protocol, SarsatProtocol::Spare);
    assert_eq!(message.beacon, SarsatBeacon::Other);
    assert_eq!(message.position(), None);
}

struct Published {
    hex: &'static str,
    hex_id: &'static str,
    country: u16,
    beacon: SarsatBeacon,
    identity: &'static str,
    fix: Option<(f64, f64)>,
}

const AMSA_COMPLIANCE_KIT: [Published; 6] = [
    Published {
        hex: "9F7B00F9E8EC737E5312378A1802B0",
        hex_id: "3EF601F3BF81FE0",
        country: 503,
        beacon: SarsatBeacon::Plb,
        identity: "ID 999",
        fix: Some((-35.738_888_89, 115.493_333_33)),
    },
    Published {
        hex: "A3E7B10016150D364D8B3689C09437",
        hex_id: "47CF62002CFFBFF",
        country: 574,
        beacon: SarsatBeacon::Plb,
        identity: "TAC 708 S/N 22",
        fix: Some((21.041_111_11, 105.49)),
    },
    Published {
        hex: "A0D205F260850F3DC9E0B70B6E4FD7",
        hex_id: "41A40BE4C0FFBFF",
        country: 525,
        beacon: SarsatBeacon::Epirb,
        identity: "MMSI 525024358",
        fix: Some((-4.952_222_22, 121.987_777_78)),
    },
    Published {
        hex: "8DB345B146202DDF3C71F59BAB7072",
        hex_id: "1B668B628CFFBFF",
        country: 219,
        beacon: SarsatBeacon::Elt,
        identity: "Aircraft 45B146",
        fix: Some((32.115_555_56, -110.941_111_11)),
    },
    Published {
        hex: "8E0D0990014710021963C85C7009F5",
        hex_id: "1C1A132002BFDFF",
        country: 224,
        beacon: SarsatBeacon::Elt,
        identity: "TAC 2153 S/N 5",
        fix: Some((27.763_333_33, 0.0)),
    },
    Published {
        hex: "4E86A265C600146DBC407600000000",
        hex_id: "9D0D44CB8C0028D",
        country: 232,
        beacon: SarsatBeacon::Epirb,
        identity: "TAC 163 S/N 78563",
        fix: None,
    },
];

#[test]
fn the_amsa_compliance_kit_decodes_to_its_published_fields() {
    for published in AMSA_COMPLIANCE_KIT {
        let bits = from_hex(published.hex).expect("hex");
        let message = decode(bits, Bits::ALL, false).expect(published.hex);
        assert_eq!(message.errors_corrected, 0, "{}", published.hex);
        assert_ne!(message.pdf2_ok, Some(false), "{}", published.hex);
        assert_eq!(message.hex_id, published.hex_id);
        assert_eq!(message.country, published.country);
        assert_eq!(message.beacon, published.beacon, "{}", published.hex);
        assert_eq!(message.identity.as_deref(), Some(published.identity));
        match published.fix {
            Some((lat, lon)) => {
                close(message.lat, lat);
                close(message.lon, lon);
            }
            None => assert_eq!(message.position(), None),
        }
    }
}
