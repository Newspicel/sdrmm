#![allow(clippy::expect_used)]

//! The signal identifier against the other decoders' signals, which it names and confirms
//! through the registry.

use std::time::Instant;

use num_complex::Complex;
use sdrmm_channels::{
    IdentChannel, IdentParams, synth, synth::ident_fixtures::RATE_HZ as INPUT_RATE_HZ,
};
use sdrmm_decoder::{Channel, ChannelCtx, ChannelOutputs, NativeChannel};
use sdrmm_decoder_test_support::{complex_noise, realtime_budget};
use sdrmm_wire::{ChannelSettings, IdentReport, IdentSignal, Modulation};

const INTERVAL_MS: u32 = 500;

fn params() -> IdentParams {
    IdentParams {
        interval_ms: INTERVAL_MS,
        ..IdentParams::default()
    }
}

fn settings(params: IdentParams) -> ChannelSettings {
    settings_at(params, 0.0)
}

fn settings_at(params: IdentParams, frequency_hz: f64) -> ChannelSettings {
    ChannelSettings {
        frequency_hz,
        squelch: sdrmm_wire::Squelch::Off,
        params: sdrmm_wire::ChannelParams::new("ident", &params).expect("ident settings"),
        blanker: Default::default(),
        if_filter: Default::default(),
    }
}

fn run(params: IdentParams, iq: &[Complex<f32>]) -> Vec<IdentReport> {
    run_at(settings(params), iq)
}

fn run_at(settings: ChannelSettings, iq: &[Complex<f32>]) -> Vec<IdentReport> {
    let ctx = ChannelCtx {
        input_rate: INPUT_RATE_HZ,
    };
    let mut channel =
        IdentChannel::new(ctx, settings).expect("ident channel builds at its own rate");
    let mut out = ChannelOutputs::default();
    let mut reports = Vec::new();
    let mut pos = 0;
    for len in [8_191usize, 1, 65_536, 129, 4_096].iter().cycle() {
        if pos >= iq.len() {
            break;
        }
        let end = (pos + len).min(iq.len());
        out.reset();
        channel.process(&iq[pos..end], &mut out);
        for event in out.events.drain(..) {
            match event.read::<IdentReport>() {
                Some(report) => reports.push(report),
                None => panic!("unexpected {} event", event.kind),
            }
        }
        pos = end;
    }
    reports
}

fn in_noise(iq: &[Complex<f32>], seconds: f64, seed: u32, amp: f32) -> Vec<Complex<f32>> {
    let wanted = (seconds * INPUT_RATE_HZ) as usize;
    let mut out = Vec::with_capacity(wanted + iq.len());
    while out.len() < wanted {
        out.extend_from_slice(iq);
    }
    let noise = complex_noise(seed, amp, out.len());
    for (sample, noise) in out.iter_mut().zip(noise) {
        *sample += noise;
    }
    out
}

fn on_air(iq: &[Complex<f32>], seconds: f64, seed: u32) -> Vec<Complex<f32>> {
    in_noise(iq, seconds, seed, 0.004)
}

fn programme(len: usize, seed: u32) -> Vec<f32> {
    let mut state = seed | 1;
    let mut smoothed = 0.0f32;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let noise = state as f32 / u32::MAX as f32 - 0.5;
            smoothed += 0.08 * (noise - smoothed);
            smoothed * 8.0
        })
        .collect()
}

fn best(report: &IdentReport) -> Option<&str> {
    report.best().map(|m| m.name.as_str())
}

fn family(report: &IdentReport) -> Modulation {
    report
        .loudest()
        .map_or(Modulation::None, |signal| signal.modulation)
}

fn loudest(report: &IdentReport) -> &IdentSignal {
    report.loudest().expect("a signal was found")
}

fn consensus(reports: &[IdentReport]) -> Modulation {
    let mut counts: Vec<(Modulation, usize)> = Vec::new();
    for report in reports {
        let found = family(report);
        match counts.iter_mut().find(|(m, _)| *m == found) {
            Some((_, n)) => *n += 1,
            None => counts.push((found, 1)),
        }
    }
    counts
        .into_iter()
        .max_by_key(|&(_, n)| n)
        .map_or(Modulation::None, |(m, _)| m)
}

