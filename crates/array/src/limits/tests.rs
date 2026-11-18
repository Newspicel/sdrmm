use crate::Settings;
use crate::beamformer::{BeamformerParams, SteerSource};
use crate::correlator::CorrelatorParams;
use crate::df::DfParams;
use crate::passive_radar::{CfarKind, Illuminator, PassiveRadarParams, ReferenceCleaning};
use crate::polarimeter::PolarimeterParams;
use crate::spatial_spectrum::SpatialSpectrumParams;

fn radar(edit: impl FnOnce(&mut PassiveRadarParams)) -> Option<&'static str> {
    let mut params = PassiveRadarParams::default();
    edit(&mut params);
    params.problem()
}

fn checked<T: Default>(
    edit: impl FnOnce(&mut T),
    problem: fn(&T) -> Option<&'static str>,
) -> Option<&'static str> {
    let mut params = T::default();
    edit(&mut params);
    problem(&params)
}

fn df(edit: impl FnOnce(&mut DfParams)) -> Option<&'static str> {
    checked(edit, DfParams::problem)
}

fn beam(edit: impl FnOnce(&mut BeamformerParams)) -> Option<&'static str> {
    checked(edit, BeamformerParams::problem)
}

fn spatial(edit: impl FnOnce(&mut SpatialSpectrumParams)) -> Option<&'static str> {
    checked(edit, SpatialSpectrumParams::problem)
}

fn correlator(edit: impl FnOnce(&mut CorrelatorParams)) -> Option<&'static str> {
    checked(edit, CorrelatorParams::problem)
}

fn polarimeter(edit: impl FnOnce(&mut PolarimeterParams)) -> Option<&'static str> {
    checked(edit, PolarimeterParams::problem)
}

#[test]
fn radar_validation_follows_the_limits() {
    let limits = crate::passive_radar::LIMITS;
    assert_eq!(radar(|p| p.cpi_ms = limits.cpi_ms.max), None);
    assert!(radar(|p| p.cpi_ms = limits.cpi_ms.max + 1).is_some());
    assert!(radar(|p| p.cpi_ms = limits.cpi_ms.min - 1).is_some());
    assert_eq!(radar(|p| p.overlap = limits.overlap.max), None);
    assert!(radar(|p| p.max_range_km = limits.max_range_km.min).is_some());
    assert_eq!(radar(|p| p.max_range_km = limits.max_range_km.max), None);
    assert_eq!(radar(|p| p.offset_hz = limits.offset_hz.min), None);
    assert!(radar(|p| p.clutter.step = limits.clutter_step.min).is_some());
    assert_eq!(radar(|p| p.clutter.reach_km = limits.reach_km.min), None);
    assert!(radar(|p| p.clutter.lead = limits.lead.max + 1).is_some());
    assert_eq!(radar(|p| p.cfar.train_range = limits.train_range.max), None);
    assert!(radar(|p| p.cfar.guard_doppler = limits.guard.max + 1).is_some());
    assert!(radar(|p| p.tracker.jerk = limits.jerk.min).is_some());
    assert_eq!(radar(|p| p.tracker.gate = limits.gate.max), None);
    assert!(radar(|p| p.tracker.coast_looks = limits.coast_looks.max + 1).is_some());
    assert!(
        radar(|p| {
            p.tracker.confirm_hits = limits.track_window.max;
            p.tracker.confirm_window = limits.track_window.max + 1;
        })
        .is_some()
    );
    assert!(radar(|p| p.assumed_altitude_m = limits.altitude_m.max + 1.0).is_some());
}

