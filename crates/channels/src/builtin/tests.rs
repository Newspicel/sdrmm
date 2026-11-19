use std::{collections::HashSet, f64::consts::TAU};

use num_complex::Complex;
use sdrmm_decoder::{
    AUDIO_RATE, ChannelCtx, ChannelError, ChannelFilter, ChannelFront, NativeChannel,
    audio_channels, channel_filter, create, default_settings, descriptor, descriptors,
    encoder::{EncoderError, EncoderParams, encoder},
    input_rate, occupied_band,
};
use sdrmm_wire::{
    CallGate, Capability, ChannelParams, ChannelSettings, EncoderInput, EventKind, KindView,
    Sideband,
};

use super::*;
use crate::{
    NfmChannel,
    am::AmParams,
    atv::{AtvColor, AtvParams},
    datv::DatvParams,
    nfm::NfmParams,
    ssb::SsbParams,
    wfm::WfmParams,
};
use sdrmm_decoder_test_support::settings;

fn default_params(type_id: &str) -> ChannelParams {
    ChannelParams::default_for(type_id).unwrap_or_else(|| panic!("unexpected type id {type_id}"))
}

#[test]
fn a_datv_input_rate_follows_its_symbol_rate() {
    let narrow = ChannelParams::new("datv", &DatvParams::default()).expect("datv settings");
    let wide = ChannelParams::new(
        "datv",
        &DatvParams {
            symbol_rate: 2_330_000.0,
            ..DatvParams::default()
        },
    )
    .expect("datv settings");
    assert_eq!(input_rate(&narrow), 2_000_000.0);
    assert_eq!(input_rate(&wide), 4_000_000.0);
    assert_eq!(
        input_rate(&ChannelParams::new("nfm", &NfmParams::default()).expect("nfm settings")),
        NfmChannel::descriptor().input_rate_hz
    );
}

#[test]
fn descriptor_lookup_matches_catalogue() {
    let catalogue = descriptors();
    for expected in &catalogue {
        assert_eq!(descriptor(&expected.type_id).as_ref(), Some(expected));
    }
    assert!(descriptor("unknown").is_none());
}

#[test]
fn only_protocols_the_identifier_recognizes_are_identifiable() {
    let identifiable = |kind: &str| descriptor(kind).is_some_and(|d| d.identifiable);
    for kind in [
        "nfm", "am", "wfm", "ssb", "dmr", "tetra", "pocsag", "adsb", "ft8",
    ] {
        assert!(identifiable(kind), "{kind}");
    }
    assert!(!identifiable("ident"));
    for descriptor in descriptors() {
        assert_eq!(
            descriptor.identifiable,
            !descriptor.signatures.is_empty(),
            "{}",
            descriptor.type_id
        );
    }
}

#[test]
fn every_descriptor_has_a_short_summary() {
    for d in descriptors() {
        assert!(!d.summary.is_empty(), "{} has no summary", d.type_id);
        assert!(d.summary.len() <= 60, "{} summary is too long", d.type_id);
    }
}

#[test]
fn every_descriptor_says_how_it_is_verified() {
    for d in descriptors() {
        assert_ne!(
            d.verification,
            sdrmm_wire::Verification::Unverified,
            "{} does not say how it is verified",
            d.type_id
        );
    }
}

