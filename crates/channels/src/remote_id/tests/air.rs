use num_complex::Complex;
use sdrmm_modem::{
    ble::{self, BlePhy, Lane},
    wifi::{self, WifiPhy},
};

use crate::{ble::pdu::parse, testutil::cf32_le};

const FLUSH: usize = 80_000;

#[derive(Debug)]
struct Heard<P> {
    bytes: Vec<u8>,
    phy: P,
}

#[derive(Default)]
struct Packets(Vec<Heard<BlePhy>>);

impl ble::Sink for Packets {
    fn packet(&mut self, packet: ble::Packet<'_>) {
        self.0.push(Heard {
            bytes: packet.pdu.to_vec(),
            phy: packet.phy,
        });
    }
}

#[derive(Default)]
struct Frames(Vec<Heard<WifiPhy>>);

impl wifi::Sink for Frames {
    fn accepts(&self, _: u8) -> bool {
        true
    }

    fn frame(&mut self, frame: wifi::Frame<'_>) {
        self.0.push(Heard {
            bytes: frame.mpdu.to_vec(),
            phy: frame.phy,
        });
    }
}

fn flushed(bytes: &[u8]) -> Vec<Complex<f32>> {
    let mut iq = cf32_le(bytes);
    iq.resize(iq.len() + FLUSH, Complex::new(0.0, 0.0));
    iq
}

fn bluetooth(iq: &[Complex<f32>], rf: u8) -> Vec<Heard<BlePhy>> {
    let mut lane = Lane::new(Some(rf), ble::channel_index(rf)).unwrap();
    let mut packets = Packets::default();
    for block in iq.chunks(4_096) {
        lane.process(block, &mut packets);
    }
    packets.0
}

fn wifi(iq: &[Complex<f32>]) -> Vec<Heard<WifiPhy>> {
    let mut receiver = wifi::Receiver::new().unwrap();
    let mut frames = Frames::default();
    for block in iq.chunks(8_192) {
        receiver.process(block, &mut frames);
    }
    frames.0
}

fn ssid(frame: &Heard<WifiPhy>) -> String {
    let length = usize::from(frame.bytes[37]);
    String::from_utf8_lossy(&frame.bytes[38..38 + length]).into_owned()
}

#[test]
fn a_dji_mini_4_pro_names_itself_on_bluetooth_channel_38() {
    let iq = flushed(include_bytes!(
        "../../../../../fixtures/remote_id_ble_dji_mini4_4m.sigmf-data"
    ));
    let packets = bluetooth(&iq, 12);
    assert_eq!(packets.len(), 8, "{packets:?}");
    assert!(packets.iter().all(|packet| packet.phy == BlePhy::Le1m));
    let response = packets
        .iter()
        .filter_map(|packet| parse(&packet.bytes, Some(38)))
        .find(|advert| {
            advert
                .data
                .windows(18)
                .any(|name| name == b"DJI-MINI4-Pro-98F8")
        })
        .expect("the scan response carries the drone's name");
    assert_eq!(
        response.sender.expect("an address").text(),
        "E4:7A:2C:AB:98:F9"
    );
}

#[test]
fn an_off_air_1_mbit_beacon_passes_its_fcs() {
    let iq = flushed(include_bytes!(
        "../../../../../fixtures/remote_id_wifi_dsss_beacon_20m.sigmf-data"
    ));
    let bursts = wifi(&iq);
    assert_eq!(bursts.len(), 1, "{bursts:?}");
    assert_eq!(bursts[0].phy, WifiPhy::Dsss1m);
    assert_eq!(bursts[0].bytes.len(), 263);
    assert_eq!(ssid(&bursts[0]), "Xiaomi 13");
}

#[test]
fn an_off_air_cck_frame_passes_its_fcs() {
    let iq = flushed(include_bytes!(
        "../../../../../fixtures/remote_id_wifi_cck_20m.sigmf-data"
    ));
    let bursts = wifi(&iq);
    assert_eq!(bursts.len(), 1, "{bursts:?}");
    assert_eq!(bursts[0].phy, WifiPhy::Cck11m);
    assert_eq!(bursts[0].bytes.len(), 220);
    assert_eq!(bursts[0].bytes[0], 0x88);
}

#[test]
fn an_off_air_6_mbit_beacon_passes_its_fcs() {
    let iq = flushed(include_bytes!(
        "../../../../../fixtures/remote_id_wifi_ofdm_beacon_20m.sigmf-data"
    ));
    let bursts = wifi(&iq);
    assert_eq!(bursts.len(), 1, "{bursts:?}");
    assert_eq!(bursts[0].phy, WifiPhy::Ofdm { mbps: 6 });
    assert_eq!(bursts[0].bytes.len(), 270);
    assert_eq!(ssid(&bursts[0]), "MipsTucker");
}
