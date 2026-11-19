use super::*;

fn push_bits(bits: &mut Vec<u8>, value: u32, count: usize) {
    for i in (0..count).rev() {
        bits.push((value >> i & 1) as u8);
    }
}

fn char5(c: char) -> u32 {
    CHARS5
        .iter()
        .position(|&b| char::from(b) == c)
        .expect("a SIS character") as u32
}

/// A PIDS frame carrying `messages` (ID and payload bits), CRC appended and each byte sent
/// least significant bit first, as the decoder hands it over.
fn frame(messages: &[(u32, Vec<u8>)]) -> [u8; PIDS_BITS] {
    let mut bits = vec![0u8];
    push_bits(&mut bits, u32::from(messages.len() == 2), 1);
    for (id, payload) in messages {
        push_bits(&mut bits, *id, 4);
        bits.extend(payload);
    }
    bits.resize(PIDS_DATA, 0);
    let mut sent: [u8; PIDS_BITS] = std::array::from_fn(|i| bits.get(i).copied().unwrap_or(0));
    let crc = (0..1 << CRC_BITS)
        .find(|&crc| {
            for i in 0..CRC_BITS {
                sent[PIDS_DATA + i] = (crc >> (CRC_BITS - 1 - i) & 1) as u8;
            }
            crc_ok(&sent)
        })
        .expect("a CRC");
    for i in 0..CRC_BITS {
        sent[PIDS_DATA + i] = (crc >> (CRC_BITS - 1 - i) & 1) as u8;
    }
    reorder(&sent)
}

fn short_name(name: &str, fm: bool) -> Vec<u8> {
    let mut bits = Vec::new();
    for c in name.chars() {
        push_bits(&mut bits, char5(c), 5);
    }
    push_bits(&mut bits, u32::from(fm), 2);
    bits
}

fn station_id(country: &str, facility: u32) -> Vec<u8> {
    let mut bits = Vec::new();
    for c in country.chars() {
        push_bits(&mut bits, char5(c), 5);
    }
    push_bits(&mut bits, 0, 3);
    push_bits(&mut bits, facility, 19);
    bits
}

#[test]
fn the_call_sign_and_station_id_are_read_from_one_frame() {
    let mut station = Station::default();
    let sent = frame(&[(1, short_name("WXYZ", true)), (0, station_id("US", 12_345))]);
    assert!(station.push(&sent));
    assert_eq!(station.call_sign.as_deref(), Some("WXYZ-FM"));
    assert_eq!(station.country.as_deref(), Some("US"));
    assert_eq!(station.facility_id, Some(12_345));
    assert_eq!(station.frames_ok, 1);
}

#[test]
fn a_frame_with_a_flipped_bit_fails_its_crc() {
    let mut station = Station::default();
    let mut sent = frame(&[(1, short_name("KABC", false))]);
    sent[20] ^= 1;
    assert!(!station.push(&sent));
    assert_eq!(station.call_sign, None);
    assert_eq!(station.frames_bad, 1);
}

#[test]
fn a_slogan_is_assembled_from_its_frames() {
    let slogan = "Rust Rocks";
    let bytes = slogan.as_bytes();
    let mut first = Vec::new();
    push_bits(&mut first, 0, 4);
    push_bits(&mut first, 1, 1);
    push_bits(&mut first, 0, 3);
    push_bits(&mut first, 0, 3);
    push_bits(&mut first, bytes.len() as u32, 7);
    for &b in &bytes[..5] {
        push_bits(&mut first, u32::from(b), 8);
    }
    let mut second = Vec::new();
    push_bits(&mut second, 1, 4);
    push_bits(&mut second, 1, 1);
    push_bits(&mut second, 0, 5);
    for i in 5..11 {
        push_bits(
            &mut second,
            u32::from(bytes.get(i).copied().unwrap_or(0)),
            8,
        );
    }
    let mut station = Station::default();
    assert!(station.push(&frame(&[(8, second)])));
    assert_eq!(station.slogan, None);
    assert!(station.push(&frame(&[(8, first)])));
    assert_eq!(station.slogan.as_deref(), Some(slogan));
}
