use std::collections::BTreeSet;

use crate::channel::ChannelStatus;
use crate::present::FieldValue;
use crate::processor::beamformer::{BeamformerReading, LaneWeight};
use crate::processor::correlator::{Baseline, CorrelatorReading};
use crate::processor::df::{DfPeak, DfReading};
use crate::processor::polarimeter::PolarimeterReading;
use crate::processor::spatial::{SpatialPeak, SpatialReading};
use crate::processor::stitch::{StitchLane, StitchReading};
use crate::processor::{PROCESSOR_TYPE_IDS, ProcessorReading};
use crate::radar::{RadarTrack, RadarUpdate};

fn peak(relative_deg: f32) -> DfPeak {
    DfPeak {
        relative_deg,
        true_deg: Some(relative_deg + 10.0),
        confidence: 0.8,
        sigma_deg: 2.0,
        ..DfPeak::default()
    }
}

fn spatial(db: f32) -> SpatialPeak {
    SpatialPeak {
        freq_hz: 100e6,
        bearing_deg: 45.0,
        db,
        true_deg: None,
    }
}

fn busy() -> Vec<ProcessorReading> {
    vec![
        ProcessorReading::Df(DfReading {
            peaks: vec![peak(10.0), peak(20.0), peak(30.0)],
            snr_db: 12.0,
            fit: 0.9,
            sources: 2,
            ..DfReading::default()
        }),
        ProcessorReading::Beamformer(BeamformerReading {
            sinr_gain_db: Some(3.0),
            snr_db: Some(15.0),
            steer_deg: Some(90.0),
            cancelled_db: Some(20.0),
            weights: vec![LaneWeight::default(); 4],
            out_rate: 48_000.0,
            ..BeamformerReading::default()
        }),
        ProcessorReading::PassiveRadar(RadarUpdate {
            tracks: vec![RadarTrack::default(); 2],
            ..RadarUpdate::default()
        }),
        ProcessorReading::Stitch(StitchReading {
            lanes: vec![StitchLane::default(); 3],
            span_hz: 6e6,
            ..StitchReading::default()
        }),
        ProcessorReading::SpatialSpectrum(SpatialReading {
            peaks: (0..8).map(|db| spatial(db as f32)).collect(),
            ..SpatialReading::default()
        }),
        ProcessorReading::Correlator(CorrelatorReading {
            baselines: vec![Baseline::default(); 6],
            integrated_s: 1.0,
            ..CorrelatorReading::default()
        }),
        ProcessorReading::Polarimeter(PolarimeterReading {
            snr_db: Some(9.0),
            ..PolarimeterReading::default()
        }),
    ]
}

fn keys(status: &ChannelStatus) -> Vec<&str> {
    status
        .readouts
        .iter()
        .map(|field| field.key.as_str())
        .collect()
}

#[test]
fn every_processor_reports_readouts_with_unique_keys() {
    let readings = busy();
    assert_eq!(readings.len(), PROCESSOR_TYPE_IDS.len());
    for reading in readings {
        let status = reading.status();
        let keys = keys(&status);
        assert!(!keys.is_empty(), "{} has no readouts", reading.type_id());
        let unique: BTreeSet<&str> = keys.iter().copied().collect();
        assert_eq!(
            unique.len(),
            keys.len(),
            "{} repeats a key",
            reading.type_id()
        );
    }
}

#[test]
fn every_empty_reading_still_reports() {
    for type_id in PROCESSOR_TYPE_IDS {
        let reading = ProcessorReading::empty(type_id).expect("reading");
        let status = reading.status();
        let unique: BTreeSet<&str> = keys(&status).into_iter().collect();
        assert_eq!(
            unique.len(),
            status.readouts.len(),
            "{type_id} repeats a key"
        );
    }
}

#[test]
fn a_df_reading_reads_its_bearings_by_rank() {
    let status = busy()[0].status();
    assert_eq!(
        keys(&status),
        [
            "bearing", "relative", "sigma", "fit", "snr", "sources", "peak_2", "peak_3"
        ]
    );
    let bearing = status.readout("bearing").expect("bearing");
    assert!(
        matches!(bearing.value, FieldValue::Number { value, .. } if (value - 20.0).abs() < 1e-6)
    );
    assert_eq!(status.health.snr_db, Some(12.0));
}

#[test]
fn spatial_peaks_rank_by_power_and_stop_at_three() {
    let status = busy()[4].status();
    assert_eq!(keys(&status), ["peak", "peak_2", "peak_3"]);
}

#[test]
fn radar_counts_confirmed_tracks_and_reports_its_reference_lock() {
    let status = busy()[2].status();
    assert!(matches!(
        status.readout("targets").map(|field| &field.value),
        Some(FieldValue::Number { value, .. }) if (*value - 2.0).abs() < f64::EPSILON
    ));
    assert_eq!(status.health.lock, Some(false));
    assert!(status.readout("drops").is_none());
}

#[test]
fn a_fusion_reports_the_half_life_auto_found() {
    let state = crate::fusion::DfFusionState {
        samples: 4,
        half_life_s: 120,
        ..crate::fusion::DfFusionState::default()
    };
    let status = state.status();
    assert_eq!(keys(&status), ["bearings", "fade"]);
}
