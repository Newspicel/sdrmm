use num_complex::Complex;
use sdrmm_wire::{
    ChannelParams, ChannelSettings, DecoderEvent, LoraBandwidth, LoraCodingRate, LoraFrame,
    LoraIntegrity, LoraIq, LoraKey, LoraParams, LoraPayload, LoraProtocol, LoraSpreadingFactor,
    MeshcoreContent, MeshcoreNodeType, MeshtasticContent, MeshtasticEncryption,
};

use super::{LoraChannel, input_rate, lorawan, meshcore, meshtastic};
use crate::{
    ChannelCtx, ChannelError, ChannelOutputs, ChannelRx, channel_filter,
    synth::lora::LoraWaveform,
    testutil::{cf32_le, settings},
};

fn frames(params: &LoraParams, iq: &[Complex<f32>]) -> Vec<LoraFrame> {
    frames_at(params, 0.0, iq)
}

fn frames_at(params: &LoraParams, carrier_hz: f64, iq: &[Complex<f32>]) -> Vec<LoraFrame> {
    let channel_params = ChannelParams::Lora(params.clone());
    let ctx = ChannelCtx {
        input_rate: input_rate(params),
    };
    let tuned = ChannelSettings {
        frequency_hz: carrier_hz,
        ..settings(channel_params.clone())
    };
    let mut channel = LoraChannel::new(ctx, tuned).expect("builds");
    let mut filter = channel_filter(&channel_params).expect("filter");
    let mut filtered = Vec::new();
    let mut out = ChannelOutputs::default();
    let mut found = Vec::new();
    for block in iq.chunks(4_096) {
        filter.process(block, &mut filtered);
        out.reset();
        channel.process(&filtered, &mut out);
        found.extend(out.events.drain(..).filter_map(|event| match event {
            DecoderEvent::Lora(frame) => Some(frame),
            _ => None,
        }));
    }
    found
}

fn on_air(waveform: &LoraWaveform, payload: &[u8]) -> Vec<Complex<f32>> {
    let rate = waveform.bandwidth_hz * 2.0;
    let mut iq = vec![Complex::new(0.0, 0.0); (0.02 * rate) as usize];
    iq.extend(waveform.frame_at(payload, rate, 0.37e-6));
    iq.resize(iq.len() + (0.2 * rate) as usize, Complex::new(0.0, 0.0));
    crate::synth::shift(&mut iq, 1_200.0, rate);
    crate::synth::add_noise(&mut iq, 11, 0.5);
    iq
}

#[test]
fn a_real_rn2483_capture_decodes_both_frames() {
    let iq = cf32_le(include_bytes!(
        "../../../../fixtures/lora_rn2483_sf7_250k.sigmf-data"
    ));
    let found = frames(&LoraParams::default(), &iq);
    assert_eq!(found.len(), 2, "{found:?}");
    for frame in &found {
        assert_eq!(frame.payload, "deadbeef");
        assert_eq!(frame.integrity, LoraIntegrity::CrcOk);
        assert_eq!(frame.spreading_factor, 7);
        assert_eq!(frame.coding_rate, LoraCodingRate::Cr48);
        assert!(!frame.implicit_header);
        assert!(frame.snr_db > 10.0, "{}", frame.snr_db);
    }
}

#[test]
fn the_gr_lora_sdr_transmitter_output_decodes_bit_exact() {
    let mut iq = cf32_le(include_bytes!(
        "../../../../fixtures/lora_grlorasdr_sf7_250k.sigmf-data"
    ));
    iq.resize(iq.len() + 20_000, Complex::new(0.0, 0.0));
    let found = frames(&LoraParams::default(), &iq);
    let expected = crate::datalink::hex(b"sdrflex lora fixture");
    assert_eq!(found.len(), 2, "{found:?}");
    for frame in &found {
        assert_eq!(frame.payload, expected);
        assert_eq!(frame.integrity, LoraIntegrity::CrcOk);
        assert_eq!(frame.sync_word, 0x12);
    }
}