#[test]
fn radar_seeds_are_valid_settings() {
    let seed = crate::passive_radar::LIMITS.seed;
    assert_eq!(
        radar(|p| {
            p.illuminator = Illuminator::DvbtPartial {
                bandwidth_hz: seed.dvbt_bandwidth_hz,
            };
            p.reference = ReferenceCleaning::Off;
        }),
        None
    );
    assert_eq!(
        radar(|p| p.illuminator = Illuminator::Custom {
            bandwidth_hz: seed.custom_bandwidth_hz
        }),
        None
    );
    assert_eq!(
        radar(|p| p.reference = ReferenceCleaning::Cma {
            taps: seed.cma_taps,
            step: seed.cma_step
        }),
        None
    );
    assert_eq!(
        radar(|p| p.cfar.kind = CfarKind::Os { rank: seed.os_rank }),
        None
    );
    assert_eq!(
        PassiveRadarParams::default().reference,
        ReferenceCleaning::Cma {
            taps: seed.cma_taps,
            step: seed.cma_step
        }
    );
}

#[test]
fn df_validation_follows_the_limits() {
    let limits = crate::df::LIMITS;
    assert_eq!(df(|p| p.sources = Some(limits.sources.max)), None);
    assert!(df(|p| p.sources = Some(limits.sources.max + 1)).is_some());
    assert!(df(|p| p.max_peaks = limits.peaks.min - 1).is_some());
    assert_eq!(df(|p| p.max_peaks = limits.peaks.max), None);
    assert_eq!(df(|p| p.smoothing = limits.smoothing.max), None);
    assert!(df(|p| p.smoothing = limits.smoothing.max + 1).is_some());
    assert_eq!(df(|p| p.offset_hz = limits.band.offset_hz.min), None);
    assert!(df(|p| p.offset_hz = limits.band.offset_hz.max * 2.0).is_some());
    assert_eq!(df(|p| p.bandwidth_hz = limits.band.bandwidth_hz.min), None);
    assert!(df(|p| p.bandwidth_hz = limits.band.bandwidth_hz.min - 1.0).is_some());
    assert_eq!(df(|p| p.report_ms = limits.report_ms.min), None);
    assert!(df(|p| p.report_ms = limits.report_ms.max + 1).is_some());
    assert_eq!(df(|p| p.carry_over = limits.carry_over.max), None);
    assert!(df(|p| p.squelch_db = limits.squelch_db.max + 0.5).is_some());
    assert_eq!(df(|p| p.loading = limits.loading.max), None);
    assert!(df(|p| p.azimuth_step_deg = limits.azimuth_step_deg.min / 2.0).is_some());
    assert_eq!(df(|p| p.yaw_gate_dps = limits.yaw_gate_dps.max), None);
    assert!(df(|p| p.yaw_gate_dps = limits.yaw_gate_dps.min - 0.5).is_some());
    assert_eq!(
        df(|p| p.station_id = Some("s".repeat(limits.station_len))),
        None
    );
    assert!(df(|p| p.station_id = Some("s".repeat(limits.station_len + 1))).is_some());
}

#[test]
fn beamformer_validation_follows_the_limits() {
    let limits = crate::beamformer::LIMITS;
    assert_eq!(beam(|p| p.nulls_deg = vec![10.0; limits.nulls]), None);
    assert!(beam(|p| p.nulls_deg = vec![10.0; limits.nulls + 1]).is_some());
    assert_eq!(beam(|p| p.taps = limits.taps.max), None);
    assert!(beam(|p| p.taps = limits.taps.max + 1).is_some());
    assert_eq!(beam(|p| p.step = limits.step.min), None);
    assert!(beam(|p| p.step = limits.step.max + 0.1).is_some());
    assert_eq!(beam(|p| p.forget = limits.forget.max), None);
    assert!(beam(|p| p.crossfade_ms = limits.crossfade_ms.max + 1).is_some());
    assert!(beam(|p| p.update_ms = limits.update_ms.min - 1).is_some());
    assert_eq!(beam(|p| p.carry_over = limits.carry_over.max), None);
    assert_eq!(beam(|p| p.loading = limits.loading.max), None);
    assert!(beam(|p| p.steer_timeout_ms = limits.steer_timeout_ms.max + 1).is_some());
    assert_eq!(
        beam(|p| p.bandwidth_hz = Some(limits.band.bandwidth_hz.min)),
        None
    );
    assert!(beam(|p| p.bandwidth_hz = Some(limits.band.bandwidth_hz.min - 1.0)).is_some());
    assert_eq!(
        beam(|p| p.bandwidth_hz = Some(sdrmm_wire::limits::LIMITS.band_seed_hz)),
        None
    );
    assert_eq!(
        BeamformerParams::default().bandwidth_hz,
        Some(sdrmm_wire::limits::LIMITS.band_seed_hz)
    );
    let steer = |elevation_deg| SteerSource::Fixed {
        azimuth_deg: 0.0,
        elevation_deg,
    };
    assert_eq!(beam(|p| p.steer = steer(limits.elevation_deg.max)), None);
    assert!(beam(|p| p.steer = steer(limits.elevation_deg.max + 1.0)).is_some());
}

