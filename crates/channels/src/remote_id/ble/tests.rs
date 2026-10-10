use sdrmm_modem::ble::crc_ok;
use sdrmm_wire::{BlePdu, RemoteIdMessage, UaType, UasIdType};

use super::remote_id_payload;
use crate::{ble::pdu::parse, remote_id::odid};

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
    let advert = parse(&frame[..frame.len() - 3], Some(5)).unwrap();
    assert_eq!(advert.kind, BlePdu::AuxAdvInd);
    assert_eq!(advert.sender.unwrap().text(), "E0:7D:EA:EB:2F:1C");
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
    let advert = parse(&pdu, Some(38)).unwrap();
    assert_eq!(advert.kind, BlePdu::AdvNonconnInd);
    assert!(!advert.is_extended());
    assert_eq!(advert.sender.unwrap().text(), "C0:11:22:33:44:55");
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
