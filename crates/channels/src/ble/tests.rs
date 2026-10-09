use num_complex::Complex;
use sdrmm_modem::ble::BlePhy as ModemPhy;
use sdrmm_wire::{
    BleAddressKind, BleAdvert, BleBeacon, BleLink, BleParams, BlePdu, BlePhy, ChannelParams,
    ChannelSettings, DecoderEvent,
};

use super::{BleChannel, ad::Fields, pdu::parse};
use crate::{
    ChannelCtx, ChannelOutputs, ChannelRx, channel_filter,
    synth::{self, remote_id::bluetooth},
    testutil::{cf32_le, settings},
};

const ADDRESS: [u8; 6] = [0x55, 0x44, 0x33, 0x22, 0x11, 0xC0];
const CHANNEL_38_HZ: f64 = 2_426e6;

fn advert(kind: u8, data: &[u8]) -> Vec<u8> {
    let mut pdu = vec![kind | 0x40, (ADDRESS.len() + data.len()) as u8];
    pdu.extend(ADDRESS);
    pdu.extend_from_slice(data);
    pdu
}

fn ibeacon() -> Vec<u8> {
    let mut data = vec![0x02, 0x01, 0x06, 0x1A, 0xFF, 0x4C, 0x00, 0x02, 0x15];
    data.extend(0x10..0x20);
    data.extend([0x00, 0x07, 0x01, 0x2C, 0xC5]);
    data
}

#[test]
fn legacy_adverts_name_their_sender_and_its_address_kind() {
    let pdu = advert(0x00, &[0x02, 0x01, 0x06]);
    let parsed = parse(&pdu, Some(38)).unwrap();
    assert_eq!(parsed.kind, BlePdu::AdvInd);
    let sender = parsed.sender.unwrap();
    assert_eq!(sender.text(), "C0:11:22:33:44:55");
    assert_eq!(sender.kind(), BleAddressKind::RandomStatic);
    assert_eq!(parsed.data, &[0x02, 0x01, 0x06]);
    assert_eq!(parse(&pdu[..5], Some(38)), None);
}

#[test]
fn a_scan_request_comes_from_the_scanner_to_the_advertiser() {
    let mut pdu = vec![0x83, 12];
    pdu.extend([1, 2, 3, 4, 5, 0x4A]);
    pdu.extend(ADDRESS);
    let parsed = parse(&pdu, Some(37)).unwrap();
    assert_eq!(parsed.kind, BlePdu::ScanReq);
    assert_eq!(parsed.sender.unwrap().kind(), BleAddressKind::Public);
    assert_eq!(parsed.target.unwrap().text(), "C0:11:22:33:44:55");
    assert!(parsed.target.unwrap().random);
}

#[test]
fn an_extended_advert_points_at_its_auxiliary_packet() {
    let pdu = [0x07, 0x07, 0x06, 0x18, 0x75, 0x0E, 0x05, 0x28, 0x40];
    let parsed = parse(&pdu, Some(37)).unwrap();
    assert_eq!(parsed.kind, BlePdu::AdvExtInd);
    assert_eq!(parsed.sender, None);
    let extended = parsed.extended.unwrap();
    let aux = extended.aux.unwrap();
    assert_eq!((aux.channel, aux.phy, aux.offset_us), (5, BlePhy::LeCoded, 40 * 30));
    let adi = extended.adi.unwrap();
    assert_eq!((adi.set, adi.data_id), (0, 0xE75));
    assert_eq!(parse(&pdu, Some(5)).unwrap().kind, BlePdu::AuxAdvInd);
}

#[test]
fn advertising_data_reads_into_fields() {
    let mut data = vec![0x02, 0x01, 0x06, 0x02, 0x0A, 0xF4];
    data.extend([0x07, 0x09, b'S', b'e', b'n', b's', b'o', b'r']);
    data.extend([0x05, 0x03, 0x0F, 0x18, 0x0A, 0x18]);
    data.extend([0x03, 0x19, 0x41, 0x03]);
    data.extend([0x06, 0xFF, 0x59, 0x00, 0xAA, 0xBB, 0xCC]);
    data.extend([0x0B, 0x24, 0x17, b'/', b'/', b'e', b'x', b'.', b'i', b'o', b'/', b'x']);
    let fields = Fields::read(&data);
    assert_eq!(fields.name.as_deref(), Some("Sensor"));
    assert!(fields.flags.unwrap().general && fields.flags.unwrap().le_only);
    assert_eq!(fields.tx_power_dbm, Some(-12));
    assert_eq!(fields.appearance, Some(0x0341));
    let uuids: Vec<(&str, Option<&str>)> = fields
        .services
        .iter()
        .map(|service| (service.uuid.as_str(), service.name.as_deref()))
        .collect();
    assert_eq!(
        uuids,
        vec![("180F", Some("Battery")), ("180A", Some("Device Information"))]
    );
    assert_eq!(fields.manufacturer[0].company.as_deref(), Some("Nordic Semiconductor"));
    assert_eq!(fields.manufacturer[0].data, "aabbcc");
    assert_eq!(fields.uri.as_deref(), Some("https://ex.io/x"));
}

