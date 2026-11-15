use std::f64::consts::TAU;

use sdrmm_modem_test_support::ber::rng::Rng;

use super::{
    super::{known_symbols::KnownSymbols, modulator::CpmMod, testutil::transmit},
    gate::CARRIER_RISE,
    *,
};
use crate::pulse::{self, Norm};
use crate::testutil::draws;

const RATE: f64 = 48_000.0;
const BAUD: f64 = 4_800.0;
const SPS: f64 = 10.0;

fn dibit_mapping() -> Mapping {
    Mapping::new(vec![1.0, 3.0, -1.0, -3.0])
}

fn four_level(deviation_hz: f64) -> CpmParams {
    CpmParams::from_deviation(
        dibit_mapping(),
        deviation_hz,
        BAUD,
        pulse::root_raised_cosine(SPS, 0.2, 8, Norm::Area),
        SPS,
    )
}

fn rx_rrc() -> Vec<f32> {
    pulse::root_raised_cosine(SPS, 0.2, 8, Norm::Area)
}

const NOISE: f32 = 0.01;

fn noise(seed: u64, len: usize) -> Vec<Complex<f32>> {
    let mut rng = Rng::new(seed);
    (0..len)
        .map(|_| {
            let re = (rng.uniform() as f32 * 2.0 - 1.0) * NOISE;
            let im = (rng.uniform() as f32 * 2.0 - 1.0) * NOISE;
            Complex::new(re, im)
        })
        .collect()
}

fn real_noise(seed: u64, len: usize) -> Vec<f32> {
    let mut rng = Rng::new(seed);
    (0..len)
        .map(|_| (rng.uniform() as f32 * 2.0 - 1.0) * NOISE)
        .collect()
}

fn listening(demod: &mut CpmDemod, seed: u64) {
    let len = demod.gate.settle_samples + 4 * SPS as usize * 100;
    let quiet = noise(seed, len);
    let mut discard = Vec::new();
    demod.process(&quiet, &mut discard);
}

#[test]
fn a_carrier_that_never_stopped_keys_the_gate_from_a_cold_start() {
    let params = four_level(1_944.0);
    let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
    let iq = transmit(&params, &draws(4_000, 4, 0x5eed));
    let mut out = Vec::new();

    demod.process(&iq, &mut out);

    let opened = demod
        .settled()
        .iter()
        .position(|&settled| settled)
        .expect("the gate never opened on a carrier that was already running");
    assert!(
        opened < 600,
        "the gate took {opened} symbols to open on a live carrier"
    );
}

#[test]
fn a_dip_inside_a_burst_does_not_restart_the_gate() {
    let params = four_level(1_944.0);
    let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
    listening(&mut demod, 0x1157);
    let threshold = demod.gate.floor * CARRIER_RISE;
    let carrier = 3.0 * threshold;
    for _ in 0..demod.gate.support * 2 {
        demod.gate.step(carrier, None);
    }
    assert!(
        demod.gate.step(carrier, None).1,
        "a steady carrier settles the gate"
    );
    let mut dipped = 0;
    while demod.gate.envelope >= threshold {
        demod.gate.step(0.0, None);
        dipped += 1;
    }
    let mut settled = true;
    for _ in 0..2 {
        settled &= demod.gate.step(0.0, None).1;
    }
    for _ in 0..20 {
        settled &= demod.gate.step(carrier, None).1;
    }
    assert!(settled, "a {}-sample dip restarted the gate", dipped + 2);
}

#[test]
fn noise_alone_never_keys_the_gate() {
    let params = four_level(1_944.0);
    let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
    let mut out = Vec::new();

    demod.process(&noise(0x1157, 96_000), &mut out);

    assert!(
        demod.settled().iter().all(|&settled| !settled),
        "noise on its own was taken for a carrier"
    );
}

#[test]
fn a_carrier_that_stops_shuts_the_gate_and_reopens_it_when_it_returns() {
    let params = four_level(1_944.0);
    let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
    let iq = transmit(&params, &draws(4_000, 4, 0x5eed));
    let mut out = Vec::new();

    demod.process(&iq, &mut out);
    assert!(demod.settled().iter().any(|&settled| settled));

    demod.process(&noise(0x2468, 96_000), &mut out);
    demod.process(&noise(0x1359, 96_000), &mut out);
    assert!(
        demod.settled().iter().all(|&settled| !settled),
        "the gate stayed open after the carrier went away"
    );

    demod.process(&iq, &mut out);
    assert!(
        demod.settled().iter().any(|&settled| settled),
        "the gate never reopened when the carrier came back"
    );
}

