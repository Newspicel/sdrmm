use std::{collections::BTreeMap, f64::consts::TAU};

use num_complex::Complex;
use sdrmm_wire::{
    ChannelParams, ChannelSettings, DecoderEvent, DectBand, DectCapability, DectFrame, DectParams,
    DectSide, DectSpan,
};

use crate::{
    AUDIO_RATE, ChannelCtx, ChannelOutputs, ChannelRx,
    dect::{DectChannel, input_rate},
    synth::dect::{self as sig, Air, Call, Station},
    testutil::settings,
};

const CLASS_A_RFPI: u64 = 0x0001_234D_5E6D;
const PMID: u32 = 0x1_2345;
const BASE_TONE_HZ: f64 = 1_000.0;
const HANDSET_TONE_HZ: f64 = 1_700.0;

fn build(params: DectParams, frequency_hz: f64) -> DectChannel {
    DectChannel::new(
        ChannelCtx {
            input_rate: input_rate(&params),
        },
        ChannelSettings {
            frequency_hz,
            ..settings(ChannelParams::Dect(params))
        },
    )
    .expect("dect channel")
}

fn run(chan: &mut DectChannel, iq: &[Complex<f32>]) -> (Vec<DectFrame>, Vec<f32>) {
    let mut out = ChannelOutputs::default();
    let mut frames = Vec::new();
    let mut audio = Vec::new();
    for chunk in iq.chunks(7_919) {
        out.reset();
        chan.process(chunk, &mut out);
        audio.extend_from_slice(&out.audio_pcm);
        frames.extend(out.events.drain(..).filter_map(|event| match event {
            DecoderEvent::Dect(frame) => Some(frame),
            _ => None,
        }));
    }
    (frames, audio)
}

fn level(audio: &[f32], freq_hz: f64) -> f64 {
    let step = TAU * freq_hz / f64::from(AUDIO_RATE);
    let sum: Complex<f64> = audio
        .iter()
        .enumerate()
        .map(|(n, &sample)| Complex::from_polar(f64::from(sample), -step * n as f64))
        .sum();
    2.0 * sum.norm() / audio.len() as f64
}

fn tail(audio: &[f32], seconds: f64) -> &[f32] {
    let keep = (seconds * f64::from(AUDIO_RATE)) as usize;
    &audio[audio.len().saturating_sub(keep)..]
}

fn last(frames: &[DectFrame], side: DectSide) -> &DectFrame {
    frames
        .iter()
        .rev()
        .find(|frame| frame.side == side && frame.voice.is_some())
        .expect("a frame with voice")
}

fn station(carrier: u8) -> Station {
    Station {
        rfpi: CLASS_A_RFPI,
        carrier,
        slot: 2,
        ..Station::default()
    }
}

fn call<'a>(station: Station, base: &'a [i16], handset: &'a [i16]) -> Call<'a> {
    Call {
        station,
        pmid: PMID,
        first_frame: 3,
        base,
        handset,
        grant_at: None,
    }
}

#[test]
fn plays_both_sides_of_a_clear_call() {
    const FRAMES: usize = 60;
    let base = sig::tone(BASE_TONE_HZ, 0.3, FRAMES * 80);
    let handset = sig::tone(HANDSET_TONE_HZ, 0.3, FRAMES * 80);
    let mut air = Air::carrier(FRAMES);
    call(station(4), &base, &handset).transmit(&mut air, FRAMES);
    let carrier_hz = DectBand::Eu.carrier_hz(4).unwrap_or_default();
    let mut chan = build(DectParams::default(), carrier_hz);
    let (frames, audio) = run(&mut chan, &air.into_iq());

    let rfp = last(&frames, DectSide::Rfp);
    let voice = rfp.voice.expect("base voice");
    assert!(voice.playing);
    assert!(voice.frames >= 50, "{voice:?}");
    assert_eq!(voice.unsynced, 5, "frames 3 to 7 precede the first Qt");
    assert_eq!(voice.x_crc_errors, 0);
    assert_eq!(voice.late, 0);
    assert_eq!(rfp.carrier, Some(4));
    let pp = last(&frames, DectSide::Pp).voice.expect("handset voice");
    assert!(pp.frames >= 50 && pp.playing, "{pp:?}");

    let speech = tail(&audio, 0.25);
    assert!(
        level(speech, BASE_TONE_HZ) > 0.15,
        "base tone {}",
        level(speech, BASE_TONE_HZ)
    );
    assert!(level(speech, HANDSET_TONE_HZ) > 0.15);
    assert!(level(speech, 600.0) < 0.02);
}