#[test]
fn real_epfl_frames_decode_from_strong_to_near_the_sensitivity_limit() {
    let cases: [(&[u8], LoraBandwidth, &str); 5] = [
        (
            include_bytes!("../../../../fixtures/lora_epfl_sf7_strong_250k.sigmf-data"),
            LoraBandwidth::Khz125,
            "30303030393637",
        ),
        (
            include_bytes!("../../../../fixtures/lora_epfl_sf7_offgrid_250k.sigmf-data"),
            LoraBandwidth::Khz125,
            "30303030313538",
        ),
        (
            include_bytes!("../../../../fixtures/lora_epfl_sf7_weak_250k.sigmf-data"),
            LoraBandwidth::Khz125,
            "30303031343133",
        ),
        (
            include_bytes!("../../../../fixtures/lora_epfl_sf10_500k.sigmf-data"),
            LoraBandwidth::Khz250,
            "3e0628492d4d374247406ea318587b451a4060",
        ),
        (
            include_bytes!("../../../../fixtures/lora_epfl_sf10_weak_500k.sigmf-data"),
            LoraBandwidth::Khz250,
            "9b056764e6bb81424740938631219a361a4062",
        ),
    ];
    for (data, bandwidth, payload) in cases {
        let params = LoraParams {
            bandwidth,
            ..LoraParams::default()
        };
        let mut iq = cf32_le(data);
        iq.resize(iq.len() + 50_000, Complex::new(0.0, 0.0));
        let found = frames_at(&params, 862_500_000.0, &iq);
        assert!(
            found
                .iter()
                .any(|f| f.payload == payload && f.integrity == LoraIntegrity::CrcOk),
            "{payload}: {found:?}"
        );
    }
}

#[test]
fn a_meshtastic_long_fast_text_reads_end_to_end() {
    let params = LoraParams {
        bandwidth: LoraBandwidth::Khz250,
        spreading_factor: LoraSpreadingFactor::Sf11,
        ..LoraParams::default()
    };
    let packet = meshtastic::text(
        0xa1b2_c3d4,
        0xffff_ffff,
        0x1234_5678,
        "LongFast",
        &[1],
        "hello mesh",
    );
    let waveform = LoraWaveform {
        spreading_factor: 11,
        bandwidth_hz: 250_000.0,
        sync_word: LoraProtocol::MESHTASTIC_SYNC_WORD,
        preamble: 16,
        ..LoraWaveform::default()
    };
    let found = frames(&params, &on_air(&waveform, &packet));
    assert_eq!(found.len(), 1);
    let Some(LoraPayload::Meshtastic(decoded)) = &found[0].decoded else {
        panic!("not Meshtastic: {:?}", found[0]);
    };
    assert_eq!(decoded.from, 0xa1b2_c3d4);
    assert_eq!(decoded.encryption, MeshtasticEncryption::Channel);
    assert_eq!(decoded.channel.as_deref(), Some("LongFast"));
    assert_eq!(
        decoded.content,
        Some(MeshtasticContent::Text {
            text: "hello mesh".to_owned()
        })
    );
    assert_eq!(found[0].station().as_deref(), Some("!a1b2c3d4"));
}

#[test]
fn a_meshcore_advert_names_and_places_its_node() {
    let params = LoraParams {
        bandwidth: LoraBandwidth::Khz62_5,
        spreading_factor: LoraSpreadingFactor::Sf8,
        ..LoraParams::default()
    };
    let packet = meshcore::advert(
        &[7; 32],
        1_760_000_000,
        2,
        "Hilltop",
        Some((47.376_9, 8.541_7)),
    );
    let waveform = LoraWaveform {
        spreading_factor: 8,
        bandwidth_hz: 62_500.0,
        coding_rate: LoraCodingRate::Cr48,
        preamble: 16,
        ..LoraWaveform::default()
    };
    let found = frames(&params, &on_air(&waveform, &packet));
    assert_eq!(found.len(), 1);
    let Some(LoraPayload::Meshcore(decoded)) = &found[0].decoded else {
        panic!("not MeshCore: {:?}", found[0]);
    };
    let MeshcoreContent::Advert {
        node_type,
        signature_ok,
        ..
    } = &decoded.content
    else {
        panic!("not an advert: {decoded:?}");
    };
    assert_eq!(*node_type, MeshcoreNodeType::Repeater);
    assert!(signature_ok);
    assert_eq!(found[0].station().as_deref(), Some("Hilltop"));
    let (lat, lon) = found[0].position().expect("located");
    assert!((lat - 47.376_9).abs() < 1e-5 && (lon - 8.541_7).abs() < 1e-5);
}