fn symbol_errors(got: &[u8], sent: &[u8], skip: usize) -> (usize, usize) {
    let (delay, errors) = (0..48)
        .map(|delay| {
            let errors = got
                .iter()
                .enumerate()
                .skip(skip)
                .filter(|&(i, s)| sent.get(i.wrapping_sub(delay)).is_none_or(|w| w != s))
                .count();
            (delay, errors)
        })
        .min_by_key(|&(_, errors)| errors)
        .unwrap();
    assert!((0..40).contains(&delay), "implausible alignment {delay}");
    (errors, got.len() - skip)
}

#[test]
fn the_centre_tracker_reads_back_the_carrier_offset_it_was_given() {
    let sent = draws(20_000, 4, 23);
    let params = four_level(1_944.0);
    let clean = transmit(&params, &sent);
    let measure = |offset_hz: f64| {
        let shifted: Vec<Complex<f32>> = clean
            .iter()
            .enumerate()
            .map(|(i, &x)| {
                let phase = TAU * offset_hz * i as f64 / RATE;
                x * Complex::new(phase.cos() as f32, phase.sin() as f32)
            })
            .collect();
        let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
        listening(&mut demod, 0x2211);
        let mut soft = Vec::new();
        demod.process(&shifted, &mut soft);
        demod.frequency_error_cycles_per_sample() * RATE
    };

    let base = measure(0.0);
    for offset_hz in [-600.0f64, -200.0, 200.0, 600.0] {
        let moved = measure(offset_hz) - base;
        assert!(
            (moved - offset_hz).abs() < 20.0,
            "a {offset_hz} Hz shift moved the tracker by {moved} Hz"
        );
    }
}

#[test]
fn recovers_four_level_symbols_at_an_unexpected_deviation() {
    for deviation in [1_944.0, 1_400.0, 2_600.0] {
        let sent = draws(400, 4, 17);
        let iq = transmit(&four_level(deviation), &sent);
        let nominal = four_level(1_944.0);
        let mut demod = CpmDemod::new(&nominal, &rx_rrc(), TIMING_BW_BURST);
        listening(&mut demod, 0x1157);
        let mut soft = Vec::new();
        demod.process(&iq, &mut soft);
        let got: Vec<u8> = soft.iter().map(|&s| nominal.mapping().slice(s)).collect();
        let (errors, total) = symbol_errors(&got, &sent, 100);
        assert!(total > 280, "only {total} symbols at {deviation} Hz");
        assert_eq!(errors, 0, "symbol errors at {deviation} Hz deviation");
    }
}

#[test]
fn a_noisy_four_level_burst_locks_within_a_few_hundred_symbols() {
    let sent = draws(600, 4, 0x5eed);
    let params = four_level(1_944.0);
    let clean = transmit(&params, &sent);
    let mut errors = 0;
    for delay in 0..SPS as usize {
        let mut iq = noise(0x77, delay);
        iq.extend_from_slice(&clean);
        let mut rng = Rng::new(7 + delay as u64);
        for sample in &mut iq {
            let re = (rng.uniform() as f32 * 2.0 - 1.0) * 0.5;
            let im = (rng.uniform() as f32 * 2.0 - 1.0) * 0.5;
            *sample += Complex::new(re, im);
        }
        let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
        listening(&mut demod, 0x1157);
        let mut soft = Vec::new();
        demod.process(&iq, &mut soft);
        let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
        let (e, t) = symbol_errors(&got[..400], &sent, 100);
        assert_eq!(t, 300);
        errors += e;
    }
    assert!(
        errors <= 100,
        "{errors} symbol errors after a hundred symbols"
    );
}

#[test]
fn a_long_run_of_one_level_does_not_pull_the_slicer_off_centre() {
    let mut sent = draws(400, 4, 7);
    for _ in 0..4 {
        sent.extend(std::iter::repeat_n(0u8, 150));
        sent.extend(draws::<u8>(40, 4, 8));
    }
    let params = four_level(1_944.0);
    let iq = transmit(&params, &sent);
    let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
    listening(&mut demod, 0x1157);
    let mut soft = Vec::new();
    demod.process(&iq, &mut soft);
    let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
    let (errors, total) = symbol_errors(&got, &sent, 300);
    assert!(total > 850, "only {total} symbols");
    assert!(errors <= 2, "{errors} symbol errors in {total}");
}