fn carrier(offset: f64, len: usize, seed: u32) -> Vec<Complex<f32>> {
    let mut iq: Vec<Complex<f32>> = (0..len)
        .map(|k| {
            Complex::from_polar(
                0.5,
                (std::f64::consts::TAU * offset * k as f64 / INPUT_RATE_HZ)
                    .rem_euclid(std::f64::consts::TAU) as f32,
            )
        })
        .collect();
    for (s, n) in iq.iter_mut().zip(complex_noise(seed, 0.002, len)) {
        *s += n;
    }
    iq
}

#[test]
fn a_quiet_channel_says_no_signal_once_and_then_keeps_quiet() {
    let noise = complex_noise(0x2b71, 0.02, (INPUT_RATE_HZ * 2.2) as usize);
    let reports = run(params(), &noise);
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(reports[0].signals.is_empty());
    assert!(reports[0].best().is_none());
}

#[test]
fn reports_arrive_once_per_interval_while_something_is_on_the_air() {
    let iq = carrier(42_000.0, (INPUT_RATE_HZ * 2.0) as usize, 0x9c14);
    let reports = run(params(), &iq);
    assert_eq!(reports.len(), 4);
    assert!(reports.iter().all(|report| !report.signals.is_empty()));
}

#[test]
fn a_signal_that_stops_is_reported_gone_once() {
    let mut iq = carrier(42_000.0, (INPUT_RATE_HZ * 1.0) as usize, 0x9c15);
    iq.extend(complex_noise(0x9c16, 0.02, (INPUT_RATE_HZ * 1.6) as usize));
    let reports = run(params(), &iq);
    let heard = reports
        .iter()
        .take_while(|report| !report.signals.is_empty())
        .count();
    assert_eq!(heard, 2, "{reports:?}");
    assert_eq!(reports.len(), 3, "one report says the signal is gone");
    assert!(reports[2].signals.is_empty());
}

#[test]
fn a_retune_reports_the_new_dial_even_when_it_is_quiet() {
    let ctx = ChannelCtx {
        input_rate: INPUT_RATE_HZ,
    };
    let mut channel = IdentChannel::new(ctx, settings(params())).expect("builds");
    let mut out = ChannelOutputs::default();
    let window = (INPUT_RATE_HZ * f64::from(INTERVAL_MS) / 1_000.0) as usize;
    channel.process(&complex_noise(0x5566, 0.02, 2 * window), &mut out);
    assert_eq!(out.events.len(), 1);
    channel.retuned();
    channel.process(&complex_noise(0x7788, 0.02, window), &mut out);
    assert_eq!(out.events.len(), 2, "the new dial gets its own first word");
}

#[test]
fn an_unmodulated_carrier_is_named_and_located() {
    let offset = 42_000.0;
    let iq = carrier(offset, (INPUT_RATE_HZ * 1.2) as usize, 0x4411);
    let reports = run(params(), &iq);
    assert_eq!(consensus(&reports), Modulation::Carrier);
    let first = loudest(&reports[0]);
    assert!(
        (first.center_offset_hz - offset).abs() < 500.0,
        "offset {} Hz",
        first.center_offset_hz
    );
    assert_eq!(best(&reports[0]), Some("Unmodulated carrier"));
}

#[test]
fn a_dmr_transmission_is_four_level_and_confirmed_by_its_framing() {
    let call = synth::dmr::Call::default();
    let iq = on_air(&synth::dmr::transmission(&call, INPUT_RATE_HZ), 2.0, 0x71a2);
    let reports = run(params(), &iq);
    assert_eq!(consensus(&reports), Modulation::Fsk4);
    let confirmed = reports
        .iter()
        .find(|r| r.best().is_some_and(|m| m.confirmed))
        .expect("a DMR transmission carries its own frame sync");
    let best = confirmed.best().expect("checked above");
    assert_eq!(best.name, "DMR");
    assert_eq!(best.type_id.as_deref(), Some("dmr"));
    let signal = loudest(confirmed);
    assert!(
        (signal.symbol_rate_hz.unwrap_or_default() - 4_800.0).abs() < 250.0,
        "baud {:?}",
        signal.symbol_rate_hz
    );
}

#[test]
fn a_p25_transmission_is_told_apart_from_dmr() {
    let iq = on_air(&synth::p25::transmission(0x293, INPUT_RATE_HZ), 2.0, 0x33c1);
    let reports = run(params(), &iq);
    assert_eq!(consensus(&reports), Modulation::Fsk4);
    let confirmed = reports
        .iter()
        .find(|r| r.best().is_some_and(|m| m.confirmed))
        .expect("a P25 transmission carries its own frame sync");
    assert_eq!(best(confirmed), Some("P25 Phase 1"));
    assert!(
        loudest(confirmed)
            .candidates
            .iter()
            .any(|m| m.name == "DMR")
    );
}

