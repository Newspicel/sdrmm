use num_complex::Complex;
use sdrmm_wire::{ChannelParams, ChannelSettings, DecoderEvent, SidParams, SidReport};

use super::{RATE, SidChannel, stations::STATIONS};
use crate::{ChannelCtx, ChannelOutputs, ChannelRx, synth::sid::carriers, testutil::settings};

const CENTRE_HZ: f64 = 30_000.0;

fn tuned(report_s: u16) -> ChannelSettings {
    ChannelSettings {
        frequency_hz: CENTRE_HZ,
        ..settings(ChannelParams::Sid(SidParams {
            report_s,
            threshold_db: 3.0,
        }))
    }
}

fn channel(report_s: u16) -> SidChannel {
    SidChannel::new(ChannelCtx { input_rate: RATE }, tuned(report_s)).expect("builds")
}

fn air(seconds: f64, tones: &[(f64, f32)], seed: u32) -> Vec<Complex<f32>> {
    carriers(tones, CENTRE_HZ, seconds, RATE, seed)
}

fn reports(channel: &mut SidChannel, iq: &[Complex<f32>]) -> Vec<SidReport> {
    let mut out = ChannelOutputs::default();
    let mut found = Vec::new();
    for block in iq.chunks(9_973) {
        out.reset();
        channel.process(block, &mut out);
        found.extend(out.events.drain(..).filter_map(|event| match event {
            DecoderEvent::Sid(report) => Some(report),
            _ => None,
        }));
    }
    found
}

fn station<'a>(report: &'a SidReport, call: &str) -> &'a sdrmm_wire::SidStationLevel {
    report
        .stations
        .iter()
        .find(|level| level.call == call)
        .expect("station in view")
}

#[test]
fn every_catalogued_station_in_the_window_is_reported() {
    let mut channel = channel(1);
    let report = reports(&mut channel, &air(1.2, &[(23_400.0, 0.1)], 1))
        .pop()
        .expect("a report");
    assert_eq!(report.window_ms, 1_000);
    assert_eq!(report.stations.len(), STATIONS.len());
    let dho = station(&report, "DHO38");
    assert!(dho.snr_db > 30.0, "{dho:?}");
    let naa = station(&report, "NAA");
    assert!(naa.snr_db.abs() < 3.0, "{naa:?}");
    assert!(report.floor_dbfs_hz < -60.0, "{report:?}");
}

#[test]
fn a_level_change_reads_in_decibels() {
    let mut channel = channel(1);
    let quiet = reports(&mut channel, &air(2.2, &[(24_000.0, 0.05)], 2));
    let loud = reports(&mut channel, &air(2.2, &[(24_000.0, 0.1)], 3));
    let before = station(quiet.last().unwrap(), "NAA").level_dbfs;
    let after = station(loud.last().unwrap(), "NAA").level_dbfs;
    assert!((after - before - 6.02).abs() < 0.3, "{before} -> {after}");
}

#[test]
fn a_flare_lifts_one_station_and_is_flagged() {
    let mut channel = channel(1);
    let calm = [(23_400.0, 0.05), (24_000.0, 0.05)];
    let flare = [(23_400.0, 0.1), (24_000.0, 0.05)];
    let mut iq = air(10.0, &calm, 4);
    iq.extend(air(35.0, &flare, 5));
    let last = reports(&mut channel, &iq).pop().expect("a report");
    let dho = station(&last, "DHO38");
    assert!(dho.disturbed, "{dho:?}");
    assert!(
        dho.deviation_db().is_some_and(|d| (d - 6.0).abs() < 0.5),
        "{dho:?}"
    );
    assert!(!station(&last, "NAA").disturbed);
    assert!(
        last.summary().starts_with("SID · DHO38"),
        "{}",
        last.summary()
    );
}

#[test]
fn the_lo_artifact_hides_the_station_it_lands_on() {
    let mut channel = channel(1);
    channel.lo_artifact_at(Some(23_400.0 - CENTRE_HZ));
    let report = reports(&mut channel, &air(1.2, &[], 6))
        .pop()
        .expect("a report");
    assert!(report.stations.iter().all(|level| level.call != "DHO38"));
    assert_eq!(report.stations.len(), STATIONS.len() - 1);
}

#[test]
fn settings_out_of_range_are_refused() {
    let ctx = ChannelCtx { input_rate: RATE };
    let bad = |report_s, threshold_db| {
        settings(ChannelParams::Sid(SidParams {
            report_s,
            threshold_db,
        }))
    };
    assert!(SidChannel::new(ctx, bad(0, 3.0)).is_err());
    assert!(SidChannel::new(ctx, bad(5, 0.5)).is_err());
    assert!(SidChannel::new(ctx, bad(5, f32::NAN)).is_err());
    assert!(
        SidChannel::new(
            ChannelCtx {
                input_rate: 8_000.0
            },
            bad(5, 3.0)
        )
        .is_err()
    );
    let mut channel = channel(5);
    assert!(channel.apply(tuned(61)).is_err());
    assert!(channel.apply(tuned(10)).is_ok());
}
