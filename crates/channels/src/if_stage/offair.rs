use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::{Ddc, FracResampler};
use sdrmm_wire::{ChannelParams, IfFilter, IfNotch, WfmParams};

use super::IfStage;
use crate::{
    ChannelCtx, ChannelOutputs, ChannelRx, WfmChannel, channel_filter,
    testutil::{cf32_le, settings},
};

const WANTED: &[u8] = include_bytes!("../../../../fixtures/wfm_offair_99m9_240k.sigmf-data");
const NEIGHBOUR: &[u8] = include_bytes!("../../../../fixtures/wfm_offair_96m5_240k.sigmf-data");
const RATE: f64 = 240_000.0;
const MIX_RATE: f64 = 2.0 * RATE;
const TUNED_HZ: f64 = 99_900_000.0;
const BLOCK: usize = 4_096;
const SETTLE_S: f64 = 0.1;
const MAX_LAG: usize = 400;

fn wfm_mono() -> ChannelParams {
    ChannelParams::Wfm(WfmParams {
        stereo: false,
        ..WfmParams::default()
    })
}

fn upsampled(bytes: &[u8]) -> Vec<Complex<f32>> {
    let mut out = Vec::new();
    FracResampler::new(MIX_RATE / RATE).process(&cf32_le(bytes), &mut out);
    out
}

fn level(samples: &[Complex<f32>]) -> f64 {
    let power: f64 = samples.iter().map(|s| f64::from(s.norm_sqr())).sum();
    (power / samples.len() as f64).sqrt()
}

/// The wanted station with something else added at `offset_hz`, `ratio_db` above it.
fn band(extra: &[Complex<f32>], offset_hz: f64, ratio_db: f64) -> Vec<Complex<f32>> {
    let wanted = upsampled(WANTED);
    let scale = level(&wanted) / level(extra) * 10f64.powf(ratio_db / 20.0);
    wanted
        .iter()
        .zip(extra)
        .enumerate()
        .map(|(k, (&w, &x))| {
            let turn = Complex::from_polar(scale, TAU * offset_hz * k as f64 / MIX_RATE);
            let x = Complex::new(f64::from(x.re), f64::from(x.im)) * turn;
            w + Complex::new(x.re as f32, x.im as f32)
        })
        .collect()
}

fn carrier(len: usize) -> Vec<Complex<f32>> {
    vec![Complex::new(1.0, 0.0); len]
}

/// What a channel tuned to the wanted station hears, through the engine's own chain.
fn listen(band: &[Complex<f32>], filter: &IfFilter) -> Vec<f32> {
    let mut ddc = Ddc::new(MIX_RATE, RATE, 0.0).unwrap();
    let mut stage = IfStage::build(filter, RATE, TUNED_HZ, BLOCK).unwrap();
    let params = wfm_mono();
    let mut decoder_filter = channel_filter(&params).unwrap();
    let mut wfm = WfmChannel::new(ChannelCtx { input_rate: RATE }, settings(params)).unwrap();
    let (mut baseband, mut filtered) = (Vec::new(), Vec::new());
    let mut out = ChannelOutputs::default();
    let mut audio = Vec::new();
    for block in band.chunks(BLOCK) {
        ddc.process(block, &mut baseband);
        let shaped = match &mut stage {
            Some(stage) => stage.process(&baseband),
            None => &baseband,
        };
        decoder_filter.process(shaped, &mut filtered);
        out.reset();
        wfm.process(&filtered, &mut out);
        audio.extend_from_slice(&out.audio_pcm);
    }
    audio
}

/// How far `heard` is from `clean`, as the ratio of what they share to what they do not.
fn fidelity_db(clean: &[f32], heard: &[f32]) -> f64 {
    let settle = (SETTLE_S * f64::from(crate::AUDIO_RATE)) as usize;
    let span = clean.len().min(heard.len()) - settle - MAX_LAG;
    let reference = &clean[settle..settle + span];
    let energy = |x: &[f32]| x.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>();
    let best = (0..MAX_LAG)
        .map(|lag| {
            let shifted = &heard[settle + lag..settle + lag + span];
            let dot: f64 = reference
                .iter()
                .zip(shifted)
                .map(|(&a, &b)| f64::from(a) * f64::from(b))
                .sum();
            dot * dot / (energy(reference) * energy(shifted))
        })
        .fold(0.0, f64::max);
    10.0 * (best / (1.0 - best)).log10()
}

fn clean() -> Vec<f32> {
    listen(&upsampled(WANTED), &IfFilter::default())
}

fn cutting(low_hz: Option<f64>, high_hz: Option<f64>) -> IfFilter {
    IfFilter {
        enabled: true,
        low_hz,
        high_hz,
        notches: Vec::new(),
    }
}

#[test]
fn edges_cut_a_stronger_station_on_the_adjacent_channel() {
    let reference = clean();
    let crowded = band(&upsampled(NEIGHBOUR), 100_000.0, 6.0);
    let wide = fidelity_db(&reference, &listen(&crowded, &IfFilter::default()));
    let cut = fidelity_db(
        &reference,
        &listen(&crowded, &cutting(None, Some(40_000.0))),
    );
    eprintln!("adjacent +100 kHz, +6 dB: wide {wide:.1} dB, cut {cut:.1} dB");
    assert!(
        wide < 0.0,
        "the neighbour never captured the channel: {wide:.1} dB"
    );
    assert!(
        cut > 10.0 && cut > wide + 25.0,
        "edges left {cut:.1} dB, from {wide:.1} dB"
    );
}

#[test]
fn a_cut_on_a_clear_channel_keeps_the_station() {
    let reference = clean();
    let narrowed = listen(
        &upsampled(WANTED),
        &cutting(Some(-60_000.0), Some(60_000.0)),
    );
    let kept = fidelity_db(&reference, &narrowed);
    eprintln!("clear channel narrowed to ±60 kHz: {kept:.1} dB");
    assert!(kept > 25.0, "narrowing alone left {kept:.1} dB");
}

#[test]
fn a_notch_takes_a_carrier_off_the_station() {
    let reference = clean();
    let whistled = band(&carrier(upsampled(WANTED).len()), 30_000.0, 0.0);
    let open = fidelity_db(&reference, &listen(&whistled, &IfFilter::default()));
    let notched = IfFilter {
        enabled: true,
        notches: vec![IfNotch {
            frequency_hz: TUNED_HZ + 30_000.0,
            width_hz: 500.0,
        }],
        ..IfFilter::default()
    };
    let cleared = fidelity_db(&reference, &listen(&whistled, &notched));
    eprintln!("carrier at +30 kHz: open {open:.1} dB, notched {cleared:.1} dB");
    assert!(
        open < 0.0,
        "the carrier never covered the station: {open:.1} dB"
    );
    assert!(cleared > 30.0, "the notch left {cleared:.1} dB");
}