#[test]
fn descriptors_are_unique_and_complete() {
    let all = descriptors();
    let ids: HashSet<&str> = all.iter().map(|d| d.type_id.as_str()).collect();
    assert_eq!(ids.len(), all.len());
    for d in &all {
        let (bandwidth, rate) = match d.type_id.as_str() {
            "nfm" => (12_500.0, 48_000.0),
            "selcall" | "dtmf" | "mdc1200" | "fleetsync" => (12_500.0, 48_000.0),
            "am" => (10_000.0, 48_000.0),
            "ssb" => (3_000.0, 48_000.0),
            "wfm" => (200_000.0, 240_000.0),
            "pocsag" => (12_500.0, 24_000.0),
            "flex" | "ermes" | "eot" | "mpt1327" => (12_500.0, 48_000.0),
            "sarsat" => (20_000.0, 24_000.0),
            "tetrapol" => (12_500.0, 48_000.0),
            "adsb" => (2_000_000.0, 2_400_000.0),
            "ais" => (25_000.0, 48_000.0),
            "aprs" | "bss" => (12_500.0, 48_000.0),
            "rtty" => (1_000.0, 8_000.0),
            "morse" => (400.0, 8_000.0),
            "cw_skimmer" => (24_000.0, 48_000.0),
            "navtex" => (600.0, 8_000.0),
            "acars" => (12_500.0, 24_000.0),
            "atv" => (1_500_000.0, 20_000_000.0),
            "sstv" => (1_600.0, 16_000.0),
            "dab" => (1_536_000.0, 2_048_000.0),
            "datv" => (449_550.0, 2_000_000.0),
            "dvbt" => (8_000_000.0, 64_000_000.0 / 7.0),
            "drm" => (100_000.0, 192_000.0),
            "hd_radio" => (
                1_092.0 * 1_488_375.0 / 4_096.0,
                1_536.0 * 1_488_375.0 / 4_096.0,
            ),
            "dmr" | "ysf" | "p25" | "p25p2" => (12_500.0, 48_000.0),
            "dstar" | "nxdn" | "dpmr" => (6_250.0, 48_000.0),
            "m17" => (9_000.0, 48_000.0),
            "edacs" => (25_000.0, 48_000.0),
            "ltr" => (12_500.0, 48_000.0),
            "tetra" => (25_000.0, 72_000.0),
            "ft8" | "ft4" | "wspr" | "msk144" | "q65" | "jt65" | "jt9" | "js8" | "fst4"
            | "fst4w" => (3_200.0, 12_000.0),
            "psk" => (650.0, 8_000.0),
            "freedv" => (1_400.0, 8_000.0),
            "ident" => (192_000.0, 240_000.0),
            "radio_clock" => (200.0, 2_000.0),
            "gnss" => (2_046_000.0, 2_048_000.0),
            "vor" => (24_000.0, 48_000.0),
            "ils" => (20_000.0, 48_000.0),
            "dsc" => (500.0, 8_000.0),
            "inmarsat_stdc" => (4_000.0, 12_000.0),
            "inmarsat_aero" => (13_000.0, 48_000.0),
            "vdl2" => (17_000.0, 100_000.0),
            "hfdl" => (6_000.0, 12_000.0),
            "iridium" => (50_000.0, 250_000.0),
            "dect" => (1_728_000.0, 2_304_000.0),
            "gsm" => (200_000.0, 1_625_000.0 / 3.0),
            "apt" => (40_000.0, 60_000.0),
            "lrpt" => (140_000.0, 288_000.0),
            "wenet" => (300_000.0, 921_416.0),
            "hrpt" => (2_400_000.0, 2_661_600.0),
            "goes_hrit" => (1_205_100.0, 1_854_000.0),
            "gk2a_lrit" => (192_000.0, 256_000.0),
            "wefax" => (1_600.0, 12_000.0),
            "same" => (12_500.0, 50_000.0),
            "radiosonde" => (20_000.0, 48_000.0),
            "horus" => (6_000.0, 8_000.0),
            "lora" => (125_000.0, 250_000.0),
            "remote_id" | "ble" => (2_000_000.0, 4_000_000.0),
            "wifi_occupancy" | "ism_survey" => (17_000_000.0, 20_000_000.0),
            "ism" => (240_000.0, 250_000.0),
            "wmbus" => (300_000.0, 400_000.0),
            "noise_figure" => (400_000.0, 500_000.0),
            "per" => (28_800.0, 96_000.0),
            "sid" => (42_000.0, 48_000.0),
            "stanag" => (3_400.0, 9_600.0),
            "uat" => (1_300_000.0, 2_083_334.0),
            other => panic!("unexpected type id {other}"),
        };
        assert_eq!(d.bandwidth_hz, bandwidth, "{}", d.type_id);
        assert_eq!(d.input_rate_hz, rate, "{}", d.type_id);
        assert!(!d.name.is_empty(), "{}", d.type_id);
        assert!(
            d.has_audio || !d.emits.is_empty() || d.has_video,
            "{} produces neither audio, decoder events nor video",
            d.type_id
        );
        assert_eq!(
            d.has_audio,
            matches!(
                d.type_id.as_str(),
                "nfm"
                    | "am"
                    | "ssb"
                    | "wfm"
                    | "atv"
                    | "dab"
                    | "datv"
                    | "drm"
                    | "hd_radio"
                    | "dvbt"
                    | "dmr"
                    | "dstar"
                    | "ysf"
                    | "nxdn"
                    | "p25"
                    | "p25p2"
                    | "dpmr"
                    | "m17"
                    | "freedv"
                    | "dect"
                    | "edacs"
                    | "ltr"
            ),
            "{} audio flag does not match its mode class",
            d.type_id
        );
        assert_eq!(
            d.has_video,
            matches!(
                d.type_id.as_str(),
                "atv"
                    | "sstv"
                    | "datv"
                    | "dvbt"
                    | "apt"
                    | "lrpt"
                    | "hrpt"
                    | "goes_hrit"
                    | "gk2a_lrit"
                    | "wefax"
                    | "wenet"
            ),
            "{} video flag does not match its mode class",
            d.type_id
        );
    }
}