#[test]
fn a_pager_transmission_is_two_level_at_its_own_baud() {
    let pages = [synth::pocsag::Page {
        address: 1_234_567,
        function: 3,
        text: "IDENT TEST".to_owned(),
        numeric: false,
        seal: None,
    }];
    let iq = on_air(
        &synth::pocsag::transmission(&pages, 1_200, 4_500.0, INPUT_RATE_HZ),
        2.0,
        0x5d90,
    );
    let reports = run(params(), &iq);
    assert_eq!(consensus(&reports), Modulation::Fsk2);
    let named = reports
        .iter()
        .find(|r| best(r) == Some("POCSAG (1200 bd)"))
        .expect("a 1200 baud pager shift is POCSAG at 1200 baud");
    let signal = loudest(named);
    assert!(
        (signal.deviation_hz.unwrap_or_default() - 4_500.0).abs() < 1_200.0,
        "deviation {:?}",
        signal.deviation_hz
    );
    let confirmed = reports
        .iter()
        .find(|r| r.best().is_some_and(|m| m.confirmed))
        .expect("the POCSAG decoder reads the pages");
    assert_eq!(best(confirmed), Some("POCSAG (1200 bd)"));
    assert!(
        confirmed
            .best()
            .is_some_and(|m| m.why.contains("decoder read")),
        "{:?}",
        confirmed.best()
    );
}

#[test]
fn a_weak_pager_is_a_shift_and_not_amplitude_modulation() {
    let pages = [synth::pocsag::Page {
        address: 1_234_567,
        function: 3,
        text: "IDENT TEST".to_owned(),
        numeric: false,
        seal: None,
    }];
    let iq = in_noise(
        &synth::pocsag::transmission(&pages, 1_200, 4_500.0, INPUT_RATE_HZ),
        2.0,
        0x5d90,
        0.8,
    );
    let reports = run(params(), &iq);
    let strongest = reports
        .iter()
        .filter_map(|r| r.loudest())
        .map(|s| s.snr_db)
        .fold(0.0, f32::max);
    assert!(
        strongest < 20.0,
        "meant to be a weak signal, got {strongest} dB"
    );
    assert_eq!(consensus(&reports), Modulation::Fsk2);
    assert!(
        reports.iter().any(|r| r
            .best()
            .is_some_and(|m| m.type_id.as_deref() == Some("pocsag"))),
        "candidates: {:?}",
        reports.iter().map(best).collect::<Vec<_>>()
    );
}

#[test]
fn weak_fm_voice_stays_analog() {
    let audio = programme((INPUT_RATE_HZ * 2.2) as usize, 0x4d21);
    let iq = in_noise(
        &sdrmm_modem::synth::fm_modulate(&audio, 3_000.0, INPUT_RATE_HZ),
        2.0,
        0x2ea7,
        1.2,
    );
    let reports = run(params(), &iq);
    let strongest = reports
        .iter()
        .filter_map(|r| r.loudest())
        .map(|s| s.snr_db)
        .fold(0.0, f32::max);
    assert!(
        strongest < 20.0,
        "meant to be a weak signal, got {strongest} dB"
    );
    assert_eq!(consensus(&reports), Modulation::Fm);
}

#[test]
fn a_broadcast_signal_is_wideband_fm() {
    let audio = programme(48_000, 0x4d21);
    let iq = on_air(
        &synth::wfm::transmission(&audio, &audio, true, INPUT_RATE_HZ),
        1.6,
        0x6f02,
    );
    let reports = run(params(), &iq);
    assert_eq!(consensus(&reports), Modulation::Fm);
    assert!(
        reports.iter().any(|r| best(r) == Some("FM broadcast")),
        "candidates: {:?}",
        reports.iter().map(best).collect::<Vec<_>>()
    );
}

fn broadcast_audio(len: usize, seed: u32, smoothing: f32) -> Vec<f32> {
    let mut state = seed | 1;
    let mut smoothed = 0.0f32;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let noise = state as f32 / u32::MAX as f32 - 0.5;
            smoothed += smoothing * (noise - smoothed);
            (smoothed * 14.0).clamp(-1.0, 1.0)
        })
        .collect()
}