#[test]
fn an_encrypted_call_is_counted_and_kept_off_the_speaker() {
    const FRAMES: usize = 60;
    let base = sig::tone(BASE_TONE_HZ, 0.3, FRAMES * 80);
    let handset = sig::tone(HANDSET_TONE_HZ, 0.3, FRAMES * 80);
    let mut air = Air::carrier(FRAMES);
    Call {
        grant_at: Some(20),
        ..call(station(4), &base, &handset)
    }
    .transmit(&mut air, FRAMES);
    let mut chan = build(DectParams::default(), 0.0);
    let (frames, audio) = run(&mut chan, &air.into_iq());
    let voice = last(&frames, DectSide::Rfp).voice.expect("voice stats");
    assert!(voice.encrypted >= 20 && !voice.playing, "{voice:?}");
    let handset_voice = last(&frames, DectSide::Pp).voice.expect("handset stats");
    assert!(handset_voice.encrypted >= 20, "{handset_voice:?}");
    let after = tail(&audio, 0.2);
    assert!(after.iter().all(|sample| sample.abs() < 1e-3));
}

#[test]
fn reads_every_carrier_of_the_eu_band_at_once() {
    const FRAMES: usize = 6;
    let band = DectBand::Eu;
    let mut air = Air::band(band, FRAMES);
    for carrier in 0..band.carriers() {
        for frame in 0..FRAMES {
            let rfpi = CLASS_A_RFPI + u64::from(carrier);
            air.signalling(carrier, frame, 2, true, sig::nt(rfpi));
        }
    }
    let params = DectParams {
        span: DectSpan::Band,
        ..DectParams::default()
    };
    let mut chan = build(params, band.center_hz());
    let (frames, _) = run(&mut chan, &air.into_iq());
    let seen: BTreeMap<String, Option<u8>> = frames
        .iter()
        .filter_map(|frame| Some((frame.identity.as_ref()?.rfpi.clone(), frame.carrier)))
        .collect();
    assert_eq!(seen.len(), 10, "{seen:?}");
    for carrier in 0..10u8 {
        let rfpi = format!("{:010X}", CLASS_A_RFPI + u64::from(carrier));
        assert_eq!(seen.get(&rfpi), Some(&Some(carrier)), "{rfpi}");
    }
    assert!(frames.iter().all(|frame| frame.crc_errors == 0));
}

#[test]
fn reads_the_five_us_carriers_from_one_capture() {
    const FRAMES: usize = 4;
    let band = DectBand::Us;
    let mut air = Air::band(band, FRAMES);
    for carrier in 0..band.carriers() {
        for frame in 0..FRAMES {
            air.signalling(
                carrier,
                frame,
                7,
                true,
                sig::nt(CLASS_A_RFPI + u64::from(carrier)),
            );
        }
    }
    let params = DectParams {
        band,
        span: DectSpan::Band,
        ..DectParams::default()
    };
    let mut chan = build(params, band.center_hz());
    let (frames, _) = run(&mut chan, &air.into_iq());
    let carriers: std::collections::BTreeSet<Option<u8>> =
        frames.iter().map(|frame| frame.carrier).collect();
    assert_eq!(carriers.len(), 5, "{carriers:?}");
    assert!(frames.iter().all(|frame| frame.carrier_hz.is_some()));
}

