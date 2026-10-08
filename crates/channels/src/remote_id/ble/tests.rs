use sdrmm_wire::{RemoteIdMessage, RemoteIdTransport, UaType, UasIdType};

use super::{
    Whitener, address_text, advert, channel_index, crc_ok, crc24, nearest_rf_channel,
    remote_id_payload, rf_channel_hz, whiten,
};
use crate::remote_id::odid;

pub(crate) const BT5_LONG_RANGE_PDU: &str = "07f409091c2febea7de0750ee916faff0d41f01905001253534556544647393337303030373000000000000000001023b5ff7e000000000000000062070000cf07005000000100300044726f6e652049442064656d6f0000000000000000000040040000000000000000010000000000001100000000000000500046494e38376173747264676531326b78797a38000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001a656";

pub(crate) fn bytes(text: &str) -> Vec<u8> {
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn a_sniffed_long_range_advert_passes_the_crc() {
    let frame = bytes(BT5_LONG_RANGE_PDU);
    assert!(crc_ok(&frame));
    let mut broken = frame.clone();
    broken[40] ^= 0x04;
    assert!(!crc_ok(&broken));
}

#[test]
fn a_sniffed_long_range_advert_carries_a_message_pack() {
    let frame = bytes(BT5_LONG_RANGE_PDU);
    let advert = advert(&frame[..frame.len() - 3]).unwrap();
    assert_eq!(advert.transport, RemoteIdTransport::BluetoothExtended);
    assert_eq!(address_text(&advert.address.unwrap()), "E0:7D:EA:EB:2F:1C");
    let (counter, pack) = remote_id_payload(advert.data).unwrap();
    assert_eq!(counter, 0x41);
    let messages = odid::parse(pack).unwrap();
    assert_eq!(messages.len(), 5);
    assert_eq!(
        messages[0],
        RemoteIdMessage::BasicId {
            id_type: UasIdType::SerialNumber,
            ua_type: UaType::Rotorcraft,
            uas_id: "SSEVTFG93700070".to_owned(),
        }
    );
}

#[test]
fn a_legacy_advert_names_its_address_and_service_data() {
    let mut pdu = vec![0x42, 37, 0x55, 0x44, 0x33, 0x22, 0x11, 0xC0];
    pdu.extend_from_slice(&[30, 0x16, 0xFA, 0xFF, 0x0D, 9]);
    pdu.extend_from_slice(&[0x30; 25]);
    let advert = advert(&pdu).unwrap();
    assert_eq!(advert.transport, RemoteIdTransport::BluetoothLegacy);
    assert_eq!(address_text(&advert.address.unwrap()), "C0:11:22:33:44:55");
    let (counter, message) = remote_id_payload(advert.data).unwrap();
    assert_eq!((counter, message.len()), (9, 25));
}

#[test]
fn foreign_service_data_is_not_remote_id() {
    let data = [3, 0x16, 0x6F, 0xFD, 30, 0x16, 0xFA, 0xFF, 0x0C, 1];
    assert_eq!(remote_id_payload(&data), None);
    assert_eq!(remote_id_payload(&[0, 1, 2]), None);
    assert_eq!(remote_id_payload(&[9, 0x16, 0xFA]), None);
}

#[test]
fn advertising_channels_map_to_their_indices() {
    assert_eq!(nearest_rf_channel(2_402e6), Some(0));
    assert_eq!(channel_index(0), 37);
    assert_eq!(channel_index(nearest_rf_channel(2_426e6).unwrap()), 38);
    assert_eq!(channel_index(nearest_rf_channel(2_480.4e6).unwrap()), 39);
    assert_eq!(channel_index(1), 0);
    assert_eq!(channel_index(13), 11);
    assert_eq!(channel_index(38), 36);
    assert_eq!(nearest_rf_channel(2_300e6), None);
    assert_eq!(rf_channel_hz(39), 2_480e6);
}

#[test]
fn whitening_is_its_own_inverse_and_depends_on_the_channel() {
    let original: Vec<u8> = (0..40).collect();
    let mut data = original.clone();
    whiten(37, &mut data);
    assert_ne!(data, original);
    let mut other = original.clone();
    whiten(38, &mut other);
    assert_ne!(data, other);
    whiten(37, &mut data);
    assert_eq!(data, original);
}

#[test]
fn whitening_follows_the_spec_register_for_every_channel() {
    for channel in 0..40u8 {
        let mut register: [bool; 7] =
            std::array::from_fn(|position| position == 0 || channel >> (6 - position) & 1 == 1);
        let mut whitener = Whitener::new(channel);
        for _ in 0..64 {
            let out = register[6];
            register = [
                out,
                register[0],
                register[1],
                register[2],
                register[3] ^ out,
                register[4],
                register[5],
            ];
            assert_eq!(whitener.bit(), out, "channel {channel}");
        }
    }
}

#[test]
fn the_crc_of_nothing_is_the_initial_register() {
    assert_eq!(crc24(&[]), 0xAA_AAAA);
}
