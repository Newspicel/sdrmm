use num_complex::Complex;
use sdrmm_dsp::Decibels;

use super::{
    super::{chip::ChipShaper, pn::PnSequence},
    *,
};
use crate::testutil::{add_noise, points};
use crate::{
    constellation::{demap, tables},
    linear::PhaseAnchor,
    soft::Llr,
};

const SPS: usize = 4;
const LEAD: usize = 53;
const PREAMBLE: usize = 32;

fn params(pn: PnSequence) -> DsssParams {
    DsssParams::new(pn, ChipShaper::root_raised_cosine(SPS, 0.35, 8), 8)
}

fn transmit(
    p: &DsssParams,
    preamble: &[Complex<f32>],
    payload: &[Complex<f32>],
) -> Vec<Complex<f32>> {
    let mut wave = vec![Complex::new(0.0, 0.0); LEAD];
    DsssMod::new(p.clone()).frame(preamble, payload, &mut wave);
    wave.resize(wave.len() + 128, Complex::new(0.0, 0.0));
    wave
}

fn identity(origin: usize, any: &[Complex<f32>]) -> Acquisition {
    Acquisition {
        origin,
        anchor: PhaseAnchor::fit_gain_only(any, any).unwrap(),
        noise_var: 1.0,
    }
}

#[test]
fn every_spreading_factor_round_trips_and_finds_its_own_origin() {
    let table = tables::qam_square(4).unwrap();
    for pn in [
        PnSequence::barker(11).unwrap(),
        PnSequence::barker(13).unwrap(),
        PnSequence::maximal_length(5).unwrap(),
        PnSequence::maximal_length(6).unwrap(),
    ] {
        let n = pn.len();
        let p = params(pn);
        let preamble = points(&table, PREAMBLE, 0x5eed);
        let payload = points(&table, 200, 0xbeef);
        let wave = transmit(&p, &preamble, &payload);
        let mut demod = DsssDemod::new(p);
        let acquisition = demod
            .acquire(&wave, &preamble, LEAD + 64)
            .unwrap_or_else(|| panic!("N = {n}: no acquisition"));
        assert_eq!(acquisition.origin, LEAD, "N = {n}");
        let mut got = Vec::new();
        demod.demodulate(PREAMBLE, payload.len(), &mut got);
        for (k, (&g, &s)) in got.iter().zip(&payload).enumerate() {
            assert!((g - s).norm() < 5e-3, "N = {n}, symbol {k}: {g} vs {s}");
        }
    }
}

#[test]
fn despreading_leaves_the_symbol_snr_unchanged_at_every_factor() {
    let table = tables::pam(2).unwrap();
    let noise_var = 0.25;
    for pn in [
        PnSequence::barker(11).unwrap(),
        PnSequence::maximal_length(5).unwrap(),
        PnSequence::maximal_length(7).unwrap(),
    ] {
        let n = pn.len();
        let p = params(pn);
        let preamble = points(&table, PREAMBLE, 0x0d55);
        let payload = points(&table, 4_000, 0x0d56);
        let mut wave = transmit(&p, &preamble, &payload);
        add_noise(&mut wave, 0xd555 + n as u64, noise_var);
        let mut demod = DsssDemod::new(p);
        demod.genie(&wave, identity(LEAD, &payload));
        let mut got = Vec::new();
        demod.demodulate(PREAMBLE, payload.len(), &mut got);
        let measured = demap::noise_var_from_known(&got, &payload);
        assert!(
            (measured / noise_var - 1.0).abs() < 0.05,
            "N = {n}: despread N0 {measured} vs chip-stream {noise_var}"
        );
    }
}

#[test]
fn the_correlator_rejects_a_narrowband_interferer_by_its_processing_gain() {
    const SYMBOL_SAMPLES: usize = 44;
    let table = tables::pam(2).unwrap();
    let symbols = 64;

    let collected = |pn: PnSequence, sps: usize| {
        let p = DsssParams::new(pn, ChipShaper::root_raised_cosine(sps, 0.35, 8), 4);
        assert_eq!(p.symbol_samples(), SYMBOL_SAMPLES);
        let preamble = points(&table, PREAMBLE, 0x1c1);
        let payload = vec![table.points()[0]; symbols];
        let clean = transmit(&p, &preamble, &payload);
        let mut demod = DsssDemod::new(p);
        let mut total = 0.0f64;
        let steps = 128;
        for step in 0..steps {
            let offset = (f64::from(step) / f64::from(steps) - 0.5) / sps as f64;
            let mut wave = clean.clone();
            for (index, s) in wave.iter_mut().enumerate() {
                let phase = std::f64::consts::TAU * offset * index as f64;
                *s += Complex::new((0.1 * phase.cos()) as f32, (0.1 * phase.sin()) as f32);
            }
            demod.genie(&wave, identity(LEAD, &payload));
            let mut got = Vec::new();
            demod.demodulate(PREAMBLE, symbols, &mut got);
            total += demap::noise_var_from_known(&got, &payload) / f64::from(steps);
        }
        total
    };

    let unspread = collected(PnSequence::from_chips(&[1]).unwrap(), SYMBOL_SAMPLES);
    for (pn, sps) in [
        (PnSequence::barker(4).unwrap(), 11),
        (PnSequence::barker(11).unwrap(), 4),
    ] {
        let n = pn.len();
        let expected = pn.processing_gain_db();
        let measured = (unspread / collected(pn, sps)).power_to_db();
        assert!(
            (measured - expected).abs() < 1.0,
            "N = {n}: measured gain {measured:.2} dB vs 10·log10(N) = {expected:.2} dB"
        );
    }
}

