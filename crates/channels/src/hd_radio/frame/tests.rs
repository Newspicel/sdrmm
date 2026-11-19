use super::*;

#[test]
fn every_control_word_is_recognised_through_four_bit_errors() {
    for &(word, content) in &PCI_WORDS {
        assert_eq!(classify(word), Some(content));
        assert_eq!(classify(word ^ 0b1011_0000_0001), Some(content));
    }
    assert_eq!(classify(0), None);
}

#[test]
fn a_packet_and_its_crc_check_to_zero() {
    let mut packet = b"HDC packet".to_vec();
    let crc = crc8(&packet);
    let check = (0..=255u8)
        .find(|&byte| {
            let mut with = packet.clone();
            with.push(byte);
            crc8(&with) == 0
        })
        .expect("a check byte");
    packet.push(check);
    assert_eq!(crc8(&packet), 0);
    assert_ne!(crc, 0);
}

#[test]
fn the_hdlc_fcs_matches_rfc_1662() {
    let mut frame = b"\x21PSD".to_vec();
    let fcs = !fcs16(&frame);
    frame.extend(fcs.to_le_bytes());
    assert_eq!(fcs16(&frame), FCS_GOOD);
}

#[test]
fn twelve_bit_locations_pack_two_in_three_bytes() {
    let bytes = [0x34, 0x12 | 0x50, 0x67];
    assert_eq!(location(&bytes, 12, 0), 0x234);
    assert_eq!(location(&bytes, 12, 1), 0x675);
    assert_eq!(location(&[0x34, 0x12], 16, 0), 0x1234);
}