fn station(smoothing: f32) -> Vec<Complex<f32>> {
    let len = (INPUT_RATE_HZ * 2.2) as usize;
    let mut iq = synth::wfm::transmission(
        &broadcast_audio(len, 0x4d21, smoothing),
        &broadcast_audio(len, 0x7712, smoothing),
        true,
        INPUT_RATE_HZ,
    );
    sdrmm_modem::synth::add_noise(&mut iq, 0x6f02, 0.004);
    iq
}

#[test]
fn a_loud_stereo_station_is_broadcast_fm_in_every_window() {
    let reports = run(params(), &station(0.35));
    assert!(reports.len() >= 4, "{} reports", reports.len());
    for report in &reports {
        assert_eq!(family(report), Modulation::Fm, "{report:?}");
        assert_eq!(best(report), Some("FM broadcast"));
        let signal = loudest(report);
        assert!(signal.confidence > 0.6, "confidence {}", signal.confidence);
    }
}

#[test]
fn a_processed_station_is_neither_keyed_nor_shifted() {
    let reports = run(params(), &station(1.0));
    assert!(reports.len() >= 4, "{} reports", reports.len());
    for report in &reports {
        assert_eq!(family(report), Modulation::Fm, "{report:?}");
        let signal = loudest(report);
        assert!(
            signal.bandwidth_hz > 100_000.0,
            "bandwidth {} Hz",
            signal.bandwidth_hz
        );
    }
}

#[test]
fn a_station_carrying_one_tone_is_still_broadcast_fm() {
    let len = (INPUT_RATE_HZ * 2.2) as usize;
    let tone = sdrmm_modem::synth::tone_audio(1_000.0, 1.0, INPUT_RATE_HZ, len);
    let mut iq = synth::wfm::transmission(&tone, &tone, true, INPUT_RATE_HZ);
    sdrmm_modem::synth::add_noise(&mut iq, 0x6f02, 0.004);
    let reports = run_at(settings_at(params(), 95_500_000.0), &iq);
    assert_eq!(consensus(&reports), Modulation::Fm, "{reports:?}");
    assert!(
        reports.iter().any(|r| best(r) == Some("FM broadcast")),
        "candidates: {:?}",
        reports.iter().map(best).collect::<Vec<_>>()
    );
}

#[test]
fn a_keyed_carrier_is_morse_rather_than_a_bare_carrier() {
    let mut iq =
        sdrmm_modem::synth::morse::transmission("CQ CQ DE TEST", 20.0, 800.0, INPUT_RATE_HZ);
    sdrmm_modem::synth::add_noise(&mut iq, 0x3311, 0.004);
    let reports = run(params(), &iq);
    assert_eq!(consensus(&reports), Modulation::Ook);
    assert!(
        reports.iter().any(|r| best(r) == Some("Morse (CW)")),
        "candidates: {:?}",
        reports.iter().map(best).collect::<Vec<_>>()
    );
}

#[test]
fn a_deeply_modulated_carrier_is_amplitude_modulation_not_keying() {
    let len = (INPUT_RATE_HZ * 2.2) as usize;
    let mut iq: Vec<Complex<f32>> =
        sdrmm_modem::synth::tone_audio(1_000.0, 1.0, INPUT_RATE_HZ, len)
            .iter()
            .map(|&a| Complex::new(f32::midpoint(1.0, 0.8 * a), 0.0))
            .collect();
    sdrmm_modem::synth::add_noise(&mut iq, 0x5511, 0.004);
    let reports = run(params(), &iq);
    assert_eq!(consensus(&reports), Modulation::Am);
    assert!(
        reports.iter().all(|r| family(r) != Modulation::Ook),
        "an 80 percent modulated carrier dips without ever being keyed off"
    );
    assert_eq!(best(&reports[0]), Some("AM voice"));
}

#[test]
fn a_retune_discards_the_half_window_it_was_holding() {
    let ctx = ChannelCtx {
        input_rate: INPUT_RATE_HZ,
    };
    let mut channel = IdentChannel::new(ctx, settings(params())).expect("builds");
    let mut out = ChannelOutputs::default();
    let half = (INPUT_RATE_HZ * f64::from(INTERVAL_MS) / 1_000.0) as usize / 2;
    channel.process(&complex_noise(0x1122, 0.02, half + 10), &mut out);
    assert!(out.events.is_empty());
    channel.retuned();
    channel.process(&complex_noise(0x3344, 0.02, half), &mut out);
    assert!(
        out.events.is_empty(),
        "the pre-retune samples must not have counted towards this window"
    );
}