#[test]
fn beacons_are_recognised() {
    let fields = Fields::read(&ibeacon());
    assert_eq!(
        fields.beacon,
        Some(BleBeacon::Ibeacon {
            uuid: "10111213-1415-1617-1819-1A1B1C1D1E1F".to_owned(),
            major: 7,
            minor: 300,
            measured_dbm: -59,
        })
    );
    let url = [0x0D, 0x16, 0xAA, 0xFE, 0x10, 0xEB, 0x03, b'e', b'x', 0x07, b'/', b'a', b'b', b'c'];
    assert_eq!(
        Fields::read(&url).beacon,
        Some(BleBeacon::EddystoneUrl {
            url: "https://ex.com/abc".to_owned(),
            tx_power_dbm: -21,
        })
    );
    let tlm = [
        0x11, 0x16, 0xAA, 0xFE, 0x20, 0x00, 0x0B, 0xB8, 0x17, 0x80, 0x00, 0x00, 0x01, 0x00, 0x00,
        0x00, 0x27, 0x10,
    ];
    assert_eq!(
        Fields::read(&tlm).beacon,
        Some(BleBeacon::EddystoneTlm {
            battery_mv: Some(3_000),
            temperature_c: Some(23.5),
            adverts: 256,
            uptime_s: 1_000.0,
        })
    );
    let find_my = [0x07, 0xFF, 0x4C, 0x00, 0x12, 0x19, 0x04, 0x00];
    assert_eq!(
        Fields::read(&find_my).beacon,
        Some(BleBeacon::FindMy { maintained: true })
    );
    let service_128 = [
        0x11, 0x07, 0xFB, 0x34, 0x9B, 0x5F, 0x80, 0x00, 0x00, 0x80, 0x00, 0x10, 0x00, 0x00, 0x0F,
        0x18, 0x00, 0x00,
    ];
    assert_eq!(Fields::read(&service_128).services[0].uuid, "180F");
    let custom = [
        0x11, 0x07, 0x9E, 0xCA, 0xDC, 0x24, 0x0E, 0xE5, 0xA9, 0xE0, 0x93, 0xF3, 0xA3, 0xB5, 0x01,
        0x00, 0x40, 0x6E,
    ];
    assert_eq!(
        Fields::read(&custom).services[0].uuid,
        "6E400001-B5A3-F393-E0A9-E50E24DCCA9E"
    );
}

fn decode(link: BleLink, frequency_hz: f64, iq: &[Complex<f32>]) -> Vec<BleAdvert> {
    let channel_params = ChannelParams::Ble(BleParams { link });
    let ctx = ChannelCtx {
        input_rate: link.input_rate_hz(),
    };
    let tuned = ChannelSettings {
        frequency_hz,
        ..settings(channel_params.clone())
    };
    let mut channel = BleChannel::new(ctx, tuned).expect("builds");
    let mut filter = channel_filter(&channel_params).expect("filter");
    let mut filtered = Vec::new();
    let mut out = ChannelOutputs::default();
    let mut found = Vec::new();
    for block in iq.chunks(8_192) {
        filter.process(block, &mut filtered);
        out.reset();
        channel.process(&filtered, &mut out);
        found.extend(out.events.drain(..).filter_map(|event| match event {
            DecoderEvent::Ble(advert) => Some(advert),
            _ => None,
        }));
    }
    found
}

fn air(packets: &[Vec<Complex<f32>>], rate: f64, gap_s: f64) -> Vec<Complex<f32>> {
    let gap = (gap_s * rate) as usize;
    let mut iq = synth::silence(gap);
    for packet in packets {
        let mut packet = packet.clone();
        synth::scale(&mut packet, 0.3);
        iq.extend(packet);
        iq.extend(synth::silence(gap));
    }
    synth::add_noise(&mut iq, 3, 0.02);
    iq
}