fn may_be_unset(field: &sdrmm_wire::SettingField) -> bool {
    use sdrmm_wire::Control;
    field.fallback.is_some()
        || !field.when.is_empty()
        || match &field.control {
            Control::Number(spec) => spec.optional.is_some(),
            Control::Text { optional, .. } => *optional,
            Control::Choice { options } => options.iter().any(|option| option.value.is_null()),
            Control::Live { .. }
            | Control::List { .. }
            | Control::Preset { .. }
            | Control::SerialPort { .. } => true,
            Control::Toggle | Control::Lanes { .. } | Control::Numbers { .. } => false,
        }
}

#[test]
fn every_form_field_names_a_setting_its_defaults_carry() {
    for descriptor in descriptors() {
        let Some(defaults) = &descriptor.defaults else {
            panic!("{} has no defaults", descriptor.type_id);
        };
        let settings = &defaults.params.settings;
        for field in descriptor
            .settings
            .iter()
            .filter(|field| !may_be_unset(field))
        {
            assert!(
                sdrmm_wire::channel::lookup(settings, &field.key).is_some(),
                "{} form names {} which its settings lack",
                descriptor.type_id,
                field.key
            );
        }
        assert!(
            defaults.check_limits(&descriptor.settings).is_ok(),
            "{} defaults break their own form",
            descriptor.type_id
        );
    }
}

#[test]
fn every_emitted_kind_is_in_the_catalog() {
    let catalog = sdrmm_wire::catalog();
    for descriptor in descriptors() {
        for kind in &descriptor.emits {
            assert!(
                catalog.entry(kind).is_some(),
                "{} emits unknown {kind}",
                descriptor.type_id
            );
        }
    }
}

#[test]
fn every_view_hint_is_one_the_frontend_can_draw() {
    for entry in sdrmm_wire::catalog().entries() {
        for view in &entry.views {
            let fields = view.fields();
            match view {
                KindView::Table { columns, sort, .. } => {
                    let unique: std::collections::BTreeSet<_> = columns.iter().collect();
                    assert_eq!(unique.len(), columns.len(), "{} repeats", entry.kind);
                    assert!(
                        sort.as_ref()
                            .is_none_or(|sort| columns.contains(&sort.field)),
                        "{} sorts by a hidden column",
                        entry.kind
                    );
                }
                KindView::Meter { min, max, .. } => {
                    assert!(min < max, "{} meter range", entry.kind);
                }
                KindView::Trend { window_s, .. } => {
                    assert!(*window_s > 0, "{} trend window", entry.kind);
                }
                KindView::Picture => {
                    assert!(
                        entry.has(Capability::Picture),
                        "{} has no picture",
                        entry.kind
                    );
                }
            }
            assert!(
                matches!(view, KindView::Picture) || !fields.is_empty(),
                "{} view names no field",
                entry.kind
            );
        }
    }
}