#[test]
fn one_report_costs_far_less_than_the_signal_it_describes() {
    let iq = on_air(
        &synth::dmr::transmission(&synth::dmr::Call::default(), INPUT_RATE_HZ),
        2.0,
        0x0f31,
    );
    let _ = run(params(), &iq);

    let mut reports = Vec::new();
    let mut elapsed = Vec::with_capacity(3);
    for _ in 0..3 {
        let started = Instant::now();
        reports = run(params(), &iq);
        elapsed.push(started.elapsed());
    }
    elapsed.sort_unstable();
    let elapsed = elapsed[1].as_secs_f64();
    let described = reports.len() as f64 * f64::from(INTERVAL_MS) / 1_000.0;
    assert!(described > 0.0, "the run produced no reports");
    assert!(
        elapsed < realtime_budget(described / 2.0),
        "identification took {elapsed:.3} s for {described:.1} s of signal"
    );
}

#[test]
fn a_long_interval_lengthens_the_cadence_rather_than_the_analysis() {
    let long = IdentParams {
        interval_ms: 2_000,
        ..IdentParams::default()
    };
    let noise = complex_noise(0x77c2, 0.02, (INPUT_RATE_HZ * 4.5) as usize);
    let on_air = carrier(42_000.0, noise.len(), 0x77c3);
    assert_eq!(run(long, &on_air).len(), 2);
}

#[test]
fn two_transmissions_in_one_window_are_both_named() {
    let seconds = 2.0;
    let mut iq = on_air(
        &synth::dmr::transmission(&synth::dmr::Call::default(), INPUT_RATE_HZ),
        seconds,
        0x71a2,
    );
    sdrmm_modem::synth::shift(&mut iq, 60_000.0, INPUT_RATE_HZ);
    let pages = [synth::pocsag::Page {
        address: 1_234_567,
        function: 3,
        text: "SURVEY".to_owned(),
        numeric: false,
        seal: None,
    }];
    let mut pager = in_noise(
        &synth::pocsag::transmission(&pages, 1_200, 4_500.0, INPUT_RATE_HZ),
        seconds,
        0x5d90,
        0.0,
    );
    sdrmm_modem::synth::shift(&mut pager, -50_000.0, INPUT_RATE_HZ);
    for (s, p) in iq.iter_mut().zip(&pager) {
        *s += p;
    }
    let reports = run(params(), &iq);
    let survey = reports
        .iter()
        .find(|r| r.signals.len() == 2)
        .unwrap_or_else(|| panic!("both signals are listed: {reports:?}"));
    let at = |offset: f64| {
        survey
            .signals
            .iter()
            .find(|s| (s.center_offset_hz - offset).abs() < 3_000.0)
            .unwrap_or_else(|| panic!("no signal near {offset} Hz in {:?}", survey.signals))
    };
    assert_eq!(at(60_000.0).modulation, Modulation::Fsk4);
    assert_eq!(at(-50_000.0).modulation, Modulation::Fsk2);
    assert_eq!(
        at(-50_000.0).best().map(|m| m.name.as_str()),
        Some("POCSAG (1200 bd)")
    );
}

#[test]
fn a_signal_knows_where_it_sits_on_the_dial() {
    let offset = 42_000.0;
    let dial = 145_000_000.0;
    let len = (INPUT_RATE_HZ * 1.2) as usize;
    let mut iq: Vec<Complex<f32>> = (0..len)
        .map(|k| {
            Complex::from_polar(
                0.5,
                (std::f64::consts::TAU * offset * k as f64 / INPUT_RATE_HZ)
                    .rem_euclid(std::f64::consts::TAU) as f32,
            )
        })
        .collect();
    sdrmm_modem::synth::add_noise(&mut iq, 0x4411, 0.002);
    let reports = run_at(settings_at(params(), dial), &iq);
    let signal = loudest(&reports[0]);
    assert!(
        (signal.frequency_hz - (dial + offset)).abs() < 500.0,
        "{} Hz",
        signal.frequency_hz
    );
}

#[test]
fn the_dial_frequency_is_part_of_the_evidence() {
    let pages = [synth::pocsag::Page {
        address: 1_234_567,
        function: 3,
        text: "IDENT TEST".to_owned(),
        numeric: false,
        seal: None,
    }];
    let iq = on_air(
        &synth::pocsag::transmission(&pages, 1_200, 4_500.0, INPUT_RATE_HZ),
        1.2,
        0x5d90,
    );
    let reports = run_at(settings_at(params(), 466_075_000.0), &iq);
    let placed = reports
        .iter()
        .find(|r| best(r) == Some("POCSAG (1200 bd)"))
        .expect("named");
    assert!(
        placed
            .best()
            .is_some_and(|m| m.why.contains("allocation") || m.confirmed),
        "{:?}",
        placed.best()
    );
}