#[test]
fn the_two_committed_codes_differ_by_their_spreading_factor_ratio() {
    let table = tables::pam(2).unwrap();
    let symbols = 64;
    let collected = |pn: PnSequence| -> (f64, f64) {
        let p = params(pn);
        let preamble = points(&table, PREAMBLE, 0x1c2);
        let payload = vec![table.points()[0]; symbols];
        let clean = transmit(&p, &preamble, &payload);
        let carrier =
            clean.iter().map(|s| f64::from(s.norm_sqr())).sum::<f64>() / clean.len() as f64;
        let mut demod = DsssDemod::new(p);
        let mut total = 0.0f64;
        let steps = 128;
        for step in 0..steps {
            let offset = (f64::from(step) / f64::from(steps) - 0.5) / SPS as f64;
            let mut wave = clean.clone();
            for (index, s) in wave.iter_mut().enumerate() {
                let phase = std::f64::consts::TAU * offset * index as f64;
                *s += Complex::new((0.1 * phase.cos()) as f32, (0.1 * phase.sin()) as f32);
            }
            demod.genie(&wave, identity(LEAD, &payload));
            let mut got = Vec::new();
            demod.demodulate(PREAMBLE, symbols, &mut got);
            total += demap::noise_var_from_known(&got, &payload) / f64::from(steps);
        }
        (total, carrier)
    };
    let (barker, barker_carrier) = collected(PnSequence::barker(11).unwrap());
    let (m31, m31_carrier) = collected(PnSequence::maximal_length(5).unwrap());

    let absolute = (barker / m31).power_to_db();
    assert!(
        absolute.abs() < 0.5,
        "at equal absolute jammer power the two codes collect {absolute:.2} dB apart, where a \
         despreader's own chip band says they should collect the same"
    );
    let relative = ((barker * barker_carrier) / (m31 * m31_carrier)).power_to_db();
    let predicted = (31.0f64 / 11.0).power_to_db();
    assert!(
        (relative - predicted).abs() < 0.5,
        "at equal C/I the length-31 code collects {relative:.2} dB less interference than \
         Barker-11, where 10·log10(31/11) predicts {predicted:.2} dB"
    );
}

#[test]
fn a_symbol_past_the_filtered_tail_is_not_despread() {
    let table = tables::qam_square(4).unwrap();
    let p = params(PnSequence::barker(11).unwrap());
    let preamble = points(&table, PREAMBLE, 0x0b0c);
    let payload = points(&table, 64, 0x0b0d);
    let wave = transmit(&p, &preamble, &payload);
    let mut demod = DsssDemod::new(p);
    demod.genie(&wave, identity(LEAD, &payload));
    assert!(demod.despread(LEAD, 0).is_some());
    assert_eq!(demod.despread(LEAD, 100_000), None);
    let mut got = Vec::new();
    demod.demodulate(PREAMBLE, 100_000, &mut got);
    assert!(got.len() < 100_000, "a short capture decided every symbol");
}

#[test]
fn the_anchor_removes_a_rotation() {
    let table = tables::qam_square(4).unwrap();
    let p = params(PnSequence::barker(11).unwrap());
    let preamble = points(&table, PREAMBLE, 0x0a0c);
    let payload = points(&table, 2_048, 0x0a0d);
    let mut wave = transmit(&p, &preamble, &payload);
    let rotation = 0.9f64;
    let gain = 0.35f64;
    let (sin, cos) = rotation.sin_cos();
    for s in wave.iter_mut() {
        *s = Complex::new(
            (gain * (f64::from(s.re) * cos - f64::from(s.im) * sin)) as f32,
            (gain * (f64::from(s.re) * sin + f64::from(s.im) * cos)) as f32,
        );
    }
    add_noise(&mut wave, 0x0a0e, 0.002);
    let mut demod = DsssDemod::new(p);
    let acquisition = demod.acquire(&wave, &preamble, LEAD + 64).unwrap();
    assert_eq!(acquisition.origin, LEAD);
    assert!((acquisition.anchor.gain.norm() - gain).abs() < 0.05 * gain);
    assert!((acquisition.anchor.gain.arg() - rotation).abs() < 0.05);
    let mut got = Vec::new();
    demod.demodulate(PREAMBLE, payload.len(), &mut got);
    let errors = got
        .iter()
        .zip(&payload)
        .filter(|&(&g, &s)| table.hard_slice(g) != table.hard_slice(s))
        .count();
    assert_eq!(errors, 0, "{errors} of {} symbols", payload.len());
}

