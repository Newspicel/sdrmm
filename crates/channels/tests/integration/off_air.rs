#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Real transmissions through the decoder API, each fixture a recording off the air, so these
//! decoders are proven on air and not only against this project's own modulators.
//! `fixtures/README.md` says where each recording comes from.

use num_complex::Complex;
use sdrmm_channels::{
    AisMessage, AprsPacket, CwSkimmerParams, CwSkimmerSpot, MorseParams, MorseText, NavtexMessage,
    PskText, RttyParams, RttyText,
};
use sdrmm_decoder::link::DataLinkMessage;
use sdrmm_decoder::{ChannelCtx, ChannelOutputs};
use sdrmm_dsp::Ddc;
use sdrmm_wire::{
    ChannelParams, ChannelSettings, DecoderEvent, DvFrame, DvFrameKind, EventContract,
};

const BLOCK: usize = 16_384;
const DISCRIMINATOR_RATE: f64 = 48_000.0;

fn cf32(bytes: &[u8]) -> Vec<Complex<f32>> {
    bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|s| {
            Complex::new(
                f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                f32::from_le_bytes([s[4], s[5], s[6], s[7]]),
            )
        })
        .collect()
}

/// Discriminator audio, as a receiver hands it to a digital voice decoder, put back on a carrier.
fn discriminator(bytes: &[u8], deviation_hz: f64) -> Vec<Complex<f32>> {
    let audio: Vec<f32> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&s| f32::from(i16::from_le_bytes(s)))
        .collect();
    let peak = audio.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
    let audio: Vec<f32> = audio.iter().map(|s| s / peak).collect();
    sdrmm_modem::synth::fm_modulate(&audio, deviation_hz, DISCRIMINATOR_RATE)
}

fn tuned(type_id: &str) -> ChannelSettings {
    sdrmm_decoder::default_settings(type_id).unwrap()
}

fn with(type_id: &str, params: &impl serde::Serialize) -> ChannelSettings {
    ChannelSettings {
        params: ChannelParams::new(type_id, params).unwrap(),
        ..tuned(type_id)
    }
}

fn decode(iq: &[Complex<f32>], rate: f64, settings: &ChannelSettings) -> Vec<DecoderEvent> {
    let input_rate = sdrmm_decoder::input_rate(&settings.params);
    let mut ddc = Ddc::new(rate, input_rate, 0.0).unwrap();
    let mut filter = sdrmm_decoder::channel_filter(&settings.params).unwrap();
    let mut rx = sdrmm_decoder::create(ChannelCtx { input_rate }, settings).unwrap();
    let (mut narrow, mut filtered) = (Vec::new(), Vec::new());
    let mut outputs = ChannelOutputs::default();
    let mut events = Vec::new();
    for block in iq.chunks(BLOCK) {
        narrow.clear();
        ddc.process(block, &mut narrow);
        filter.process(&narrow, &mut filtered);
        outputs.reset();
        rx.process(&filtered, &mut outputs);
        events.append(&mut outputs.events);
    }
    events
}

fn read<T: EventContract>(events: &[DecoderEvent]) -> Vec<T> {
    events.iter().filter_map(DecoderEvent::read::<T>).collect()
}

fn text<T: EventContract>(events: &[DecoderEvent], of: impl Fn(&T) -> &str) -> String {
    read::<T>(events).iter().map(of).collect()
}

fn links(events: &[DecoderEvent], kind: &str) -> Vec<DataLinkMessage> {
    events
        .iter()
        .filter(|event| event.kind == kind)
        .filter_map(DecoderEvent::payload)
        .collect()
}

#[test]
fn ais_reads_a_ships_voyage_data() {
    let iq = cf32(include_bytes!(
        "../../../../fixtures/ais_offair_24k.sigmf-data"
    ));
    let heard = read::<AisMessage>(&decode(&iq, 24_000.0, &tuned("ais")));
    let [ship] = &heard[..] else {
        panic!("one message expected: {heard:?}");
    };
    assert_eq!((ship.mmsi, ship.msg_type), (232_004_345, 5));
    assert_eq!(ship.name.as_deref(), Some("HOHEBANK"));
    assert_eq!(ship.destination.as_deref(), Some("VARBERG"));
}