#[test]
fn a_slotted_transmission_reports_its_bursts_and_the_bursts_favour_dmr() {
    let call = synth::dmr::Call::default();
    let iq = on_air(
        &synth::dmr::simplex_transmission(&call, INPUT_RATE_HZ),
        2.0,
        0x71a3,
    );
    let reports = run(params(), &iq);
    assert_eq!(consensus(&reports), Modulation::Fsk4);
    let bursty = reports
        .iter()
        .filter_map(|r| r.loudest())
        .find(|s| s.burst_ms.is_some())
        .unwrap_or_else(|| panic!("a single-slot call is bursty: {reports:?}"));
    let burst = bursty.burst_ms.unwrap_or_default();
    assert!((burst - 30.0).abs() < 5.0, "burst {burst} ms");
    let period = bursty.burst_period_ms.expect("slots repeat every frame");
    assert!((period - 60.0).abs() < 6.0, "period {period} ms");
    assert_eq!(bursty.best().map(|m| m.name.as_str()), Some("DMR"));
}

fn ofdm_like_dab(seconds: f64, seed: u32) -> Vec<Complex<f32>> {
    const RATE: f64 = 2_048_000.0;
    const USEFUL: usize = 2_048;
    const GUARD: usize = 504;
    const CARRIERS: usize = 1_536;
    let mut fft = sdrmm_dsp::math::fft::FftPair::new(USEFUL);
    let mut state = seed | 1;
    let symbols = (seconds * RATE / (USEFUL + GUARD) as f64) as usize;
    let mut out = Vec::with_capacity(symbols * (USEFUL + GUARD));
    let mut symbol = vec![Complex::default(); USEFUL];
    for _ in 0..symbols {
        symbol.fill(Complex::default());
        for k in 1..=CARRIERS / 2 {
            for index in [k, USEFUL - k] {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let phase =
                    (state % 4) as f32 * std::f32::consts::FRAC_PI_2 + std::f32::consts::FRAC_PI_4;
                symbol[index] = Complex::from_polar(2.0, phase);
            }
        }
        fft.inverse_scaled(&mut symbol);
        out.extend_from_slice(&symbol[USEFUL - GUARD..]);
        out.extend_from_slice(&symbol);
    }
    sdrmm_modem::synth::resample(&out, RATE, INPUT_RATE_HZ)
}

#[test]
fn a_slice_of_a_dab_ensemble_is_ofdm_with_a_millisecond_symbol() {
    let mut iq = ofdm_like_dab(2.2, 0x0da8);
    sdrmm_modem::synth::add_noise(&mut iq, 0x0da8, 0.002);
    let reports = run_at(settings_at(params(), 227_360_000.0), &iq);
    assert_eq!(consensus(&reports), Modulation::Ofdm, "{reports:?}");
    let signal = reports
        .iter()
        .filter_map(|r| r.loudest())
        .find(|s| s.modulation == Modulation::Ofdm)
        .expect("checked above");
    let symbol = signal.ofdm_symbol_us.expect("useful symbol");
    assert!((symbol - 1_000.0).abs() < 30.0, "symbol {symbol} µs");
    let guard = signal.ofdm_guard_us.expect("guard interval");
    assert!((guard - 246.0).abs() < 25.0, "guard {guard} µs");
    assert_eq!(signal.best().map(|m| m.name.as_str()), Some("DAB / DAB+"));
}

#[test]
fn a_broadcast_station_is_confirmed_by_its_rds() {
    let station = synth::rds::Station {
        pi: 0xD3C2,
        ps: "SDR-M4  ".to_owned(),
        radiotext: "identifier".to_owned(),
        pty: 10,
        tp: true,
        ta: false,
        music: true,
        alt_freqs_hz: Vec::new(),
    };
    let mut iq = synth::rds::transmission(&station, 2.5, Some(1_000.0), INPUT_RATE_HZ);
    sdrmm_modem::synth::add_noise(&mut iq, 0x6f02, 0.002);
    let reports = run_at(settings_at(params(), 95_500_000.0), &iq);
    let confirmed = reports
        .iter()
        .find(|r| r.best().is_some_and(|m| m.confirmed))
        .unwrap_or_else(|| panic!("the wfm decoder reads the RDS groups: {reports:?}"));
    assert_eq!(best(confirmed), Some("FM broadcast"));
}
