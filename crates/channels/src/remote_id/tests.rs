use num_complex::Complex;
use sdrmm_wire::{
    ChannelParams, ChannelSettings, DecoderEvent, HeightReference, RemoteIdFrame, RemoteIdLink,
    RemoteIdLocation, RemoteIdMessage, RemoteIdParams, RemoteIdPhy, RemoteIdTransport, UaStatus,
    UaType, UasIdType,
};

use super::{RemoteIdChannel, input_rate};
use crate::{
    ChannelCtx, ChannelError, ChannelOutputs, ChannelRx, channel_filter,
    synth::{
        self,
        remote_id::{self as tx, BlePhy, WifiPhy},
    },
    testutil::settings,
};

const ADDRESS: [u8; 6] = [0xC2, 0x11, 0x22, 0x33, 0x44, 0x55];
const WIFI_ADDRESS: [u8; 6] = [0x60, 0x60, 0x1F, 0x12, 0x34, 0x56];
const CHANNEL_37_HZ: f64 = 2_402e6;
const CHANNEL_38_HZ: f64 = 2_426e6;
const DATA_8_HZ: f64 = 2_420e6;
const WIFI_6_HZ: f64 = 2_437e6;

pub(crate) fn decode(
    link: RemoteIdLink,
    frequency_hz: f64,
    iq: &[Complex<f32>],
) -> Vec<RemoteIdFrame> {
    let params = RemoteIdParams { link };
    let channel_params = ChannelParams::RemoteId(params);
    let ctx = ChannelCtx {
        input_rate: input_rate(&params),
    };
    let tuned = ChannelSettings {
        frequency_hz,
        ..settings(channel_params.clone())
    };
    let mut channel = RemoteIdChannel::new(ctx, tuned).expect("builds");
    let mut filter = channel_filter(&channel_params).expect("filter");
    let mut filtered = Vec::new();
    let mut out = ChannelOutputs::default();
    let mut found = Vec::new();
    for block in iq.chunks(8_192) {
        filter.process(block, &mut filtered);
        out.reset();
        channel.process(&filtered, &mut out);
        found.extend(out.events.drain(..).filter_map(|event| match event {
            DecoderEvent::RemoteId(frame) => Some(frame),
            _ => None,
        }));
    }
    found
}

fn location() -> RemoteIdLocation {
    RemoteIdLocation {
        status: UaStatus::Airborne,
        lat: Some(52.520_008_1),
        lon: Some(13.404_954_3),
        track_deg: Some(93.0),
        speed_mps: Some(8.5),
        vertical_speed_mps: Some(1.0),
        pressure_altitude_m: Some(102.5),
        geodetic_altitude_m: Some(140.0),
        height_m: Some(87.0),
        height_reference: HeightReference::Takeoff,
        horizontal_accuracy_m: Some(3.0),
        vertical_accuracy_m: Some(10.0),
        pressure_accuracy_m: None,
        speed_accuracy_mps: Some(1.0),
        seconds_after_hour: Some(2_017.3),
        timestamp_accuracy_s: Some(0.1),
    }
}

fn basic_id() -> RemoteIdMessage {
    RemoteIdMessage::BasicId {
        id_type: UasIdType::SerialNumber,
        ua_type: UaType::Rotorcraft,
        uas_id: "1581F5FJD239C00DW22E".to_owned(),
    }
}

fn pack() -> Vec<RemoteIdMessage> {
    vec![
        basic_id(),
        RemoteIdMessage::Location(location()),
        RemoteIdMessage::SelfId {
            description_type: 0,
            text: "Bridge inspection".to_owned(),
        },
        RemoteIdMessage::OperatorId {
            id_type: 0,
            operator_id: "DEU12345abcdef".to_owned(),
        },
    ]
}

fn on_air(
    mut signal: Vec<Complex<f32>>,
    rate: f64,
    offset_hz: f64,
    noise: f32,
) -> Vec<Complex<f32>> {
    let gap = (300e-6 * rate) as usize;
    let mut iq = synth::silence(gap);
    synth::scale(&mut signal, 0.3);
    iq.append(&mut signal);
    iq.extend(synth::silence(gap));
    synth::shift(&mut iq, offset_hz, rate);
    synth::add_noise(&mut iq, 7, noise);
    iq
}