#[test]
fn calls_follow_the_mode_class() {
    for d in descriptors() {
        let expected = match d.type_id.as_str() {
            "dmr" | "dstar" | "ysf" | "nxdn" | "p25" | "p25p2" | "dpmr" | "m17" | "freedv"
            | "tetra" | "edacs" | "ltr" | "mpt1327" => Some(CallGate::Frames),
            "nfm" | "am" | "ssb" | "dect" => Some(CallGate::Squelch),
            _ => None,
        };
        assert_eq!(d.calls, expected, "{}", d.type_id);
        assert_eq!(
            d.calls == Some(CallGate::Frames),
            d.emits.contains(&EventKind::DV),
            "{} gates calls by frames exactly when it emits digital voice",
            d.type_id
        );
        assert!(
            !d.emits.contains(&EventKind::CALL),
            "{} declares calls, which only the call tracker makes",
            d.type_id
        );
        if d.calls == Some(CallGate::Squelch) {
            assert!(
                d.has_audio,
                "{} gates calls by squelch without audio",
                d.type_id
            );
        }
    }
}

#[test]
fn audio_channels_matches_the_frames_each_mode_produces() {
    const LEN: usize = 96_000;
    for d in descriptors().into_iter().filter(|d| d.has_audio) {
        let params = default_params(&d.type_id);
        let channels = usize::from(audio_channels(&params));
        let ctx = ChannelCtx {
            input_rate: d.input_rate_hz,
        };
        let mut chan = create(ctx, &settings(params)).expect("builds");
        let audio = sdrmm_decoder_test_support::run_ragged(
            chan.as_mut(),
            &sdrmm_decoder_test_support::complex_noise(7, 0.5, LEN),
        );
        assert_eq!(
            audio.len() % channels,
            0,
            "{} emitted a partial sample frame",
            d.type_id
        );
        if d.calls == Some(CallGate::Frames) || d.emits.contains(&EventKind::named("broadcast")) {
            continue;
        }
        if d.type_id == "atv" && audio.is_empty() {
            continue;
        }
        let frames = audio.len() / channels;
        let expected = (LEN as f64 * f64::from(AUDIO_RATE) / d.input_rate_hz) as usize;
        assert!(
            frames <= expected && expected - frames < 200,
            "{} produced {frames} frames of {channels}-channel audio, expected ~{expected}",
            d.type_id
        );
    }
}

fn home_settings(type_id: &str) -> ChannelSettings {
    ChannelSettings {
        frequency_hz: default_settings(type_id).map_or(0.0, |defaults| defaults.frequency_hz),
        ..settings(default_params(type_id))
    }
}

#[test]
fn create_builds_every_registered_type() {
    for d in descriptors() {
        let ctx = ChannelCtx {
            input_rate: d.input_rate_hz,
        };
        let built = create(ctx, &home_settings(&d.type_id));
        assert!(built.is_ok(), "{}: {:?}", d.type_id, built.err());
    }
}

#[test]
fn create_rejects_mismatched_input_rate() {
    let ctx = ChannelCtx {
        input_rate: 96_000.0,
    };
    let err = create(
        ctx,
        &settings(ChannelParams::new("nfm", &NfmParams::default()).expect("nfm settings")),
    );
    assert!(matches!(err, Err(ChannelError::InvalidSettings(_))));
}

