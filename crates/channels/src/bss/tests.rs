use num_complex::Complex;
use sdrmm_dsp::crc16_x25;
use sdrmm_modem::packet::afsk_burst;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::{ChannelParams, DecoderEvent};

use super::{frame::parse, *};
use sdrmm_decoder_test_support::settings;
use sdrmm_modem::synth::silence;

const RATE: f64 = 48_000.0;
/// Laid out like a VR-N76 frame heard off air: a callsign, a 12-byte location, a text.
const HANDHELD: &str =
    "01 07204E3043414C4C 0D25124F80DDAAF0 0064 0000 005A 0C2468656C6C6F20776F726C64";

fn bss_settings() -> ChannelSettings {
    settings(ChannelParams::new("bss", &BssParams::default()).expect("bss settings"))
}

fn channel() -> BssChannel {
    BssChannel::new(ChannelCtx { input_rate: RATE }, bss_settings()).expect("bss channel")
}

fn keyed(body: &[u8]) -> Vec<Complex<f32>> {
    let mut iq = silence(4_800);
    iq.extend(afsk_burst(body, RATE));
    iq.extend(silence(4_800));
    iq
}

pub(crate) fn fixture() -> crate::builtin::fixtures::Fixture {
    crate::builtin::fixtures::Fixture::new(
        ChannelParams::new("bss", &BssParams::default()).expect("bss settings"),
        keyed(&hex_bytes(HANDHELD)),
    )
}

fn packets(events: &[DecoderEvent]) -> Vec<BssPacket> {
    events
        .iter()
        .map(|event| event.payload::<BssPacket>().expect("a bss packet"))
        .collect()
}

fn hex_bytes(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text.bytes().filter(u8::is_ascii_hexdigit).collect();
    digits
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).expect("ascii"), 16).expect("hex"))
        .collect()
}

fn with_fcs(body: &[u8]) -> Vec<u8> {
    let mut frame = body.to_vec();
    frame.extend(crc16_x25(body).to_le_bytes());
    frame
}

fn heard(frames: &[Vec<u8>]) -> (Vec<BssPacket>, u32) {
    let mut rx = channel();
    let mut out = ChannelOutputs::default();
    for frame in frames {
        rx.listener.hear(frame, &mut out);
    }
    (packets(&out.events), rx.rejected())
}

#[test]
fn reads_a_handheld_frame_off_the_air_once() {
    let mut rx = channel();
    let mut out = ChannelOutputs::default();
    for block in keyed(&hex_bytes(HANDHELD)).chunks(2_048) {
        rx.process(block, &mut out);
    }
    let heard = packets(&out.events);
    assert_eq!(heard.len(), 1, "{heard:?}");
    let packet = &heard[0];
    assert_eq!(packet.source, "N0CALL");
    assert_eq!(packet.message.as_deref(), Some("hello world"));
    assert_eq!(packet.destination, None);
    assert!((packet.lat.expect("lat") - 40.0).abs() < 1e-6);
    assert!((packet.lon.expect("lon") + 75.0).abs() < 1e-6);
    assert_eq!(packet.altitude_m, Some(100));
    assert_eq!(packet.speed_kmh, Some(0));
    assert_eq!(packet.heading_deg, Some(90));
    assert!(packet.unknown.is_empty());
    assert_eq!(rx.rejected(), 0);
}

#[test]
fn reads_a_message_with_a_bare_fix() {
    let packet = parse(&hex_bytes(
        "01 07204E3043414C4C 0121 062468656C6C6F 0725180858062250",
    ))
    .expect("bss");
    assert_eq!(packet.source, "N0CALL");
    assert_eq!(packet.destination, None);
    assert_eq!(packet.message.as_deref(), Some("hello"));
    assert!((packet.lat.expect("lat") - 52.5).abs() < 1e-6);
    assert!((packet.lon.expect("lon") - 13.4).abs() < 1e-6);
    assert_eq!(packet.altitude_m, None);
    assert_eq!(packet.heading_deg, None);
}

#[test]
fn reads_altitude_speed_and_heading_after_the_fix() {
    let packet = parse(&hex_bytes(
        "01 07204E3043414C4C 0D25180858062250 002A 0000 0151",
    ))
    .expect("bss");
    assert_eq!(packet.altitude_m, Some(42));
    assert_eq!(packet.speed_kmh, Some(0));
    assert_eq!(packet.heading_deg, Some(337));
}

#[test]
fn unknown_altitude_speed_and_heading_stay_empty() {
    let packet = parse(&hex_bytes("01 0D25180858062250 8000 FFFF FFFF")).expect("bss");
    assert!(packet.lat.is_some());
    assert_eq!(
        (packet.altitude_m, packet.speed_kmh, packet.heading_deg),
        (None, None, None)
    );
}