#[test]
fn tracks_a_carrier_offset() {
    let sent = draws(900, 4, 5);
    let params = four_level(1_944.0);
    let mut iq = transmit(&params, &sent);
    for (k, s) in iq.iter_mut().enumerate() {
        *s *= Complex::from_polar(1.0, (TAU * 400.0 * k as f64 / RATE) as f32);
    }
    let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
    listening(&mut demod, 0x1157);
    let mut soft = Vec::new();
    demod.process(&iq, &mut soft);
    let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
    let (errors, _) = symbol_errors(&got, &sent, soft.len() - 200);
    assert_eq!(errors, 0);
}

#[test]
fn block_splits_do_not_change_the_symbols() {
    let sent = draws(300, 4, 41);
    let params = four_level(1_944.0);
    let iq = transmit(&params, &sent);
    let mut whole = Vec::new();
    let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
    listening(&mut demod, 0x1157);
    demod.process(&iq, &mut whole);

    let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
    listening(&mut demod, 0x1157);
    let mut ragged = Vec::new();
    let mut pos = 0;
    for len in [997usize, 1, 4_096, 65, 7].iter().cycle() {
        if pos >= iq.len() {
            break;
        }
        let end = (pos + len).min(iq.len());
        demod.process(&iq[pos..end], &mut ragged);
        pos = end;
    }
    assert_eq!(whole, ragged);
}

#[test]
fn a_keyed_transmitter_does_not_lose_its_clock_in_the_dead_time() {
    const ON: usize = 132;
    const FRAME: usize = 288;
    let params = four_level(1_944.0);
    let sent = draws(2_880, 4, 23);
    let keyed: Vec<Option<u8>> = sent
        .iter()
        .enumerate()
        .map(|(i, &s)| (i % FRAME < ON).then_some(s))
        .collect();
    let mut iq = CpmMod::new(params.clone()).keyed(&keyed);
    let floor = noise(0xbeef, iq.len());
    for (s, n) in iq.iter_mut().zip(floor) {
        *s += n;
    }
    let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
    listening(&mut demod, 0x1157);
    let mut soft = Vec::new();
    demod.process(&iq, &mut soft);

    let ideal = iq.len() / SPS as usize;
    assert!(
        (soft.len() as i64 - ideal as i64).abs() <= 2,
        "recovered {} symbols, ideal {ideal}",
        soft.len()
    );

    let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
    let (delay, _) = (0..48)
        .map(|delay| {
            let errors = (300usize..400)
                .filter(|&i| sent.get(i.wrapping_sub(delay)).is_none_or(|w| *w != got[i]))
                .count();
            (delay, errors)
        })
        .min_by_key(|&(_, errors)| errors)
        .unwrap();
    let last = sent.len() - FRAME + delay;
    let bad: Vec<usize> = (last..last + ON)
        .filter(|&i| sent.get(i - delay).is_none_or(|w| *w != got[i]))
        .map(|i| i - last)
        .collect();
    assert!(
        bad.is_empty(),
        "symbol errors at {bad:?} in the last of {} bursts",
        sent.len() / FRAME
    );
}

#[test]
fn the_first_burst_is_clean_at_any_timing_phase() {
    const ON: usize = 132;
    let params = four_level(1_944.0);
    let sent = draws(ON, 4, 23);
    let mut keyed = vec![None; 40];
    keyed.extend(sent.iter().copied().map(Some));
    keyed.extend(vec![None; 40]);
    let burst = CpmMod::new(params.clone()).keyed(&keyed);
    for delay in 0..SPS as usize {
        let mut iq = noise(0x99, delay);
        iq.extend_from_slice(&burst);
        let floor = noise(0xbeef, iq.len());
        for (s, n) in iq.iter_mut().zip(floor) {
            *s += n;
        }
        let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
        listening(&mut demod, 0x1157);
        let mut soft = Vec::new();
        demod.process(&iq, &mut soft);
        let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
        let errors = (0..soft.len())
            .map(|lag| {
                (1..ON)
                    .filter(|&i| got.get(i + lag) != Some(&sent[i]))
                    .count()
            })
            .min()
            .unwrap_or(ON);
        assert_eq!(errors, 0, "symbol errors at a delay of {delay} samples");
    }
}

