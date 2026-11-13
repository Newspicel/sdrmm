use super::*;
use crate::processor::correlator::Baseline;
use crate::processor::df::DfPeak;
use crate::radar::{RadarAxes, RadarTrack, TrackMatch};

#[test]
fn a_bearing_shows_its_peaks_and_their_mirrors() {
    let reading = DfReading {
        peaks: vec![
            DfPeak {
                relative_deg: 30.0,
                true_deg: Some(120.0),
                sigma_deg: 4.0,
                mirror_deg: Some(150.0),
                ..DfPeak::default()
            },
            DfPeak {
                relative_deg: 200.0,
                ..DfPeak::default()
            },
        ],
        azimuth_deg: Some(90.0),
        ..DfReading::default()
    };
    let bearing = reading.view().bearing.expect("a rose");
    let weights: Vec<NeedleWeight> = bearing.needles.iter().map(|n| n.weight).collect();
    assert_eq!(
        weights,
        [
            NeedleWeight::Primary,
            NeedleWeight::Mirror,
            NeedleWeight::Secondary
        ]
    );
    assert_eq!(bearing.sigma_deg, Some(4.0));
    assert_eq!(bearing.heading_deg, Some(90.0));
}

#[test]
fn a_reading_without_heading_says_so() {
    let labels: Vec<String> = DfReading::default()
        .view()
        .flags
        .into_iter()
        .map(|flag| flag.label)
        .collect();
    assert!(labels.contains(&"No heading".to_owned()));
}

#[test]
fn a_null_beside_the_steer_is_flagged() {
    let reading = BeamformerReading {
        steer_deg: Some(359.0),
        nulls_deg: vec![4.0],
        ..BeamformerReading::default()
    };
    assert!(
        reading
            .view()
            .flags
            .iter()
            .any(|flag| flag.label == "Null close")
    );
}

#[test]
fn radar_tracks_rank_by_strength_and_tie_to_their_marks() {
    let track = |id, snr_db| RadarTrack {
        id,
        snr_db,
        truth: (id == 2).then(|| TrackMatch {
            id: "abc".into(),
            callsign: Some("DLH1".into()),
        }),
        ..RadarTrack::default()
    };
    let update = RadarUpdate {
        tracks: (1..=8).map(|id| track(id, id as f32)).collect(),
        axes: RadarAxes {
            carrier_hz: 98.1e6,
            ..RadarAxes::default()
        },
        ..RadarUpdate::default()
    };
    let view = update.view();
    let table = view.table.expect("a table");
    let keys: Vec<u32> = table.rows.iter().map(|row| row.key).collect();
    assert_eq!(keys, [8, 7, 6, 5, 4, 3]);
    let marks = view.surface.expect("a surface").marks;
    assert!(marks.iter().any(|mark| mark.key == Some(8)));
    assert_eq!(
        view.headline.map(|line| line.text),
        Some("98.10 MHz".into())
    );
}

#[test]
fn a_radar_problem_heads_the_face_unless_it_is_only_a_chip() {
    let update = RadarUpdate {
        problems: vec![RadarProblem::TableOutOfRange, RadarProblem::NoTransmitter],
        ..RadarUpdate::default()
    };
    let view = update.view();
    assert_eq!(
        view.headline,
        Some(Headline {
            text: "No transmitter".into(),
            warn: true
        })
    );
    assert_eq!(view.flags.len(), 1);
}

#[test]
fn a_baseline_is_a_layer_named_by_its_pair() {
    let reading = CorrelatorReading {
        baselines: vec![Baseline {
            a: 0,
            b: 2,
            coherence: 0.5,
            phase_deg: -45.0,
            ..Baseline::default()
        }],
        ..CorrelatorReading::default()
    };
    let layers = reading.view().surface.expect("a surface").layers;
    assert_eq!(layers[0].label, "1-3");
    assert_eq!(layers[0].phase_deg, Some(-45.0));
}

#[test]
fn a_stitched_lane_signs_its_equalisation() {
    let reading = StitchReading {
        lanes: vec![StitchLane {
            lane: 1,
            center_hz: 100e6,
            noise_eq_db: 1.34,
            ..StitchLane::default()
        }],
        ..StitchReading::default()
    };
    let row = &reading.view().table.expect("a table").rows[0];
    assert_eq!(row.cells[0].text, "L2");
    assert_eq!(row.cells[2].text, "+1.3 dB");
}
