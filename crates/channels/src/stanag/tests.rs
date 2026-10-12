mod air;
mod synth;

use num_complex::Complex;
use sdrmm_wire::{
    ChannelParams, DecoderEvent, StanagInterleave, StanagModem, StanagParams, StanagRate,
    StanagReport,
};

use super::{INPUT_RATE_HZ, StanagChannel, channel_filter, preamble::RATES};
use crate::{ChannelCtx, ChannelOutputs, ChannelRx, testutil::settings};

const SECONDS: f64 = 6.0;
const OFFSET_HZ: f64 = 23.4;
const SNR_DB: f64 = 10.0;

fn channel() -> StanagChannel {
    StanagChannel::new(
        ChannelCtx {
            input_rate: INPUT_RATE_HZ,
        },
        settings(ChannelParams::Stanag(StanagParams::default())),
    )
    .unwrap()
}

fn run(channel: &mut StanagChannel, iq: &[Complex<f32>]) -> Vec<StanagReport> {
    let mut filter = channel_filter();
    let mut filtered = Vec::new();
    let mut out = ChannelOutputs::default();
    let mut reports = Vec::new();
    for block in iq.chunks(1_031) {
        filter.process(block, &mut filtered);
        out.reset();
        channel.process(&filtered, &mut out);
        reports.extend(out.events.drain(..).map(|event| match event {
            DecoderEvent::Stanag(report) => report,
            other => panic!("unexpected {other:?}"),
        }));
    }
    reports
}

fn identify(iq: &[Complex<f32>]) -> (Vec<StanagReport>, u32) {
    let mut channel = channel();
    let reports = run(&mut channel, iq);
    (reports, channel.rejected())
}

fn only_modem(iq: &[Complex<f32>]) -> StanagReport {
    let (reports, rejected) = identify(iq);
    assert!(
        reports
            .iter()
            .all(|report| report.modem == reports[0].modem),
        "{reports:?}"
    );
    assert_eq!(rejected, 0);
    reports
        .last()
        .cloned()
        .unwrap_or_else(|| panic!("nothing named"))
}

#[test]
fn names_each_modem_in_noise() {
    let carrier = 1_800.0 + OFFSET_HZ;
    let cases = [
        (
            StanagModem::Stanag4285,
            synth::stanag_4285(SECONDS, carrier, 1),
        ),
        (
            StanagModem::Stanag4529,
            synth::stanag_4529(SECONDS, carrier, 2),
        ),
        (
            StanagModem::MilStd110a,
            synth::mil_110a(SECONDS, carrier, None, 3),
        ),
        (
            StanagModem::Stanag4539,
            synth::stanag_4539(SECONDS, carrier, 4),
        ),
        (
            StanagModem::ThirtyNineTone,
            synth::thirty_nine_tone(SECONDS, OFFSET_HZ, 5),
        ),
        (
            StanagModem::SixteenTone,
            synth::sixteen_tone(SECONDS, OFFSET_HZ, 6),
        ),
        (StanagModem::Ale2g, synth::ale(SECONDS, OFFSET_HZ, 7)),
    ];
    for (modem, clean) in cases {
        let report = only_modem(&synth::awgn(&clean, SNR_DB, 11));
        assert_eq!(report.modem, Some(modem));
        assert!(report.confidence > 0.3, "{modem:?} {report:?}");
    }
}

#[test]
fn framed_modems_report_their_carrier_to_a_hertz() {
    let carrier = 1_800.0 + OFFSET_HZ;
    for clean in [
        synth::stanag_4285(SECONDS, carrier, 1),
        synth::stanag_4529(SECONDS, carrier, 2),
    ] {
        let report = only_modem(&synth::awgn(&clean, SNR_DB, 12));
        assert!(
            (f64::from(report.audio_hz) - carrier).abs() < 1.0,
            "{report:?}"
        );
    }
}

#[test]
fn other_modems_report_their_carrier_from_the_spectrum() {
    let carrier = 1_800.0 + OFFSET_HZ;
    for clean in [
        synth::stanag_4539(SECONDS, carrier, 4),
        synth::mil_110a(SECONDS, carrier, None, 3),
    ] {
        let report = only_modem(&synth::awgn(&clean, SNR_DB, 12));
        assert!(
            (f64::from(report.audio_hz) - carrier).abs() < 25.0,
            "{report:?}"
        );
    }
}

