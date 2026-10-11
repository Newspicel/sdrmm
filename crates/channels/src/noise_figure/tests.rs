use num_complex::Complex;
use sdrmm_wire::{
    ChannelParams, ChannelSettings, DecoderEvent, NoiseFigureParams, NoiseFigureReport,
};

use super::{NoiseFigureChannel, RATE, levels::Pair, noise_factor};
use crate::{
    ChannelCtx, ChannelOutputs, ChannelRx,
    synth::noise_figure::{self, switched},
    testutil::settings,
};

fn tuned(enr_db: f32) -> ChannelSettings {
    settings(ChannelParams::NoiseFigure(NoiseFigureParams { enr_db }))
}

fn channel(enr_db: f32) -> NoiseFigureChannel {
    NoiseFigureChannel::new(ChannelCtx { input_rate: RATE }, tuned(enr_db)).expect("builds")
}

fn noise(seconds: f64, amp: f32, seed: u32) -> Vec<Complex<f32>> {
    noise_figure::noise(seconds, amp, RATE, seed)
}

fn reports(channel: &mut NoiseFigureChannel, iq: &[Complex<f32>]) -> Vec<NoiseFigureReport> {
    let mut out = ChannelOutputs::default();
    let mut found = Vec::new();
    for block in iq.chunks(7_777) {
        out.reset();
        channel.process(block, &mut out);
        found.extend(out.events.drain(..).filter_map(|event| match event {
            DecoderEvent::NoiseFigure(report) => Some(report),
            _ => None,
        }));
    }
    found
}

fn expected_nf_db(y_db: f64, enr_db: f64) -> f64 {
    let y = 10f64.powf(y_db / 10.0);
    10.0 * (10f64.powf(enr_db / 10.0) / (y - 1.0)).log10()
}

#[test]
fn a_switched_source_reads_the_noise_figure() {
    let mut channel = channel(15.0);
    let hot_amp = 10f32.sqrt();
    let mut iq = noise(3.0, 1.0, 1);
    iq.extend(noise(3.0, hot_amp, 2));
    let all = reports(&mut channel, &iq);
    let last = all.last().expect("a report");
    let y = last.y_db.expect("Y");
    assert!((y - 10.0).abs() < 0.1, "{last:?}");
    let nf = last.nf_db.expect("NF");
    assert!(
        (f64::from(nf) - expected_nf_db(10.0, 15.0)).abs() < 0.15,
        "{last:?}"
    );
    assert_eq!(last.source_on, Some(true));
    assert!(last.temperature_k.is_some_and(|k| k > 0.0));
    assert!(all[0].nf_db.is_none());
}

#[test]
fn switching_back_keeps_the_measurement_and_tracks_the_source() {
    let mut channel = channel(15.0);
    let hot_amp = 10f32.powf(0.6);
    let mut iq = noise(2.0, 1.0, 3);
    iq.extend(noise(2.0, hot_amp, 4));
    iq.extend(noise(2.0, 1.0, 5));
    let last = reports(&mut channel, &iq).pop().expect("a report");
    assert_eq!(last.source_on, Some(false));
    assert!((last.y_db.unwrap() - 12.0).abs() < 0.1, "{last:?}");
}

#[test]
fn a_steady_level_asks_for_the_source_to_be_switched() {
    let mut channel = channel(15.0);
    let last = reports(&mut channel, &noise(3.0, 1.0, 6))
        .pop()
        .expect("a report");
    assert_eq!(last.nf_db, None);
    assert!(last.settled_s > 2.5, "{last:?}");
    assert!(last.summary().contains("toggle"), "{}", last.summary());
}

#[test]
fn the_lo_artifact_is_left_out_of_the_measurement() {
    let mut channel = channel(15.0);
    channel.lo_artifact_at(Some(0.0));
    let carrier = |len: usize, iq: &mut [Complex<f32>]| {
        for sample in iq.iter_mut().take(len) {
            *sample += Complex::new(30.0, 0.0);
        }
    };
    let mut cold = noise(2.0, 1.0, 7);
    let len = cold.len();
    carrier(len, &mut cold);
    let mut hot = noise(2.0, 10f32.sqrt(), 8);
    carrier(len, &mut hot);
    cold.extend(hot);
    let last = reports(&mut channel, &cold).pop().expect("a report");
    assert!((last.y_db.unwrap() - 10.0).abs() < 0.1, "{last:?}");
}

#[test]
fn the_switched_source_signal_reads_its_y_factor() {
    let mut channel = channel(15.0);
    let mut iq = switched(8.0, 1.5, RATE);
    iq.extend(switched(8.0, 1.5, RATE));
    let last = reports(&mut channel, &iq).pop().expect("a report");
    assert!((last.y_db.unwrap() - 8.0).abs() < 0.1, "{last:?}");
}

#[test]
fn a_y_factor_beyond_the_enr_has_no_noise_figure() {
    let pair = Pair {
        hot: 100.0,
        cold: 1.0,
    };
    assert_eq!(noise_factor(pair, 15.0), None);
    assert!(noise_factor(pair, 25.0).is_some());
    assert_eq!(
        noise_factor(
            Pair {
                hot: 1.0,
                cold: 1.0
            },
            15.0
        ),
        None
    );
}

#[test]
fn settings_out_of_range_are_refused() {
    assert!(NoiseFigureChannel::new(ChannelCtx { input_rate: RATE }, tuned(0.0)).is_err());
    assert!(NoiseFigureChannel::new(ChannelCtx { input_rate: RATE }, tuned(f32::NAN)).is_err());
    assert!(
        NoiseFigureChannel::new(
            ChannelCtx {
                input_rate: 48_000.0
            },
            tuned(15.0)
        )
        .is_err()
    );
    let mut channel = channel(15.0);
    assert!(channel.apply(tuned(99.0)).is_err());
    assert!(channel.apply(tuned(5.0)).is_ok());
}