#[test]
fn a_beacon_on_channel_38_is_heard_once_per_hold() {
    let rate = BleLink::Channel.input_rate_hz();
    let packet = bluetooth(&advert(0x02, &ibeacon()), 12, ModemPhy::Le1m, rate);
    let iq = air(&vec![packet; 8], rate, 0.1);
    let adverts = decode(BleLink::Channel, CHANNEL_38_HZ, &iq);
    assert_eq!(adverts.len(), 1, "{adverts:?}");
    let first = &adverts[0];
    assert_eq!(first.pdu, BlePdu::AdvNonconnInd);
    assert_eq!(first.channel, Some(38));
    assert_eq!(first.address.as_ref().unwrap().address, "C0:11:22:33:44:55");
    assert_eq!(first.beacon.as_ref().unwrap().label(), "iBeacon");
    assert_eq!(first.manufacturer[0].company.as_deref(), Some("Apple"));
    assert!(first.level_dbfs > -20.0 && first.level_dbfs < -5.0, "{}", first.level_dbfs);
}

#[test]
fn a_held_device_reports_how_often_it_repeated() {
    let rate = BleLink::Channel.input_rate_hz();
    let packet = bluetooth(&advert(0x02, &ibeacon()), 12, ModemPhy::Le1m, rate);
    let iq = air(&vec![packet; 14], rate, 0.1);
    let adverts = decode(BleLink::Channel, CHANNEL_38_HZ, &iq);
    assert_eq!(adverts.len(), 2, "{adverts:?}");
    assert_eq!(adverts[1].repeats, 9);
}

#[test]
fn new_content_is_reported_at_once() {
    let rate = BleLink::Channel.input_rate_hz();
    let named = advert(0x04, &[0x05, 0x09, b'L', b'a', b'm', b'p']);
    let packets = [
        bluetooth(&advert(0x00, &[0x02, 0x01, 0x06]), 12, ModemPhy::Le1m, rate),
        bluetooth(&named, 12, ModemPhy::Le1m, rate),
    ];
    let adverts = decode(BleLink::Channel, CHANNEL_38_HZ, &air(&packets, rate, 0.01));
    let kinds: Vec<BlePdu> = adverts.iter().map(|advert| advert.pdu).collect();
    assert_eq!(kinds, vec![BlePdu::AdvInd, BlePdu::ScanRsp]);
    assert_eq!(adverts[1].name.as_deref(), Some("Lamp"));
}

#[test]
fn the_band_link_hears_long_range_on_a_data_channel() {
    let rate = BleLink::Band.input_rate_hz();
    let mut aux = vec![0x47, 0, 0x09, 0x09];
    aux.extend(ADDRESS);
    aux.extend([0x75, 0x0E, 0x05, 0x09, b'F', b'a', b'r', b'!']);
    aux[1] = (aux.len() - 2) as u8;
    let mut far = bluetooth(&aux, 9, ModemPhy::CodedS8, rate);
    synth::shift(&mut far, 2_420e6 - CHANNEL_38_HZ, rate);
    let adverts = decode(BleLink::Band, CHANNEL_38_HZ, &air(&[far], rate, 0.001));
    assert_eq!(adverts.len(), 1, "{adverts:?}");
    let advert = &adverts[0];
    assert_eq!(advert.pdu, BlePdu::AuxAdvInd);
    assert_eq!(advert.phy, BlePhy::LeCodedS8);
    assert_eq!(advert.channel, Some(8));
    assert_eq!(advert.name.as_deref(), Some("Far!"));
    assert_eq!(advert.adi.unwrap().data_id, 0xE75);
}

#[test]
fn off_air_phones_and_a_drone_on_channel_38() {
    let mut iq = cf32_le(include_bytes!(
        "../../../../fixtures/remote_id_ble_dji_mini4_4m.sigmf-data"
    ));
    iq.resize(iq.len() + 80_000, Complex::new(0.0, 0.0));
    let adverts = decode(BleLink::Channel, CHANNEL_38_HZ, &iq);
    assert!(!adverts.is_empty());
    let drone = adverts
        .iter()
        .find(|advert| advert.name.as_deref() == Some("DJI-MINI4-Pro-98F8"))
        .expect("the drone names itself");
    assert_eq!(drone.address.as_ref().unwrap().address, "E4:7A:2C:AB:98:F9");
    assert_eq!(drone.pdu, BlePdu::ScanRsp);
    assert!(adverts.iter().all(|advert| advert.channel == Some(38)));
}