#[test]
fn ale_reports_the_middle_of_its_tones() {
    let report = only_modem(&synth::ale(SECONDS, OFFSET_HZ, 7));
    assert!((f64::from(report.audio_hz) - 1_625.0 - OFFSET_HZ).abs() < 13.0);
}

#[test]
fn reads_every_110a_rate_from_its_preamble() {
    for &(d1, d2, rate) in &RATES {
        let clean = synth::mil_110a(SECONDS, 1_800.0 + OFFSET_HZ, Some((d1, d2)), 20);
        let report = only_modem(&synth::awgn(&clean, SNR_DB, 13));
        assert_eq!(report.rate, Some(rate), "D1 {d1} D2 {d2}");
    }
}

#[test]
fn a_rate_learned_later_is_reported_again() {
    let carrier = 1_800.0 + OFFSET_HZ;
    let mut iq = synth::mil_110a(4.0, carrier, None, 3);
    iq.extend(synth::mil_110a(SECONDS, carrier, Some((4, 5)), 3));
    let (reports, _) = identify(&synth::awgn(&iq, SNR_DB, 14));
    let rates: Vec<_> = reports.iter().map(|report| report.rate).collect();
    assert_eq!(rates.first().copied().flatten(), None, "{rates:?}");
    assert_eq!(
        rates.last().copied().flatten(),
        Some(StanagRate {
            bps: 1_200,
            interleave: StanagInterleave::Long,
            voice: false,
        })
    );
}

#[test]
fn noise_alone_names_nothing_and_rejects_nothing() {
    let silence = vec![Complex::default(); (SECONDS * synth::RATE) as usize];
    let (reports, rejected) = identify(
        &synth::awgn(&silence, 0.0, 15)
            .iter()
            .map(|s| s * 0.1)
            .collect::<Vec<_>>(),
    );
    assert!(reports.is_empty(), "{reports:?}");
    assert_eq!(rejected, 0);
}

#[test]
fn an_unknown_signal_is_counted_not_named() {
    let len = (SECONDS * synth::RATE) as usize;
    let mut noise = synth::Noise::new(16);
    let mut phase = 0.0f64;
    let mut tone = 0.0;
    let rtty: Vec<Complex<f32>> = (0..len)
        .map(|n| {
            if n % 211 == 0 {
                tone = if noise.next_u32().is_multiple_of(2) {
                    1_275.0
                } else {
                    2_125.0
                };
            }
            phase = (phase + std::f64::consts::TAU * tone / synth::RATE)
                .rem_euclid(std::f64::consts::TAU);
            Complex::from_polar(1.0, phase as f32)
        })
        .collect();
    let (reports, rejected) = identify(&synth::awgn(&rtty, 20.0, 17));
    assert!(reports.is_empty(), "{reports:?}");
    assert!(rejected > 0);
}

#[test]
fn a_modem_that_stops_is_reported_gone() {
    let mut iq = synth::stanag_4285(SECONDS, 1_800.0, 1);
    iq.extend(vec![Complex::default(); (SECONDS * synth::RATE) as usize]);
    let (reports, _) = identify(&synth::awgn(&iq, SNR_DB, 18));
    let modems: Vec<_> = reports.iter().map(|report| report.modem).collect();
    assert_eq!(modems, [Some(StanagModem::Stanag4285), None]);
}

#[test]
fn the_dial_frequency_places_the_carrier() {
    let mut channel = StanagChannel::new(
        ChannelCtx {
            input_rate: INPUT_RATE_HZ,
        },
        sdrmm_wire::ChannelSettings {
            frequency_hz: 8_502_000.0,
            ..settings(ChannelParams::Stanag(StanagParams::default()))
        },
    )
    .unwrap();
    let iq = synth::awgn(&synth::stanag_4285(SECONDS, 1_800.0, 1), SNR_DB, 19);
    let report = run(&mut channel, &iq).pop().unwrap();
    assert!((report.frequency_hz.unwrap() - 8_503_800.0).abs() < 1.0);
}

#[test]
fn settings_out_of_range_are_refused() {
    for window_s in [1, 13] {
        let refused = StanagChannel::new(
            ChannelCtx {
                input_rate: INPUT_RATE_HZ,
            },
            settings(ChannelParams::Stanag(StanagParams { window_s })),
        );
        assert!(refused.is_err(), "{window_s}");
    }
    let wrong = StanagChannel::new(
        ChannelCtx {
            input_rate: INPUT_RATE_HZ,
        },
        settings(ChannelParams::Sid(sdrmm_wire::SidParams::default())),
    );
    assert!(wrong.is_err());
}
