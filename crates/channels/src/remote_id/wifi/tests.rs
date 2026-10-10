use sdrmm_wire::{HeightReference, RemoteIdMessage, RemoteIdPhy, RemoteIdTransport, UaStatus};

use sdrmm_modem::wifi::{append_fcs, fcs_ok};

use super::{Mpdu, build, frame};
use crate::remote_id::{ble::tests::bytes, tracker::Tracker};

const PARROT_ANAFI_BEACON: &str = "80000000ffffffffffff903ae65bc8a8903ae65bc8a8303c81e1a60500000000640031040014416e616669546865726d616c2d4130303434313801018c03010105040102000007244348200101140201140301140401140501140601140701140801140901140a01140b01143b125151525354737475767778797a7b7d7e7f802a010030140100000fac040100000fac040100000fac028c002d1a8d011fffff0000000000000000000000000000000000000000003d1601001500000000000000000000000000000000000000dd180050f2020101810003a4000027a4000042545e0061322f007f0104dd0700a0c602020300dd1b9003b7091950493034303434354143304130303434313800000000dd21fa0bbc0d5bf0190110105400009ff3211ce320340500000000d00700004b240000cfb82627";
const ESP32_NAN: &str = "d0000000516f9a01000084cca8604324506f9a01017950060409506f9a130327008869199d92090100101d22f0190150004742522d4f502d313233414243440000000000000000000e040001000222";
const ESP32_BEACON: &str = "80000000ffffffffffff84cca860432484cca860432460060000000000000000b80b2104030106000e4742522d4f502d31323341424344dd21fa0bbc0d22f0190150004742522d4f502d31323341424344000000000000000004";

fn with_fcs(text: &str) -> Vec<u8> {
    append_fcs(bytes(text))
}

fn heard(mpdu: &[u8], phy: RemoteIdPhy) -> sdrmm_wire::RemoteIdFrame {
    frame(
        &Mpdu {
            bytes: mpdu,
            phy,
            channel: Some(1),
            level_dbfs: -30.0,
        },
        &mut Tracker::default(),
    )
    .expect("a Remote ID frame")
}

#[test]
fn a_real_parrot_anafi_beacon_reports_its_position() {
    let mpdu = bytes(PARROT_ANAFI_BEACON);
    assert!(fcs_ok(&mpdu));
    let frame = heard(&mpdu, RemoteIdPhy::Ofdm);
    assert_eq!(frame.transport, RemoteIdTransport::WifiBeacon);
    assert_eq!(frame.address, "90:3A:E6:5B:C8:A8");
    assert_eq!(frame.ssid.as_deref(), Some("AnafiThermal-A004418"));
    assert_eq!(frame.counter, Some(0x5B));
    let [RemoteIdMessage::Location(location)] = &frame.messages[..] else {
        panic!("{:?}", frame.messages);
    };
    assert_eq!(location.status, UaStatus::Ground);
    assert_eq!(location.track_deg, Some(84.0));
    assert_eq!(location.speed_mps, Some(0.0));
    assert!((location.lat.unwrap() - 47.198_710_3).abs() < 1e-9);
    assert!((location.lon.unwrap() - 8.730_237_1).abs() < 1e-9);
    assert_eq!(location.height_m, Some(0.0));
    assert_eq!(location.height_reference, HeightReference::Takeoff);
    assert_eq!(location.seconds_after_hour, Some(929.1));
    let mut broken = mpdu.clone();
    broken[60] ^= 1;
    assert!(!fcs_ok(&broken));
}

#[test]
fn an_esp32_transmitter_sends_the_same_pack_by_beacon_and_nan() {
    let nan = heard(&with_fcs(ESP32_NAN), RemoteIdPhy::Dsss1m);
    let beacon = heard(&with_fcs(ESP32_BEACON), RemoteIdPhy::Dsss1m);
    assert_eq!(nan.transport, RemoteIdTransport::WifiNan);
    assert_eq!(beacon.transport, RemoteIdTransport::WifiBeacon);
    assert_eq!(nan.address, "84:CC:A8:60:43:24");
    assert_eq!(nan.counter, Some(0x22));
    assert_eq!(beacon.ssid.as_deref(), Some("GBR-OP-123ABCD"));
    let operator = RemoteIdMessage::OperatorId {
        id_type: 0,
        operator_id: "GBR-OP-123ABCD".to_owned(),
    };
    assert_eq!(nan.messages, vec![operator.clone()]);
    assert_eq!(beacon.messages, vec![operator]);
}

#[test]
fn ordinary_beacons_are_not_remote_id() {
    let mut mpdu = bytes(ESP32_BEACON);
    mpdu.truncate(mpdu.len() - 35);
    let mpdu = append_fcs(mpdu);
    let found = frame(
        &Mpdu {
            bytes: &mpdu,
            phy: RemoteIdPhy::Dsss1m,
            channel: None,
            level_dbfs: 0.0,
        },
        &mut Tracker::default(),
    );
    assert!(found.is_none());
}

#[test]
fn a_french_beacon_goes_through_the_frame_parser() {
    let tlvs = [1, 1, 1, 3, 4, b'S', b'N', b'0', b'1', 4, 4, 0, 74, 133, 157];
    let mpdu = build::french_beacon([2, 0, 0, 0, 0, 1], "FR", &tlvs);
    let frame = heard(&mpdu, RemoteIdPhy::Dsss1m);
    assert_eq!(frame.transport, RemoteIdTransport::WifiBeaconFrench);
    assert_eq!(frame.uas_id.as_deref(), Some("SN01"));
    assert!(frame.messages.len() == 2, "{:?}", frame.messages);
}

const DJI_BEACON: &str = "80000000ffffffffffff60601f2977cc60601f7a388320007b1db44ab18c73ed66000431000e4d415649435f4149525f5245414c010882848b960c121824dd180050f2020101000003a4000027a4000042435e0062322f00dd5e26371258621310024d06331f455055475430373837475753354949365a66140044b77c005900e400100e480de803b01d68757c1c00000000ec29bc00066cc60159b7ddff489b5cffdc0700383434323633360000000000000000000000006f548482";

#[test]
fn a_dji_drone_id_beacon_names_the_serial() {
    let mpdu = bytes(DJI_BEACON);
    assert!(fcs_ok(&mpdu));
    let frame = heard(&mpdu, RemoteIdPhy::Dsss1m);
    assert_eq!(frame.transport, RemoteIdTransport::WifiBeaconDji);
    assert_eq!(frame.ssid.as_deref(), Some("MAVIC_AIR_REAL"));
    assert_eq!(frame.uas_id.as_deref(), Some("EPUGT0787GWS5II6"));
    assert!(frame.position().is_some());
}