#[test]
fn follows_a_call_on_the_outermost_carrier_of_a_wideband_capture() {
    const FRAMES: usize = 50;
    let band = DectBand::Eu;
    let base = sig::tone(BASE_TONE_HZ, 0.3, FRAMES * 80);
    let silence = vec![0i16; FRAMES * 80];
    let mut air = Air::band(band, FRAMES);
    call(station(9), &base, &silence).transmit(&mut air, FRAMES);
    air.dummy_bearer(
        &Station {
            rfpi: 0x0055_5555_5555,
            carrier: 0,
            slot: 2,
            ..Station::default()
        },
        FRAMES,
    );
    let params = DectParams {
        span: DectSpan::Band,
        ..DectParams::default()
    };
    let mut chan = build(params, band.center_hz());
    let (frames, audio) = run(&mut chan, &air.into_iq());
    let rfp = last(&frames, DectSide::Rfp);
    assert_eq!(rfp.carrier, Some(9));
    let voice = rfp.voice.expect("voice");
    assert!(voice.frames >= 20 && voice.playing, "{voice:?}");
    assert_eq!(voice.x_crc_errors, 0);
    assert!(level(tail(&audio, 0.2), BASE_TONE_HZ) > 0.15);
    assert!(
        frames.iter().any(|frame| frame
            .identity
            .as_ref()
            .is_some_and(|id| id.rfpi == "5555555555")),
        "the dummy bearer on carrier 0 shares the capture"
    );
}

#[test]
fn reports_dsaa2_and_dsc2_from_extended_capabilities_part_two() {
    const FRAMES: usize = 24;
    let station = station(4);
    let messages = [
        sig::qt_capabilities(sig::capability_bits(&[12, 17, 36, 37])),
        sig::qt_message(0x4, sig::capability_bits(&[21, 23, 40])),
        sig::qt_message(0xC, sig::capability_bits(&[22, 24, 42, 43, 44])),
        sig::qt_message(0xE, sig::capability_bits(&[14])),
    ];
    let mut air = Air::carrier(FRAMES);
    air.qt_cycle(&station, &messages, FRAMES);
    let mut chan = build(DectParams::default(), 0.0);
    let (frames, _) = run(&mut chan, &air.into_iq());
    let latest = frames.last().expect("frames");
    assert_eq!(latest.security.dsaa2_supported, Some(true));
    assert_eq!(latest.security.dsc2_supported, Some(true));
    assert_eq!(latest.security.ciphering_supported, Some(true));
    for expected in [
        DectCapability::ExtendedFpInfo,
        DectCapability::MacSuspendResume,
        DectCapability::ExtendedFpInfo2,
        DectCapability::EmergencyCall,
        DectCapability::ExtendedFpInfo3,
        DectCapability::WidebandVoice,
        DectCapability::ReKeying,
        DectCapability::Dsaa2,
        DectCapability::Dsc2,
        DectCapability::Modulation16qam,
    ] {
        assert!(
            latest.has(expected),
            "{expected:?} missing: {:?}",
            latest.capabilities
        );
    }
}

#[test]
fn a_base_without_part_two_reports_no_dsaa2() {
    const FRAMES: usize = 12;
    let messages = [
        sig::qt_capabilities(sig::capability_bits(&[12, 36, 37])),
        sig::qt_message(0x4, sig::capability_bits(&[21])),
    ];
    let mut air = Air::carrier(FRAMES);
    air.qt_cycle(&station(4), &messages, FRAMES);
    let mut chan = build(DectParams::default(), 0.0);
    let (frames, _) = run(&mut chan, &air.into_iq());
    let latest = frames.last().expect("frames");
    assert_eq!(latest.security.dsaa2_supported, Some(false));
    assert_eq!(latest.security.dsc2_supported, Some(false));
}

fn load_wav(path: &std::path::Path) -> (Vec<Complex<f32>>, f64) {
    let bytes = std::fs::read(path).expect("DECT recording");
    let rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
    let data = bytes
        .windows(4)
        .position(|window| window == b"data")
        .expect("data chunk")
        + 8;
    let iq = bytes[data..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|s| {
            Complex::new(
                f32::from(i16::from_le_bytes([s[0], s[1]])) / 32_768.0,
                f32::from(i16::from_le_bytes([s[2], s[3]])) / 32_768.0,
            )
        })
        .collect();
    (iq, f64::from(rate))
}