#[test]
fn aprs_reads_packet_traffic_through_a_digipeater() {
    let iq = cf32(include_bytes!(
        "../../../../fixtures/aprs_offair_24k.sigmf-data"
    ));
    let heard: Vec<String> = read::<AprsPacket>(&decode(&iq, 24_000.0, &tuned("aprs")))
        .into_iter()
        .map(|packet| packet.tnc2)
        .collect();
    assert_eq!(
        heard,
        [
            "N2RQ-9>K2YYD-10,W2KPQ-2*:; K2YYD-10 DE N2RQ-9 (FN30FQ) QTC 0...\r",
            "W2KPQ>N2RQ-9:",
            "N2RQ-9>K2YYD-10,W2KPQ-2*:FF\r",
        ]
    );
}

#[test]
fn vdl2_reads_an_acars_downlink_as_dumpvdl2_does() {
    let iq = cf32(include_bytes!(
        "../../../../fixtures/vdl2_offair_48k.sigmf-data"
    ));
    let heard = links(&decode(&iq, 48_000.0, &tuned("vdl2")), "vdl2");
    assert!(heard.iter().all(|m| m.crc_ok), "{heard:?}");
    let acars = heard
        .iter()
        .find(|m| m.message_type == "acars")
        .expect("an ACARS downlink");
    assert_eq!(acars.station.as_deref(), Some("HB-IJW"));
    assert_eq!(acars.text.as_deref(), Some("/EHAM.TI2/040EHAMACFFA"));
    assert_eq!(acars.details["label"], "B9");
    assert!(
        heard.iter().any(|m| m.message_type == "avlc-rr"),
        "{heard:?}"
    );
}

#[test]
fn hfdl_reads_a_squitter_as_dumphfdl_does() {
    let iq = cf32(include_bytes!(
        "../../../../fixtures/hfdl_offair_12k.sigmf-data"
    ));
    let heard = links(&decode(&iq, 12_000.0, &tuned("hfdl")), "hfdl");
    let [squitter] = &heard[..] else {
        panic!("one squitter expected: {heard:?}");
    };
    assert!(squitter.crc_ok);
    assert_eq!(squitter.station.as_deref(), Some("Riverhead, New York"));
    let details = &squitter.details["details"];
    assert_eq!(
        (
            &details["frame_index"],
            &details["frame_offset"],
            &details["systable_version"]
        ),
        (&2397.into(), &1.into(), &52.into())
    );
}

#[test]
fn navtex_reads_a_navigational_warning_without_repair() {
    let iq = cf32(include_bytes!(
        "../../../../fixtures/navtex_offair_1k.sigmf-data"
    ));
    let heard = read::<NavtexMessage>(&decode(&iq, 1_000.0, &tuned("navtex")));
    let [warning] = &heard[..] else {
        panic!("one warning expected: {heard:?}");
    };
    assert_eq!(
        (warning.station, warning.subject, warning.serial),
        (Some('V'), Some('A'), Some(54))
    );
    assert!(
        warning.complete && warning.errors_corrected == 0,
        "{warning:?}"
    );
    assert!(warning.text.contains("SOUTHWOLD LIGHT"), "{warning:?}");
}

#[test]
fn nxdn48_reads_a_call_and_its_talker_alias() {
    let iq = cf32(include_bytes!(
        "../../../../fixtures/nxdn48_offair_12k.sigmf-data"
    ));
    let heard = read::<DvFrame>(&decode(&iq, 12_000.0, &tuned("nxdn")));
    assert!(
        heard
            .iter()
            .any(|f| f.source == Some(901) && f.group_call == Some(true)),
        "{heard:?}"
    );
    assert!(
        heard
            .iter()
            .any(|f| f.talker_alias.as_deref() == Some("REPEATER")),
        "{heard:?}"
    );
}

#[test]
fn nxdn96_reads_a_group_call() {
    let iq = cf32(include_bytes!(
        "../../../../fixtures/nxdn96_offair_48k.sigmf-data"
    ));
    let settings = with(
        "nxdn",
        &sdrmm_channels::NxdnParams {
            bandwidth: sdrmm_channels::NxdnBandwidth::Wide,
            ..Default::default()
        },
    );
    let heard = read::<DvFrame>(&decode(&iq, 48_000.0, &settings));
    assert!(
        heard
            .iter()
            .any(|f| f.source == Some(8_814) && f.color_code == Some(7)),
        "{heard:?}"
    );
}