#[test]
fn occupied_band_tracks_params_and_sideband() {
    assert_eq!(
        occupied_band(
            &ChannelParams::new(
                "nfm",
                &NfmParams {
                    bandwidth_hz: 25_000.0,
                    ..NfmParams::default()
                }
            )
            .expect("nfm settings")
        ),
        (-12_500.0, 12_500.0)
    );
    assert_eq!(
        occupied_band(
            &ChannelParams::new(
                "am",
                &AmParams {
                    bandwidth_hz: 8_000.0,
                    ..AmParams::default()
                }
            )
            .expect("am settings")
        ),
        (-4_000.0, 4_000.0)
    );
    assert_eq!(
        occupied_band(
            &ChannelParams::new(
                "ssb",
                &SsbParams {
                    sideband: Sideband::Usb,
                    bandwidth_hz: 10_000.0,
                }
            )
            .expect("ssb settings")
        ),
        (100.0, 10_000.0)
    );
    assert_eq!(
        occupied_band(
            &ChannelParams::new(
                "ssb",
                &SsbParams {
                    sideband: Sideband::Lsb,
                    bandwidth_hz: 10_000.0,
                }
            )
            .expect("ssb settings")
        ),
        (-10_000.0, -100.0)
    );
    assert_eq!(
        occupied_band(&ChannelParams::new("wfm", &WfmParams::default()).expect("wfm settings")),
        (-100_000.0, 100_000.0)
    );
    assert_eq!(
        occupied_band(
            &ChannelParams::new(
                "atv",
                &AtvParams {
                    color: AtvColor::Pal,
                    sound_subcarrier_hz: Some(5_500_000.0),
                    ..AtvParams::default()
                }
            )
            .expect("atv settings")
        ),
        (-750_000.0, 5_565_000.0)
    );
}

fn filter_rms(filter: &mut ChannelFilter, freq_norm: f64) -> f32 {
    let mut out = Vec::new();
    filter.process(
        &sdrmm_decoder_test_support::complex_tone(freq_norm, 8_192),
        &mut out,
    );
    let settled = &out[512..];
    (settled.iter().map(|v| f64::from(v.norm_sqr())).sum::<f64>() / settled.len() as f64).sqrt()
        as f32
}

#[test]
fn channel_filter_passes_in_channel_and_rejects_adjacent() {
    let mut f =
        channel_filter(&ChannelParams::new("nfm", &NfmParams::default()).expect("nfm settings"))
            .unwrap();
    let pass = filter_rms(&mut f, 1_000.0 / 48_000.0);
    assert!((0.9..1.05).contains(&pass), "in-channel rms {pass}");
    let mut f =
        channel_filter(&ChannelParams::new("nfm", &NfmParams::default()).expect("nfm settings"))
            .unwrap();
    let reject = filter_rms(&mut f, 15_000.0 / 48_000.0);
    assert!(reject < 0.01, "adjacent leak rms {reject}");

    let ssb = ChannelParams::new("ssb", &SsbParams::default()).expect("ssb settings");
    let mut f = channel_filter(&ssb).unwrap();
    let pass = filter_rms(&mut f, 1_000.0 / 48_000.0);
    assert!((0.9..1.05).contains(&pass), "usb rms {pass}");
    let mut f = channel_filter(&ssb).unwrap();
    let reject = filter_rms(&mut f, -1_000.0 / 48_000.0);
    assert!(reject < 0.01, "lsb leak rms {reject}");
}

#[test]
fn channel_filter_rejects_out_of_range_bandwidth() {
    for params in [
        ChannelParams::new(
            "nfm",
            &NfmParams {
                bandwidth_hz: f64::NAN,
                ..NfmParams::default()
            },
        )
        .expect("nfm settings"),
        ChannelParams::new(
            "am",
            &AmParams {
                bandwidth_hz: 0.0,
                ..AmParams::default()
            },
        )
        .expect("am settings"),
        ChannelParams::new(
            "ssb",
            &SsbParams {
                sideband: Sideband::Usb,
                bandwidth_hz: 50.0,
            },
        )
        .expect("ssb settings"),
    ] {
        assert!(
            matches!(
                channel_filter(&params),
                Err(ChannelError::InvalidSettings(_))
            ),
            "{} must be rejected",
            params.type_id()
        );
    }
}

fn tone(hz: f64, rate: f64, len: usize) -> Vec<Complex<f32>> {
    (0..len)
        .map(|n| Complex::from_polar(1.0, (TAU * (hz * n as f64 / rate).fract()) as f32))
        .collect()
}

fn level_db(front: &mut ChannelFront, iq: &[Complex<f32>]) -> f64 {
    let mut out = Vec::new();
    let mut all = Vec::new();
    for block in iq.chunks(4_096) {
        front.process(block, &mut out);
        all.extend_from_slice(&out);
    }
    let settled = &all[all.len() / 2..];
    let power = settled.iter().map(|s| f64::from(s.norm_sqr())).sum::<f64>();
    10.0 * (power / settled.len() as f64).log10()
}