#[test]
fn a_continuous_stream_holds_lock_over_twenty_thousand_symbols() {
    let sent = draws(20_000, 4, 0x5eed);
    let params = four_level(1_944.0);
    let iq = transmit(&params, &sent);
    let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_CONTINUOUS);
    listening(&mut demod, 0x1157);
    let mut soft = Vec::new();
    demod.process(&iq, &mut soft);
    let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
    let (errors, total) = symbol_errors(&got, &sent, 500);
    assert!(total > 19_000, "only {total} symbols recovered");
    assert!(
        errors <= total / 1_000,
        "{errors} symbol errors in {total}: the continuous floor is back"
    );
}

#[test]
fn gmsk_loopback_is_clean() {
    let params = CpmParams::from_h(
        Mapping::natural(2),
        0.5,
        pulse::gaussian_freq(SPS, 0.5, 3, Norm::Area),
        SPS,
    );
    let sent = draws(600, 2, 71);
    let iq = transmit(&params, &sent);
    let mut demod = CpmDemod::new(
        &params,
        &pulse::gaussian(SPS, 0.5, 3, Norm::Area),
        TIMING_BW_BURST,
    );
    listening(&mut demod, 0x1157);
    let mut soft = Vec::new();
    demod.process(&iq, &mut soft);
    let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
    let (errors, total) = symbol_errors(&got, &sent, 100);
    assert!(total > 400);
    assert_eq!(errors, 0, "GMSK symbol errors");
}

#[test]
fn two_level_cpfsk_loopback_is_clean() {
    let params = CpmParams::from_h(Mapping::natural(2), 0.5, pulse::rect(SPS, Norm::Area), SPS);
    let sent = draws(600, 2, 29);
    let iq = transmit(&params, &sent);
    let mut demod = CpmDemod::new(&params, &pulse::rect(SPS, Norm::Area), TIMING_BW_BURST);
    listening(&mut demod, 0x1157);
    let mut soft = Vec::new();
    demod.process(&iq, &mut soft);
    let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
    let (errors, total) = symbol_errors(&got, &sent, 100);
    assert!(total > 400);
    assert_eq!(errors, 0, "2FSK symbol errors");
}

#[test]
fn eight_level_loopback_is_clean_on_the_known_symbol_hook() {
    const PATTERN: [u8; 16] = [7, 0, 5, 2, 6, 1, 4, 3, 0, 7, 3, 4, 1, 6, 2, 5];
    const PERIOD: usize = 128;
    const FRAMES: usize = 12;
    let params = CpmParams::from_h(Mapping::natural(8), 0.3, pulse::rect(SPS, Norm::Area), SPS);
    let mut sent = Vec::with_capacity(FRAMES * PERIOD);
    for frame in 0..FRAMES {
        sent.extend_from_slice(&PATTERN);
        sent.extend(draws::<u8>(
            PERIOD - PATTERN.len(),
            8,
            0x0dd5 ^ frame as u64,
        ));
    }
    let iq = transmit(&params, &sent);
    let mut demod = CpmDemod::new(&params, &pulse::rect(SPS, Norm::Area), TIMING_BW_BURST);
    listening(&mut demod, 0x1157);
    let mut soft = Vec::new();
    demod.process(&iq, &mut soft);

    let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
    let (delay, _) = (0..48usize)
        .map(|d| {
            let errs = (200..1_000usize)
                .filter(|&i| {
                    sent.get(i.wrapping_sub(d))
                        .is_none_or(|w| got.get(i).is_none_or(|g| g != w))
                })
                .count();
            (d, errs)
        })
        .min_by_key(|&(_, e)| e)
        .unwrap();
    let mut hook = KnownSymbols::new(&params, (4 * PERIOD) as u32);
    let mut errors = Vec::new();
    for frame in 1..FRAMES {
        let base = frame * PERIOD + delay;
        hook.anchor(&PATTERN, &soft[base..base + PATTERN.len()]);
        for k in PATTERN.len()..PERIOD {
            hook.tick();
            let Some(&s) = soft.get(base + k) else {
                continue;
            };
            if params.mapping().slice(hook.correct(s)) != sent[frame * PERIOD + k] {
                errors.push(frame * PERIOD + k);
            }
        }
    }
    assert!(
        errors.is_empty(),
        "8-level symbol errors at {errors:?} through the hook"
    );
}

