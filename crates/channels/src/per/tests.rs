use num_complex::Complex;
use sdrmm_wire::{ChannelParams, ChannelSettings, DecoderEvent, PerBaud, PerParams, PerReport};

use super::{PerChannel, RATE, channel_filter};
use crate::{
    ChannelCtx, ChannelOutputs, ChannelRx,
    synth::{add_noise, per::frames, silence},
    testutil::settings,
};

fn tuned(params: PerParams) -> ChannelSettings {
    settings(ChannelParams::Per(params))
}

fn channel(params: PerParams) -> PerChannel {
    PerChannel::new(ChannelCtx { input_rate: RATE }, tuned(params)).expect("builds")
}

fn run(channel: &mut PerChannel, params: &PerParams, iq: &[Complex<f32>]) -> Vec<PerReport> {
    let mut filter = channel_filter(params);
    let mut filtered = Vec::new();
    let mut out = ChannelOutputs::default();
    let mut found = Vec::new();
    let mut padded = iq.to_vec();
    padded.extend(silence(RATE as usize));
    for block in padded.chunks(4_801) {
        filter.process(block, &mut filtered);
        out.reset();
        channel.process(&filtered, &mut out);
        found.extend(out.events.drain(..).filter_map(|event| match event {
            DecoderEvent::Per(report) => Some(report),
            _ => None,
        }));
    }
    found
}

#[test]
fn a_clean_stream_has_no_errors_at_every_rate() {
    for baud in PerBaud::ALL {
        let params = PerParams {
            baud,
            payload_bytes: 16,
        };
        let mut channel = channel(params);
        let iq = frames(&params, 0..20, RATE);
        let report = run(&mut channel, &params, &iq).pop().expect("a report");
        assert_eq!(report.received, 20, "{baud:?} {report:?}");
        assert_eq!(report.per, 0.0, "{baud:?}");
        assert_eq!(report.ber, Some(0.0), "{baud:?}");
        assert_eq!(report.last_sequence, Some(19));
    }
}

#[test]
fn skipped_frames_are_missed_packets() {
    let params = PerParams::default();
    let mut channel = channel(params);
    let sent = (0..30).filter(|sequence| sequence % 10 != 5);
    let report = run(&mut channel, &params, &frames(&params, sent, RATE))
        .pop()
        .expect("a report");
    assert_eq!(report.received, 27);
    assert_eq!(report.missed, 3);
    assert_eq!(report.expected, 30);
    assert_eq!(channel.rejected(), 0);
}

#[test]
fn a_spectrally_inverted_stream_still_decodes() {
    let params = PerParams::default();
    let mut channel = channel(params);
    let iq: Vec<_> = frames(&params, 0..10, RATE)
        .into_iter()
        .map(|s| s.conj())
        .collect();
    let report = run(&mut channel, &params, &iq).pop().expect("a report");
    assert_eq!(report.received, 10);
}

#[test]
fn noise_raises_the_bit_error_rate_and_surfaces_rejects() {
    let params = PerParams {
        baud: PerBaud::Baud9600,
        payload_bytes: 64,
    };
    let mut channel = channel(params);
    let mut iq = frames(&params, 0..200, RATE);
    add_noise(&mut iq, 9, 1.1);
    let report = run(&mut channel, &params, &iq).pop().expect("a report");
    assert!(report.bit_errors > 0, "{report:?}");
    assert!(report.per > 0.0, "{report:?}");
    assert!(report.received > 0, "{report:?}");
    assert_eq!(u64::from(channel.rejected()), report.errored);
}

#[test]
fn reports_only_follow_new_frames() {
    let params = PerParams::default();
    let mut channel = channel(params);
    let mut iq = frames(&params, 0..5, RATE);
    iq.extend(silence(3 * RATE as usize));
    assert_eq!(run(&mut channel, &params, &iq).len(), 1);
}

#[test]
fn a_new_setting_starts_a_new_count() {
    let params = PerParams::default();
    let mut channel = channel(params);
    run(&mut channel, &params, &frames(&params, 0..5, RATE));
    let longer = PerParams {
        payload_bytes: 48,
        ..params
    };
    channel.apply(tuned(longer)).expect("applies");
    let report = run(&mut channel, &longer, &frames(&longer, 100..103, RATE))
        .pop()
        .expect("a report");
    assert_eq!(report.received, 3);
    assert_eq!(report.expected, 3);
}

#[test]
fn settings_out_of_range_are_refused() {
    let ctx = ChannelCtx { input_rate: RATE };
    let empty = PerParams {
        payload_bytes: 0,
        ..PerParams::default()
    };
    assert!(PerChannel::new(ctx, tuned(empty)).is_err());
    assert!(
        PerChannel::new(
            ChannelCtx {
                input_rate: 48_000.0
            },
            tuned(PerParams::default())
        )
        .is_err()
    );
    let mut channel = channel(PerParams::default());
    assert!(channel.apply(tuned(empty)).is_err());
}
