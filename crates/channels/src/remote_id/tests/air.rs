use num_complex::Complex;
use sdrmm_wire::RemoteIdPhy;

use crate::{
    remote_id::{
        ble::{
            address_text, advert, channel_index,
            receiver::{Lane, Packet, Syncs},
        },
        wifi::{dsss::Dsss, ofdm::Ofdm, receiver::Burst},
    },
    testutil::cf32_le,
};

const FLUSH: usize = 80_000;

fn flushed(bytes: &[u8]) -> Vec<Complex<f32>> {
    let mut iq = cf32_le(bytes);
    iq.resize(iq.len() + FLUSH, Complex::new(0.0, 0.0));
    iq
}

fn bluetooth(iq: &[Complex<f32>], rf: u8) -> Vec<Packet> {
    let mut lane = Lane::new(Some(rf), channel_index(rf));
    let syncs = Syncs::new();
    let mut packets = Vec::new();
    for block in iq.chunks(4_096) {
        lane.process(block, &syncs, &mut packets);
    }
    packets
}

fn wifi(iq: &[Complex<f32>]) -> Vec<Burst> {
    let mut dsss = Dsss::new();
    let mut ofdm = Ofdm::new();
    let mut bursts = Vec::new();
    for block in iq.chunks(8_192) {
        dsss.process(block, &mut bursts);
        ofdm.process(block, &mut bursts);
    }
    bursts
}

fn ssid(burst: &Burst) -> String {
    let length = usize::from(burst.bytes[37]);
    String::from_utf8_lossy(&burst.bytes[38..38 + length]).into_owned()
}

#[test]
fn a_dji_mini_4_pro_names_itself_on_bluetooth_channel_38() {
    let iq = flushed(include_bytes!(
        "../../../../../fixtures/remote_id_ble_dji_mini4_4m.sigmf-data"
    ));
    let packets = bluetooth(&iq, 12);
    assert_eq!(packets.len(), 8, "{packets:?}");
    assert!(packets.iter().all(|packet| packet.phy == RemoteIdPhy::Le1m));
    let response = packets
        .iter()
        .filter_map(|packet| advert(&packet.pdu))
        .find(|advert| {
            advert
                .data
                .windows(18)
                .any(|name| name == b"DJI-MINI4-Pro-98F8")
        })
        .expect("the scan response carries the drone's name");
    assert_eq!(
        address_text(&response.address.expect("an address")),
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
    assert_eq!(bursts[0].phy, RemoteIdPhy::Dsss1m);
    assert_eq!(bursts[0].bytes.len(), 263);
    assert_eq!(ssid(&bursts[0]), "Xiaomi 13");
}

#[test]
fn an_off_air_cck_frame_passes_its_fcs() {
    let iq = flushed(include_bytes!(
        "../../../../../fixtures/remote_id_wifi_cck_20m.sigmf-data"
    ));
    let mut dsss = Dsss::new();
    let mut bursts = Vec::new();
    for block in iq.chunks(8_192) {
        dsss.process(block, &mut bursts);
    }
    assert_eq!(bursts.len(), 1, "{bursts:?}");
    assert_eq!(bursts[0].phy, RemoteIdPhy::Cck11m);
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
    assert_eq!(bursts[0].phy, RemoteIdPhy::Ofdm);
    assert_eq!(bursts[0].bytes.len(), 270);
    assert_eq!(ssid(&bursts[0]), "MipsTucker");
}