#[test]
#[ignore = "needs DECT_SIGID_WAV=<SigIDWiki Dect.zip dect.wav>"]
fn decodes_two_real_base_stations_off_air() {
    let path =
        std::path::PathBuf::from(std::env::var_os("DECT_SIGID_WAV").expect("DECT_SIGID_WAV"));
    let (iq, rate) = load_wav(&path);
    let mut ddc = sdrmm_dsp::Ddc::new(rate, crate::dect::burst::INPUT_RATE_HZ, 0.0).expect("ddc");
    let mut resampled = Vec::new();
    ddc.process(&iq, &mut resampled);
    let mut chan = build(DectParams::default(), 0.0);
    let (frames, _) = run(&mut chan, &resampled);
    let stations: BTreeMap<String, &DectFrame> = frames
        .iter()
        .filter_map(|frame| Some((frame.identity.as_ref()?.rfpi.clone(), frame)))
        .collect();
    assert!(stations.contains_key("01594D6FA8"), "{:?}", stations.keys());
    assert!(stations.contains_key("019B990340"), "{:?}", stations.keys());
    let strong = frames
        .iter()
        .rev()
        .find(|frame| {
            frame
                .identity
                .as_ref()
                .is_some_and(|id| id.rfpi == "01594D6FA8")
                && frame.security.dsaa2_supported.is_some()
        })
        .expect("extended capabilities from the strong base");
    assert!(strong.has(DectCapability::ExtendedFpInfo2));
    assert!(strong.has(DectCapability::NoEmission));
    assert!(strong.has(DectCapability::StandardCiphering));
    assert_eq!(strong.security.dsaa2_supported, Some(false));
    assert_eq!(strong.security.dsc2_supported, Some(false));
    assert_eq!(strong.carrier, Some(7));
    assert!(strong.crc_errors * 20 < strong.bursts, "{strong:?}");
}

fn relocate(iq: &[Complex<f32>], rate: f64, band: DectBand, carrier: u8) -> Vec<Complex<f32>> {
    let wide = crate::dect::wideband::input_rate(band);
    let taps = sdrmm_dsp::fir::design_lowpass_kaiser(255, 650_000.0 / rate, 80.0);
    let mut limited = Vec::new();
    sdrmm_dsp::Decimator::new(&taps, 1).process(iq, &mut limited);
    let mut ddc = sdrmm_dsp::Ddc::new(rate, wide, 0.0).expect("ddc");
    let mut out = Vec::new();
    ddc.process(&limited, &mut out);
    let offset = band.carrier_hz(carrier).unwrap_or_default() - band.center_hz();
    for (index, sample) in out.iter_mut().enumerate() {
        let turns = (offset / wide * index as f64).fract();
        let (sin, cos) = (TAU * turns).sin_cos();
        *sample *= Complex::new(cos as f32, sin as f32);
    }
    out
}

#[test]
#[ignore = "needs DECT_SIGID_WAV=<SigIDWiki Dect.zip dect.wav>"]
fn separates_real_bases_moved_onto_two_carriers_of_a_wideband_capture() {
    let path =
        std::path::PathBuf::from(std::env::var_os("DECT_SIGID_WAV").expect("DECT_SIGID_WAV"));
    let (iq, rate) = load_wav(&path);
    let band = DectBand::Eu;
    let low = relocate(&iq, rate, band, 1);
    let high = relocate(&iq, rate, band, 8);
    let delay = (crate::dect::wideband::input_rate(band) * 0.0031) as usize;
    let mut wide = low;
    for (slot, sample) in wide.iter_mut().skip(delay).zip(&high) {
        *slot += *sample;
    }
    let params = DectParams {
        span: DectSpan::Band,
        ..DectParams::default()
    };
    let mut chan = build(params, band.center_hz());
    let (frames, _) = run(&mut chan, &wide);
    let mut seen = BTreeMap::new();
    for frame in &frames {
        if let Some(id) = &frame.identity {
            seen.insert((id.rfpi.clone(), frame.carrier), frame.crc_errors);
        }
    }
    for carrier in [1, 8] {
        assert!(
            seen.contains_key(&("01594D6FA8".to_owned(), Some(carrier))),
            "{seen:?}"
        );
        assert!(
            seen.contains_key(&("019B990340".to_owned(), Some(carrier))),
            "{seen:?}"
        );
    }
    assert!(
        seen.keys()
            .all(|(_, carrier)| matches!(carrier, Some(1 | 8))),
        "a base leaked onto another carrier: {seen:?}"
    );
}