#[test]
fn a_legacy_bluetooth_location_carries_the_remembered_id() {
    let rate = RemoteIdLink::Bluetooth.input_rate_hz();
    let mut iq = Vec::new();
    for (counter, message) in [basic_id(), RemoteIdMessage::Location(location())]
        .iter()
        .enumerate()
    {
        let pdu = tx::legacy_pdu(ADDRESS, counter as u8, message);
        iq.extend(on_air(
            tx::bluetooth(&pdu, 0, BlePhy::Le1m, rate),
            rate,
            40e3,
            0.05,
        ));
    }
    let frames = decode(RemoteIdLink::Bluetooth, CHANNEL_37_HZ, &iq);
    assert_eq!(frames.len(), 2, "{frames:?}");
    let last = &frames[1];
    assert_eq!(last.transport, RemoteIdTransport::BluetoothLegacy);
    assert_eq!(last.phy, RemoteIdPhy::Le1m);
    assert_eq!(last.address, "C2:11:22:33:44:55");
    assert_eq!(last.channel, Some(37));
    assert_eq!(last.counter, Some(1));
    assert_eq!(last.uas_id.as_deref(), Some("1581F5FJD239C00DW22E"));
    assert_eq!(last.messages, vec![RemoteIdMessage::Location(location())]);
    assert_eq!(last.position(), Some((52.520_008_1, 13.404_954_3)));
}

fn try_channel(link: RemoteIdLink, frequency_hz: f64) -> Result<RemoteIdChannel, ChannelError> {
    let params = RemoteIdParams { link };
    let ctx = ChannelCtx {
        input_rate: input_rate(&params),
    };
    RemoteIdChannel::new(
        ctx,
        ChannelSettings {
            frequency_hz,
            ..settings(ChannelParams::RemoteId(params))
        },
    )
}

#[test]
fn a_bluetooth_channel_off_the_grid_is_refused() {
    for hz in [145e6, 2_401e6, 2_482e6] {
        assert!(matches!(
            try_channel(RemoteIdLink::Bluetooth, hz),
            Err(ChannelError::InvalidSettings(_))
        ));
    }
    assert!(try_channel(RemoteIdLink::Bluetooth, DATA_8_HZ).is_ok());
}

#[test]
fn long_range_packs_decode_on_a_data_channel_at_both_coding_rates() {
    let rate = RemoteIdLink::Bluetooth.input_rate_hz();
    for (phy, wire) in [
        (BlePhy::CodedS8, RemoteIdPhy::LeCodedS8),
        (BlePhy::CodedS2, RemoteIdPhy::LeCodedS2),
    ] {
        let pdu = tx::aux_adv_pdu(ADDRESS, 9, &pack());
        let iq = on_air(tx::bluetooth(&pdu, 9, phy, rate), rate, -60e3, 0.1);
        let frames = decode(RemoteIdLink::Bluetooth, DATA_8_HZ, &iq);
        assert_eq!(frames.len(), 1, "{phy:?} {frames:?}");
        assert_eq!(frames[0].transport, RemoteIdTransport::BluetoothExtended);
        assert_eq!(frames[0].phy, wire);
        assert_eq!(frames[0].channel, Some(8));
        assert_eq!(frames[0].messages, pack());
    }
}

#[test]
fn a_primary_channel_pointer_carries_no_pack() {
    let rate = RemoteIdLink::Bluetooth.input_rate_hz();
    let pdu = tx::ext_adv_pdu(8, BlePhy::CodedS8);
    let iq = on_air(
        tx::bluetooth(&pdu, 12, BlePhy::CodedS8, rate),
        rate,
        0.0,
        0.05,
    );
    assert!(decode(RemoteIdLink::Bluetooth, CHANNEL_38_HZ, &iq).is_empty());
}

#[test]
fn a_twenty_megahertz_window_hears_every_bluetooth_channel_in_it() {
    let rate = RemoteIdLink::BluetoothBand.input_rate_hz();
    let mut first = tx::bluetooth(
        &tx::legacy_pdu(ADDRESS, 1, &basic_id()),
        12,
        BlePhy::Le1m,
        rate,
    );
    let mut second = tx::bluetooth(
        &tx::aux_adv_pdu([0xD0, 1, 2, 3, 4, 5], 2, &pack()),
        9,
        BlePhy::CodedS8,
        rate,
    );
    synth::shift(&mut second, DATA_8_HZ - CHANNEL_38_HZ, rate);
    first.resize(second.len(), Complex::new(0.0, 0.0));
    let mixed: Vec<Complex<f32>> = first.iter().zip(&second).map(|(a, b)| a + b).collect();
    let frames = decode(
        RemoteIdLink::BluetoothBand,
        CHANNEL_38_HZ,
        &on_air(mixed, rate, 0.0, 0.05),
    );
    let mut channels: Vec<Option<u8>> = frames.iter().map(|frame| frame.channel).collect();
    channels.sort_unstable();
    assert_eq!(channels, vec![Some(8), Some(38)], "{frames:?}");
}