#[test]
fn a_silent_channel_produces_finite_symbols() {
    let params = four_level(1_944.0);
    let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
    let mut soft = Vec::new();
    demod.process(&vec![Complex::new(0.0, 0.0); 4_800], &mut soft);
    assert!(!soft.is_empty());
    assert!(soft.iter().all(|s| s.is_finite()), "non-finite symbol");
}

fn afsk_params() -> CpmParams {
    CpmParams::from_deviation(
        Mapping::new(vec![1.0, -1.0]),
        500.0,
        1_200.0,
        pulse::rect(40.0, Norm::Area),
        40.0,
    )
}

fn afsk_rx() -> Vec<f32> {
    pulse::rect(20.0, Norm::Area)
}

fn afsk_audio(sent: &[u8]) -> Vec<f32> {
    let baseband = transmit(&afsk_params(), sent);
    let mut carrier = Nco::new(1_700.0, RATE as f32);
    baseband
        .iter()
        .map(|&s| (s * carrier.next_sample()).re)
        .collect()
}

fn afsk_roundtrip(detector: RealDetector, receive_filter: &[f32], seed: u64) {
    let params = afsk_params();
    let sent = draws(500, 2, seed);
    let audio = afsk_audio(&sent);
    let mut demod = CpmDemod::real(&params, receive_filter, TIMING_BW_BURST, RATE, detector);
    let quiet = real_noise(0x1157, demod.gate.settle_samples + 19_200);
    let mut discard = Vec::new();
    demod.process_real(&quiet, &mut discard);
    let mut soft = Vec::new();
    demod.process_real(&audio, &mut soft);
    let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
    let (errors, total) = symbol_errors(&got, &sent, 80);
    assert!(total > 350, "only {total} symbols");
    assert_eq!(errors, 0, "AFSK bit errors with {detector:?}");
}

#[test]
fn afsk_decodes_through_the_tone_filterbank() {
    afsk_roundtrip(
        RealDetector::ToneFilterbank {
            plus_hz: 2_200.0,
            minus_hz: 1_200.0,
        },
        &afsk_rx(),
        33,
    );
}

#[test]
fn afsk_decodes_through_the_analytic_discriminator() {
    afsk_roundtrip(
        RealDetector::Discriminator { centre_hz: 1_700.0 },
        &pulse::rect(40.0, Norm::Area),
        34,
    );
}

#[test]
fn real_block_splits_do_not_change_the_symbols() {
    let params = afsk_params();
    let sent = draws(200, 2, 9);
    let audio = afsk_audio(&sent);
    let detector = RealDetector::ToneFilterbank {
        plus_hz: 2_200.0,
        minus_hz: 1_200.0,
    };
    let filter = afsk_rx();
    let mut whole = Vec::new();
    let mut demod = CpmDemod::real(&params, &filter, TIMING_BW_BURST, RATE, detector);
    demod.process_real(&audio, &mut whole);

    let mut demod = CpmDemod::real(&params, &filter, TIMING_BW_BURST, RATE, detector);
    let mut ragged = Vec::new();
    let mut pos = 0;
    for len in [997usize, 1, 4_096, 65, 7].iter().cycle() {
        if pos >= audio.len() {
            break;
        }
        let end = (pos + len).min(audio.len());
        demod.process_real(&audio[pos..end], &mut ragged);
        pos = end;
    }
    assert_eq!(whole, ragged);
}

#[test]
fn a_silent_audio_channel_produces_finite_symbols() {
    let params = afsk_params();
    let mut demod = CpmDemod::real(
        &params,
        &pulse::rect(40.0, Norm::Area),
        TIMING_BW_BURST,
        RATE,
        RealDetector::Discriminator { centre_hz: 1_700.0 },
    );
    let mut soft = Vec::new();
    demod.process_real(&vec![0.0; 48_000], &mut soft);
    assert!(!soft.is_empty());
    assert!(soft.iter().all(|s| s.is_finite()), "non-finite symbol");
}