#[test]
fn a_lorawan_uplink_decrypts_with_its_session_keys() {
    let nwk_s_key = [0x44; 16];
    let app_s_key = [0xec; 16];
    let packet = lorawan::uplink(0x2601_1bda, 7, 10, b"21.5C", &nwk_s_key, &app_s_key, false);
    let hex = |key: &[u8; 16]| crate::datalink::hex(key);
    let params = LoraParams {
        spreading_factor: LoraSpreadingFactor::Sf9,
        keys: vec![LoraKey::LorawanSession {
            dev_addr: "26011bda".to_owned(),
            nwk_s_key: hex(&nwk_s_key),
            app_s_key: hex(&app_s_key),
        }],
        ..LoraParams::default()
    };
    let waveform = LoraWaveform {
        spreading_factor: 9,
        sync_word: LoraProtocol::LORAWAN_SYNC_WORD,
        ..LoraWaveform::default()
    };
    let found = frames(&params, &on_air(&waveform, &packet));
    assert_eq!(found.len(), 1);
    let Some(LoraPayload::Lorawan(decoded)) = &found[0].decoded else {
        panic!("not LoRaWAN: {:?}", found[0]);
    };
    assert_eq!(decoded.dev_addr.as_deref(), Some("26011bda"));
    assert_eq!(decoded.f_cnt, Some(7));
    assert_eq!(decoded.mic_ok, Some(true));
    assert_eq!(decoded.decrypted.as_deref(), Some("32312e3543"));
}

#[test]
fn a_raw_protocol_leaves_the_payload_undecoded() {
    let params = LoraParams {
        protocol: LoraProtocol::Raw,
        spreading_factor: LoraSpreadingFactor::Sf7,
        iq: LoraIq::Both,
        ..LoraParams::default()
    };
    let packet = meshtastic::text(1, 2, 3, "LongFast", &[1], "raw");
    let waveform = LoraWaveform {
        sync_word: LoraProtocol::MESHTASTIC_SYNC_WORD,
        ..LoraWaveform::default()
    };
    let found = frames(&params, &on_air(&waveform, &packet));
    assert_eq!(found.len(), 1);
    assert!(found[0].decoded.is_none());
    assert_eq!(found[0].payload, crate::datalink::hex(&packet));
}

#[test]
fn malformed_keys_and_layout_changes_are_refused() {
    let params = LoraParams {
        keys: vec![LoraKey::MeshtasticChannel {
            name: "x".to_owned(),
            psk: "not base64!".to_owned(),
        }],
        ..LoraParams::default()
    };
    let ctx = ChannelCtx {
        input_rate: input_rate(&params),
    };
    assert!(matches!(
        LoraChannel::new(ctx, settings(ChannelParams::Lora(params))),
        Err(ChannelError::InvalidSettings(_))
    ));
    let default = LoraParams::default();
    let ctx = ChannelCtx {
        input_rate: input_rate(&default),
    };
    let mut channel =
        LoraChannel::new(ctx, settings(ChannelParams::Lora(default.clone()))).expect("builds");
    let wider = LoraParams {
        bandwidth: LoraBandwidth::Khz250,
        ..default.clone()
    };
    assert!(channel.apply(settings(ChannelParams::Lora(wider))).is_err());
    let keyed = LoraParams {
        protocol: LoraProtocol::Meshtastic,
        keys: vec![LoraKey::MeshtasticChannel {
            name: "Ops".to_owned(),
            psk: "AQ==".to_owned(),
        }],
        ..default
    };
    assert!(channel.apply(settings(ChannelParams::Lora(keyed))).is_ok());
}