#[test]
fn reads_location_and_call_requests_with_their_message_id() {
    let asks = parse(&hex_bytes(
        "01 07204E3043414C4C 850005 0927444C314142432D37",
    ))
    .expect("bss");
    assert_eq!(asks.message_id, Some(5));
    assert_eq!(asks.location_request.as_deref(), Some("DL1ABC-7"));
    let rings = parse(&hex_bytes(
        "01 07204E3043414C4C 850002 0928444C314142432D37",
    ))
    .expect("bss");
    assert_eq!(rings.message_id, Some(2));
    assert_eq!(rings.call_request.as_deref(), Some("DL1ABC-7"));
}

#[test]
fn keeps_unknown_fields_as_hex() {
    let packet = parse(&hex_bytes("01 07204E3043414C4C 0326ABCD")).expect("bss");
    assert_eq!(packet.unknown, ["26:abcd"]);
}

#[test]
fn fields_that_overrun_or_stop_short_are_refused() {
    for body in [
        "01",
        "02 07204E3043414C4C",
        "01 08204E3043414C4C",
        "01 07204E3043414C4C 05",
        "01 07204E3043414C4C 0024",
        "01 0525 180858",
        "01 07204E3043414C4C 8500",
        "01 0725 7FFFFF000000",
    ] {
        assert_eq!(parse(&hex_bytes(body)), None, "{body}");
    }
}

#[test]
fn a_frame_with_a_good_fcs_and_broken_fields_is_counted() {
    let (packets, rejected) = heard(&[with_fcs(&hex_bytes("01 07204E3043414C4C 05"))]);
    assert!(packets.is_empty());
    assert_eq!(rejected, 1);
}

#[test]
fn ax25_and_corrupt_frames_are_not_ours() {
    let ax25 = with_fcs(b"\x82\xa0\xa4\xa6\x40\x40\x60\x88\x98\x62\x82\x84\x86\x61\x03\xf0!");
    let mut corrupt = with_fcs(&hex_bytes("01 07204E3043414C4C 0624 68656C6C6F"));
    corrupt[3] ^= 0b0110_1000;
    let (packets, rejected) = heard(&[ax25, corrupt]);
    assert!(packets.is_empty());
    assert_eq!(rejected, 0);
}

#[test]
fn one_flipped_bit_is_repaired() {
    let mut frame = with_fcs(&hex_bytes("01 07204E3043414C4C 0624 68656C6C6F"));
    frame[12] ^= 0x01;
    let (packets, _) = heard(&[frame]);
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].message.as_deref(), Some("hello"));
}

#[test]
fn every_lane_hearing_one_frame_yields_one_packet() {
    let frame = with_fcs(&hex_bytes("01 07204E3043414C4C 0624 68656C6C6F"));
    let (packets, _) = heard(&vec![frame; 9]);
    assert_eq!(packets.len(), 1);
}

#[test]
fn the_summary_names_sender_addressee_and_what_they_sent() {
    let packet = parse(&hex_bytes(
        "01 07204E3043414C4C 0721444C31414243 0624 68656C6C6F 0725180858062250",
    ))
    .expect("bss");
    let shown = packet.present(&EventKind::named("bss"));
    assert_eq!(shown.summary, "N0CALL → DL1ABC: hello · position");
    assert_eq!(shown.callsign.as_deref(), Some("N0CALL"));
    assert!(shown.place.is_some());
}

#[test]
fn a_bandwidth_past_the_input_rate_is_refused() {
    let wide = settings(
        ChannelParams::new(
            "bss",
            &BssParams {
                bandwidth_hz: 96_000.0,
            },
        )
        .expect("bss settings"),
    );
    assert!(BssChannel::new(ChannelCtx { input_rate: RATE }, wide).is_err());
}

#[test]
fn hears_every_frame_of_the_generator_signal() {
    let signal = &crate::bss::synth::SIGNALS[0];
    let iq = sdrmm_modem::synth::resample(
        &signal.rendered().expect("the bss signal renders"),
        signal.rate_hz,
        RATE,
    );
    let mut rx = channel();
    let mut out = ChannelOutputs::default();
    for block in iq.chunks(2_048) {
        rx.process(block, &mut out);
    }
    let heard = packets(&out.events);
    let sources: Vec<&str> = heard.iter().map(|p| p.source.as_str()).collect();
    assert_eq!(sources, ["DL1ABC-7", "DO2XYZ", "DL1ABC-7"], "{heard:?}");
    assert_eq!(heard[0].message.as_deref(), Some("CQ from the Tor"));
    assert!((heard[1].lat.expect("lat") - 52.4009).abs() < 1e-4);
    assert_eq!(heard[1].heading_deg, Some(250));
    assert_eq!(heard[2].call_request.as_deref(), Some("DO2XYZ"));
    assert_eq!(rx.rejected(), 0);
}