#[test]
fn each_construction_reads_only_the_domain_it_was_built_for() {
    let mut real = CpmDemod::real(
        &afsk_params(),
        &pulse::rect(40.0, Norm::Area),
        TIMING_BW_BURST,
        RATE,
        RealDetector::Discriminator { centre_hz: 1_700.0 },
    );
    let mut soft = Vec::new();
    real.process_real(&[0.0; 4_096], &mut soft);
    real.reset();

    let mut iq = CpmDemod::new(&four_level(1_944.0), &rx_rrc(), TIMING_BW_BURST);
    iq.process(&[Complex::new(0.0, 0.0); 4_096], &mut soft);
    iq.reset();
}

#[test]
#[should_panic(expected = "two levels")]
fn a_filterbank_needs_a_two_level_mapping() {
    let params = four_level(1_944.0);
    let _ = CpmDemod::real(
        &params,
        &rx_rrc(),
        TIMING_BW_BURST,
        RATE,
        RealDetector::ToneFilterbank {
            plus_hz: 2_200.0,
            minus_hz: 1_200.0,
        },
    );
}

#[test]
fn complex_process_steady_state_allocates_nothing() {
    let params = four_level(1_944.0);
    let iq = transmit(&params, &draws(1_200, 4, 0x5eed));
    let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
    let mut soft = Vec::with_capacity(iq.len());
    demod.process(&iq, &mut soft);
    soft.clear();
    demod.process(&iq, &mut soft);
    soft.clear();
    sdrmm_modem_test_support::ber::perf::assert_no_alloc("CpmDemod::process", || {
        demod.process(&iq, &mut soft)
    });
    assert!(!soft.is_empty(), "the measured call recovered no symbols");
}

#[test]
fn real_process_steady_state_allocates_nothing() {
    let params = afsk_params();
    let audio = afsk_audio(&draws(300, 2, 0x0dd5));
    let mut demod = CpmDemod::real(
        &params,
        &afsk_rx(),
        TIMING_BW_BURST,
        RATE,
        RealDetector::ToneFilterbank {
            plus_hz: 2_200.0,
            minus_hz: 1_200.0,
        },
    );
    let mut soft = Vec::with_capacity(audio.len());
    demod.process_real(&audio, &mut soft);
    soft.clear();
    demod.process_real(&audio, &mut soft);
    soft.clear();
    sdrmm_modem_test_support::ber::perf::assert_no_alloc("CpmDemod::process_real", || {
        demod.process_real(&audio, &mut soft);
    });
    assert!(!soft.is_empty(), "the measured call recovered no symbols");
}

#[test]
fn a_noise_floor_that_rises_with_a_burst_does_not_hold_the_gate_open() {
    const ON: usize = 132;
    const OFF: usize = 156;
    let params = four_level(1_944.0);
    for amplitude in [3.0 * NOISE, 10.0 * NOISE, 30.0 * NOISE] {
        let mut keyed: Vec<Option<u8>> = Vec::new();
        let mut sent = Vec::new();
        for burst in 0..6u64 {
            let syms = draws(ON, 4, 23 + burst);
            keyed.extend(syms.iter().copied().map(Some));
            keyed.extend(vec![None; OFF]);
            sent.push(syms);
        }
        let mut iq = CpmMod::new(params.clone()).keyed(&keyed);
        let mut rng = Rng::new(7);
        for s in &mut iq {
            *s += Complex::new(
                (rng.uniform() as f32 * 2.0 - 1.0) * amplitude,
                (rng.uniform() as f32 * 2.0 - 1.0) * amplitude,
            );
        }
        let mut demod = CpmDemod::new(&params, &rx_rrc(), TIMING_BW_BURST);
        listening(&mut demod, 0x1157);
        let mut soft = Vec::new();
        demod.process(&iq, &mut soft);
        let got: Vec<u8> = soft.iter().map(|&s| params.mapping().slice(s)).collect();
        for (index, burst) in sent.iter().enumerate() {
            let errors = (0..got.len().saturating_sub(ON))
                .map(|lag| (8..ON).filter(|&i| got[lag + i] != burst[i]).count())
                .min()
                .unwrap_or(ON);
            assert_eq!(errors, 0, "noise {amplitude}: burst {index}");
        }
        let open =
            demod.settled().iter().filter(|&&s| s).count() as f32 / demod.settled().len() as f32;
        assert!(open < 0.5, "noise {amplitude}: the gate stayed open {open}");
    }
}