#[test]
fn other_bluetooth_adverts_are_ignored() {
    let rate = RemoteIdLink::Bluetooth.input_rate_hz();
    let mut pdu = vec![0x42, 20];
    pdu.extend(ADDRESS);
    pdu.extend([13, 0xFF, 0x4C, 0x00, 0x02, 0x15, 1, 2, 3, 4, 5, 6, 7, 8]);
    let iq = on_air(tx::bluetooth(&pdu, 39, BlePhy::Le1m, rate), rate, 0.0, 0.05);
    assert!(decode(RemoteIdLink::Bluetooth, 2_480e6, &iq).is_empty());
}

fn wifi(phy: WifiPhy, short_preamble: bool, mpdu: &[u8]) -> Vec<Complex<f32>> {
    let signal = match phy {
        WifiPhy::Ofdm { mbps } => tx::ofdm(mpdu, mbps),
        _ => tx::dsss(mpdu, phy, short_preamble).unwrap(),
    };
    on_air(signal, RemoteIdLink::Wifi.input_rate_hz(), 35e3, 0.02)
}

#[test]
fn wifi_beacons_decode_at_every_rate() {
    let beacon = tx::build::beacon(
        WIFI_ADDRESS,
        "RID-1581F5FJD",
        17,
        &tx::encode::pack(&pack()),
    );
    let rates = [
        (WifiPhy::Dsss1m, false, RemoteIdPhy::Dsss1m),
        (WifiPhy::Dsss2m, true, RemoteIdPhy::Dsss2m),
        (WifiPhy::Cck5m5, false, RemoteIdPhy::Cck5m5),
        (WifiPhy::Cck11m, true, RemoteIdPhy::Cck11m),
    ]
    .into_iter()
    .chain(
        [6, 9, 12, 18, 24, 36, 48, 54]
            .map(|mbps| (WifiPhy::Ofdm { mbps }, false, RemoteIdPhy::Ofdm)),
    );
    for (phy, short_preamble, wire) in rates {
        let frames = decode(
            RemoteIdLink::Wifi,
            WIFI_6_HZ,
            &wifi(phy, short_preamble, &beacon),
        );
        assert_eq!(frames.len(), 1, "{phy:?}");
        let frame = &frames[0];
        assert_eq!(frame.transport, RemoteIdTransport::WifiBeacon);
        assert_eq!(frame.address, "60:60:1F:12:34:56");
        assert_eq!(frame.ssid.as_deref(), Some("RID-1581F5FJD"));
        assert_eq!(frame.channel, Some(6));
        assert_eq!(frame.counter, Some(17));
        assert_eq!(frame.messages, pack(), "{phy:?}");
        assert_eq!(frame.phy, wire);
    }
}

#[test]
fn nan_service_discovery_frames_decode() {
    let frame = tx::build::nan(WIFI_ADDRESS, 4, &tx::encode::pack(&pack()));
    let frames = decode(
        RemoteIdLink::Wifi,
        WIFI_6_HZ,
        &wifi(WifiPhy::Ofdm { mbps: 6 }, false, &frame),
    );
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].transport, RemoteIdTransport::WifiNan);
    assert_eq!(frames[0].counter, Some(4));
    assert_eq!(frames[0].messages, pack());
}

#[test]
fn a_broken_frame_is_counted_on_the_next_one() {
    let mut beacon = tx::build::beacon(WIFI_ADDRESS, "RID", 1, &tx::encode::pack(&pack()));
    let good = wifi(WifiPhy::Ofdm { mbps: 12 }, false, &beacon);
    let last = beacon.len() - 1;
    beacon[last] ^= 0x55;
    let mut iq = wifi(WifiPhy::Ofdm { mbps: 12 }, false, &beacon);
    iq.extend(good);
    let frames = decode(RemoteIdLink::Wifi, WIFI_6_HZ, &iq);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].rejected, 1);
}

#[test]
fn the_signal_generator_scenes_decode() {
    let bluetooth = decode(
        RemoteIdLink::Bluetooth,
        CHANNEL_37_HZ,
        &tx::bluetooth_scene(RemoteIdLink::Bluetooth.input_rate_hz()),
    );
    assert_eq!(bluetooth.len(), 5, "{bluetooth:?}");
    assert!(
        bluetooth
            .iter()
            .all(|frame| frame.uas_id.as_deref() == Some("1581F5FJD239C00DW22E"))
    );
    let wifi = decode(RemoteIdLink::Wifi, 145e6, &tx::wifi_scene().unwrap());
    let transports: Vec<RemoteIdTransport> = wifi.iter().map(|frame| frame.transport).collect();
    assert_eq!(
        transports,
        [RemoteIdTransport::WifiBeacon, RemoteIdTransport::WifiNan]
    );
    for frame in &wifi {
        assert_eq!(frame.messages, tx::demo_drone());
    }
}

mod air;