#[test]
fn a_decimating_filter_takes_the_last_halving_and_still_stops_what_folds_back() {
    let params = ChannelParams::of(&WfmParams::default()).expect("wfm settings");
    let rate = 1_920_000.0;
    let mut front = ChannelFront::new(&params, rate, 0.0).expect("front");
    assert_eq!(front.filter.decimation(), 2);
    assert_eq!(front.wide_rate, 480_000.0);
    let len = 960_000;
    let mut out = Vec::new();
    front.process(&tone(0.0, rate, len), &mut out);
    assert!(
        (out.len() as f64 - len as f64 / 8.0).abs() <= 2.0,
        "{} out",
        out.len()
    );
    for (hz, passes) in [
        (20_000.0, true),
        (-85_000.0, true),
        (130_000.0, false),
        (-200_000.0, false),
        (250_000.0, false),
        (455_000.0, false),
    ] {
        let mut front = ChannelFront::new(&params, rate, 0.0).expect("front");
        let db = level_db(&mut front, &tone(hz, rate, len));
        if passes {
            assert!(db.abs() < 0.05, "{hz} Hz passes at {db} dB");
        } else {
            assert!(db < -99.0, "{hz} Hz leaks at {db} dB");
        }
    }
}

#[test]
fn a_filter_at_the_decoders_rate_leaves_the_converter_whole() {
    let params = ChannelParams::of(&NfmParams::default()).expect("nfm settings");
    let front = ChannelFront::new(&params, 1_920_000.0, 0.0).expect("front");
    assert_eq!(front.filter.decimation(), 1);
    assert_eq!(front.wide_rate, input_rate(&params));
}

#[test]
fn only_the_modes_with_an_encoder_transmit() {
    let types: Vec<String> = sdrmm_decoder::encoder::encoders()
        .into_iter()
        .map(|d| d.type_id)
        .collect();
    assert_eq!(types, ["nfm", "am", "ssb", "aprs"]);
}

#[test]
fn every_built_in_opens_on_its_own_defaults() {
    for encoder in encoders() {
        let descriptor = encoder.descriptor();
        let params = EncoderParams {
            type_id: descriptor.type_id.clone(),
            settings: descriptor.defaults.clone(),
        };
        assert!(encoder.open(&params).is_ok(), "{}", descriptor.type_id);
        assert!(descriptor.rate_hz > 0.0 && descriptor.bandwidth_hz > 0.0);
        assert_eq!(
            descriptor.input == EncoderInput::Message,
            !descriptor.message.is_empty(),
            "{}",
            descriptor.type_id
        );
    }
}

#[test]
fn an_encoder_refuses_another_encoders_settings_and_an_audio_one_composes_nothing() {
    let nfm = encoder("nfm").expect("nfm encodes");
    assert!(matches!(
        nfm.open(&EncoderParams::bare("am")),
        Err(EncoderError::WrongType { got, .. }) if got == "am"
    ));
    assert!(matches!(
        nfm.compose(&serde_json::json!({})),
        Err(EncoderError::NoMessages(id)) if id == "nfm"
    ));
    assert_eq!(encoder("aprs").expect("aprs").descriptor().message.len(), 4);
    assert!(matches!(
        encoder("wfm"),
        Err(EncoderError::Unknown(id)) if id == "wfm"
    ));
}

fn offered_widths(field: &sdrmm_wire::SettingField) -> Vec<serde_json::Value> {
    match &field.control {
        sdrmm_wire::Control::Choice { options } if field.key == "bandwidth_hz" => {
            options.iter().map(|option| option.value.clone()).collect()
        }
        _ => Vec::new(),
    }
}

fn with_width(field: &sdrmm_wire::SettingField, width: serde_json::Value) -> serde_json::Value {
    let mut settings = serde_json::Map::new();
    for condition in &field.when {
        if let Some(value) = condition.any_of.first() {
            settings.insert(condition.key.clone(), value.clone());
        }
    }
    settings.insert(field.key.clone(), width);
    serde_json::Value::Object(settings)
}