#[test]
fn spatial_validation_follows_the_limits() {
    let limits = crate::spatial_spectrum::LIMITS;
    for bins in [limits.bins.min, limits.bins.max] {
        assert_eq!(spatial(|p| p.bins = bins), None);
    }
    assert!(spatial(|p| p.bins = limits.bins.max * 2).is_some());
    assert_eq!(spatial(|p| p.columns = limits.columns.min), None);
    assert!(spatial(|p| p.columns = limits.columns.min / 2).is_some());
    assert_eq!(
        spatial(|p| {
            p.bins = limits.bins.max;
            p.columns = limits.columns.max;
        }),
        None
    );
    assert!(spatial(|p| p.average_ms = limits.average_ms.max + 1).is_some());
    assert_eq!(spatial(|p| p.report_ms = limits.report_ms.max), None);
    assert!(spatial(|p| p.report_ms = limits.report_ms.max + 1).is_some());
    for step in [limits.azimuth_step_deg.min, limits.azimuth_step_deg.max] {
        assert_eq!(spatial(|p| p.azimuth_step_deg = step), None);
    }
    assert!(spatial(|p| p.azimuth_step_deg = limits.azimuth_step_deg.max + 2.0).is_some());
    assert!(spatial(|p| p.span_db = limits.span_db.min - 1.0).is_some());
    assert_eq!(
        spatial(|p| p.bandwidth_hz = Some(sdrmm_wire::limits::LIMITS.band_seed_hz)),
        None
    );
    assert!(spatial(|p| p.offset_hz = limits.band.offset_hz.min - 1.0).is_some());
}

#[test]
fn correlator_validation_follows_the_limits() {
    let limits = crate::correlator::LIMITS;
    for bins in [limits.bins.min, limits.bins.max] {
        assert_eq!(
            correlator(|p| {
                p.bins = bins;
                p.channels = limits.channels.min;
            }),
            None
        );
    }
    assert!(correlator(|p| p.bins = limits.bins.min / 2).is_some());
    assert_eq!(correlator(|p| p.channels = limits.channels.max), None);
    assert!(correlator(|p| p.channels = limits.channels.min / 2).is_some());
    assert_eq!(correlator(|p| p.integrate_s = limits.integrate_s.min), None);
    assert!(correlator(|p| p.integrate_s = limits.integrate_s.max + 1.0).is_some());
    assert_eq!(
        correlator(|p| p.bandwidth_hz = Some(sdrmm_wire::limits::LIMITS.band_seed_hz)),
        None
    );
    assert!(correlator(|p| p.bandwidth_hz = Some(limits.band.bandwidth_hz.max + 1.0)).is_some());
}

#[test]
fn polarimeter_validation_follows_the_limits() {
    let limits = crate::polarimeter::LIMITS;
    assert_eq!(
        polarimeter(|p| p.bandwidth_hz = limits.band.bandwidth_hz.min),
        None
    );
    assert!(polarimeter(|p| p.bandwidth_hz = limits.band.bandwidth_hz.min - 1.0).is_some());
    assert_eq!(
        polarimeter(|p| p.offset_hz = limits.band.offset_hz.max),
        None
    );
    assert!(polarimeter(|p| p.report_ms = limits.report_ms.min - 1).is_some());
    assert_eq!(polarimeter(|p| p.average_ms = limits.average_ms.max), None);
    assert!(polarimeter(|p| p.crossfade_ms = limits.crossfade_ms.max + 1).is_some());
}