#[test]
fn fitting_the_anchor_slope_costs_more_phase_than_it_removes() {
    let table = tables::qam_square(4).unwrap();
    let p = params(PnSequence::barker(11).unwrap());
    let preamble = points(&table, PREAMBLE, 0x51_0e);
    let payload = points(&table, 2_048, 0x51_0f);
    let mut wave = transmit(&p, &preamble, &payload);
    add_noise(&mut wave, 0x51_10, 0.25);
    let mut demod = DsssDemod::new(p);
    demod.acquire(&wave, &preamble, LEAD + 64).unwrap();

    let despread: Vec<Complex<f32>> = (0..preamble.len())
        .map(|k| demod.despread(LEAD, k).unwrap())
        .collect();
    let indices: Vec<usize> = (0..preamble.len()).collect();
    let sloped = PhaseAnchor::fit(&indices, &despread, &preamble).unwrap();

    let errors_with = |anchor: PhaseAnchor| {
        (0..payload.len())
            .filter(|&k| {
                let index = PREAMBLE + k;
                let raw = demod.despread(LEAD, index).unwrap();
                let corrected = anchor.correct(index, raw);
                table.hard_slice(corrected) != table.hard_slice(payload[k])
            })
            .count()
    };
    let gain_only = errors_with(demod.acquisition().unwrap().anchor);
    let with_slope = errors_with(sloped);
    assert!(
        gain_only < payload.len() / 12,
        "the committed correction lost {gain_only} of {}, past the channel's own floor",
        payload.len()
    );
    assert!(
        with_slope > payload.len() / 4,
        "the fitted slope lost only {with_slope} of {} symbols against the constant gain's \
         {gain_only}; if extrapolation has stopped hurting, the entry should fit it",
        payload.len()
    );
}

#[test]
fn a_demodulator_with_no_acquisition_produces_no_symbols() {
    let p = params(PnSequence::barker(11).unwrap());
    let demod = DsssDemod::new(p);
    let mut out = Vec::new();
    demod.demodulate(PREAMBLE, 100, &mut out);
    assert!(out.is_empty());
}

#[test]
fn the_framing_overhead_is_the_symbol_count_ratio() {
    let overhead = DsssParams::framing_overhead_db(32, 2_048);
    assert!((overhead - (2080.0f64 / 2048.0).power_to_db()).abs() < 1e-12);
    assert!((overhead - 0.0673).abs() < 1e-3, "{overhead}");
    assert!(DsssParams::framing_overhead_db(0, 100).abs() < 1e-12);
    assert!((DsssParams::framing_overhead_db(100, 100) - 3.0103).abs() < 1e-3);
}

#[test]
fn llr_magnitudes_predict_their_own_error_rate() {
    let table = tables::qam_square(4).unwrap();
    let p = params(PnSequence::barker(11).unwrap());
    let preamble = points(&table, 128, 0x11c0);
    let payload = points(&table, 20_000, 0x11c1);
    let mut wave = transmit(&p, &preamble, &payload);
    add_noise(&mut wave, 0x11c2, 0.6);
    let mut demod = DsssDemod::new(p);
    demod.acquire(&wave, &preamble, LEAD + 64).unwrap();
    let mut got = Vec::new();
    demod.demodulate(preamble.len(), payload.len(), &mut got);
    let bits = table.bits_per_symbol();
    let mut llrs = vec![Llr(0.0); got.len() * bits];
    demod.llrs(&got, &table, &mut llrs);

    let mut bands = [(0u32, 0u32); 3];
    for (i, &llr) in llrs.iter().enumerate() {
        let label = table.hard_slice(payload[i / bits]);
        let sent = (label >> (i % bits)) & 1 == 1;
        let band = match llr.0.abs() {
            x if x < 1.0 => 0,
            x if x < 3.0 => 1,
            _ => 2,
        };
        bands[band].0 += 1;
        if llr.bit() != sent {
            bands[band].1 += 1;
        }
    }
    for (band, &(count, wrong)) in bands.iter().enumerate() {
        assert!(count > 300, "band {band} saw only {count} bits");
        let measured = f64::from(wrong) / f64::from(count);
        let predicted = 1.0 / (1.0 + [0.5f64, 2.0, 4.0][band].exp());
        assert!(
            measured < predicted * 3.0 + 0.02,
            "band {band}: measured {measured}, predicted {predicted}"
        );
    }
}