#[test]
fn every_offered_bandwidth_builds_a_channel_filter() {
    for decoder in decoders() {
        let descriptor = decoder.descriptor();
        for field in &descriptor.settings {
            for width in offered_widths(field) {
                let params = ChannelParams {
                    type_id: descriptor.type_id.clone(),
                    settings: with_width(field, width.clone()),
                };
                assert!(
                    decoder.filter(&params).is_ok() && decoder.normalize(&params).is_ok(),
                    "{} offers {width} Hz but refuses it",
                    descriptor.type_id
                );
            }
        }
    }
}

#[test]
fn every_mode_lands_on_the_detector_that_made_its_audio() {
    use crate::{DmrParams, NfmParams, SsbParams, WfmParams};
    use sdrmm_decoder::{ClickProfile, click_profile};
    for (params, profile) in [
        (
            ChannelParams::new("nfm", &NfmParams::default()).expect("nfm settings"),
            ClickProfile::Discriminator,
        ),
        (
            ChannelParams::new("wfm", &WfmParams::default()).expect("wfm settings"),
            ClickProfile::Discriminator,
        ),
        (
            ChannelParams::new("ssb", &SsbParams::default()).expect("ssb settings"),
            ClickProfile::Detector,
        ),
        (
            ChannelParams::new("dmr", &DmrParams::default()).expect("dmr settings"),
            ClickProfile::Vocoder,
        ),
    ] {
        assert_eq!(click_profile(&params), profile, "{params:?}");
    }
}

#[test]
fn a_damaged_transmission_is_reported_as_rejected() {
    use crate::synth as tx;
    use sdrmm_decoder::{Channel, ChannelOutputs};

    const RATE: f64 = 48_000.0;
    fn channel(type_id: &str) -> Box<dyn Channel> {
        let settings = default_settings(type_id).expect("channel settings");
        create(ChannelCtx { input_rate: RATE }, &settings).expect("channel")
    }
    fn rejects(chan: &mut dyn Channel, iq: &[Complex<f32>]) -> u32 {
        let mut out = ChannelOutputs::default();
        for block in iq.chunks(4_096) {
            out.reset();
            chan.process(block, &mut out);
        }
        chan.rejected()
    }
    fn hissing(clean: &[Complex<f32>], seed: u32) -> Vec<Complex<f32>> {
        let hiss = sdrmm_decoder_test_support::complex_noise(seed, 0.9, clean.len());
        clean
            .iter()
            .zip(hiss)
            .map(|(&sample, noise)| sample + noise)
            .collect()
    }
    fn time_slipped(clean: &[Complex<f32>]) -> Vec<Complex<f32>> {
        const SLIP: usize = 60;
        let cut = clean.len() / 4;
        let mut out = clean[..cut].to_vec();
        out.extend_from_slice(&clean[cut + SLIP..]);
        out
    }

    let modes = [
        (
            "dmr",
            tx::dmr::transmission(&tx::dmr::Call::default(), RATE),
        ),
        ("p25", tx::p25::trunking(0x2A, RATE)),
        (
            "ysf",
            tx::ysf::transmission(&tx::ysf::Fich::default(), RATE),
        ),
        (
            "nxdn",
            tx::nxdn::transmission(&tx::nxdn::Shape::default(), 1, true, RATE),
        ),
        ("m17", tx::m17::transmission("ALL", "DL1ABC", RATE)),
    ];
    let silent: Vec<&str> = modes
        .iter()
        .enumerate()
        .filter(|(index, (type_id, clean))| {
            let seed = 0x11 * (*index as u32 + 1);
            rejects(channel(type_id).as_mut(), &hissing(clean, seed)) == 0
                && rejects(channel(type_id).as_mut(), &time_slipped(clean)) == 0
        })
        .map(|(_, (type_id, _))| *type_id)
        .collect();

    assert!(
        silent.is_empty(),
        "these modes threw frames away without saying so: {silent:?}"
    );
}

#[test]
fn every_positive_fixture_states_what_it_must_read() {
    for (type_id, positive) in super::positives() {
        assert!(
            positive.proves_a_decode(),
            "{type_id} has no expected payload, so its fixture proves nothing"
        );
    }
}