#[test]
fn p25_reads_a_control_channel() {
    let iq = cf32(include_bytes!(
        "../../../../fixtures/p25_mmr_city_offair_48k.sigmf-data"
    ));
    let heard = read::<DvFrame>(&decode(&iq, 48_000.0, &tuned("p25")));
    assert!(
        heard.iter().all(|f| f.color_code == Some(0x164)),
        "{heard:?}"
    );
    assert!(
        heard
            .iter()
            .any(|f| f.system_id == Some(0x164) && f.site_id == Some(7)),
        "{heard:?}"
    );
}

fn calls(heard: &[DvFrame]) -> Vec<(Option<&str>, Option<&str>, Option<&str>)> {
    heard
        .iter()
        .map(|f| {
            (
                f.source_call.as_deref(),
                f.destination_call.as_deref(),
                f.via.as_deref(),
            )
        })
        .collect()
}

#[test]
fn dstar_reads_a_header_through_a_repeater() {
    let iq = discriminator(
        include_bytes!("../../../../fixtures/dstar_f1zil_48k.s16"),
        1_200.0,
    );
    let heard = read::<DvFrame>(&decode(&iq, DISCRIMINATOR_RATE, &tuned("dstar")));
    assert!(
        calls(&heard).contains(&(Some("F1NSR"), Some("CQCQCQ"), Some("F1ZIL  G"))),
        "{heard:?}"
    );
}

#[test]
fn ysf_reads_a_header_through_a_repeater() {
    let iq = discriminator(
        include_bytes!("../../../../fixtures/ysf_f5zoo_48k.s16"),
        1_944.0,
    );
    let heard = read::<DvFrame>(&decode(&iq, DISCRIMINATOR_RATE, &tuned("ysf")));
    assert!(
        calls(&heard).contains(&(Some("F6FCE"), Some("**********"), Some("F5ZOO-R1"))),
        "{heard:?}"
    );
}

#[test]
fn dpmr_reads_a_call_header() {
    let iq = discriminator(
        include_bytes!("../../../../fixtures/dpmr_dsdcc_48k.s16"),
        1_050.0,
    );
    let heard = read::<DvFrame>(&decode(&iq, DISCRIMINATOR_RATE, &tuned("dpmr")));
    assert!(
        heard
            .iter()
            .any(|f| f.source == Some(289) && f.destination == Some(14_653)),
        "{heard:?}"
    );
    let ends = heard
        .iter()
        .filter(|f| f.kind == DvFrameKind::Terminator)
        .count();
    assert!(ends <= 1, "a call ended {ends} times: {heard:?}");
}

#[test]
fn rtty_reads_a_weather_bulletin() {
    let iq = cf32(include_bytes!(
        "../../../../fixtures/rtty_dwd_offair_2k.sigmf-data"
    ));
    let settings = with(
        "rtty",
        &RttyParams {
            baud: 50.0,
            shift_hz: 450.0,
            invert: true,
            ..RttyParams::default()
        },
    );
    let heard = text::<RttyText>(&decode(&iq, 2_000.0, &settings), |t| &t.text);
    assert!(heard.contains("PLEASE PAY ATTENTION TO LOCAL"), "{heard:?}");
}

fn beacon() -> Vec<Complex<f32>> {
    cf32(include_bytes!(
        "../../../../fixtures/morse_ncdxf_offair_2k.sigmf-data"
    ))
}

#[test]
fn morse_reads_a_beacon_call() {
    for wpm in [None, Some(22.0)] {
        let settings = with(
            "morse",
            &MorseParams {
                wpm,
                ..MorseParams::default()
            },
        );
        let heard = text::<MorseText>(&decode(&beacon(), 2_000.0, &settings), |t| &t.text);
        assert!(heard.contains("OH2B"), "{wpm:?}: {heard:?}");
    }
}

#[test]
fn cw_skimmer_finds_and_reads_a_beacon() {
    let settings = with("cw_skimmer", &CwSkimmerParams::default());
    let heard = read::<CwSkimmerSpot>(&decode(&beacon(), 2_000.0, &settings));
    assert!(
        heard
            .iter()
            .any(|spot| spot.text.contains("OH2B") && (spot.wpm - 22.0).abs() < 1.5),
        "{heard:?}"
    );
}

#[test]
fn psk31_reads_a_cq_call() {
    let iq = cf32(include_bytes!(
        "../../../../fixtures/psk31_offair_1k.sigmf-data"
    ));
    let heard = text::<PskText>(&decode(&iq, 1_000.0, &tuned("psk")), |t| &t.text);
    assert!(heard.contains("CQ CQ CQ de DK9KW DK9KW DK9KW"), "{heard:?}");
}
