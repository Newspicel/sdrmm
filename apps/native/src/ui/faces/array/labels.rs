use std::time::{Duration, SystemTime};

use sdrmm_wire::{
    array::{
        ArrayCalSource, ArrayFailure, ArrayOrientation, ArrayRecordingStatus, ArrayStatus,
        CalPhase, CalSourceKind, SyncState,
    },
    device::Coherence,
    limits::ARRAY_LIMITS,
    position::HeadingSource,
};

use crate::ui::faces::processor::age_label;

pub const NO_LANES: &str = "no lanes";
pub const FAILED: &str = "failed";
pub const PICK_ONE: &str = "Pick one";
pub const NONE: &str = "None";

const CHECK_INTERVALS: [(u32, &str); 4] = [(0, "Off"), (10, "10 s"), (60, "60 s"), (300, "5 min")];
const DELAY_DIGITS: [(f64, usize); 3] = [(9.995, 2), (999.95, 1), (99_999.5, 0)];

#[must_use]
pub const fn tier_text(tier: Coherence) -> &'static str {
    match tier {
        Coherence::None => NONE,
        Coherence::TimeSync => "Shared clock",
        Coherence::PhaseCoherent => "Shared LO",
    }
}

#[must_use]
pub fn tier_options(declared: Coherence) -> Vec<(Coherence, String)> {
    let unset = (declared == Coherence::None).then(|| (Coherence::None, PICK_ONE.to_owned()));
    unset
        .into_iter()
        .chain(
            [Coherence::TimeSync, Coherence::PhaseCoherent]
                .map(|tier| (tier, tier_text(tier).to_owned())),
        )
        .collect()
}

#[must_use]
pub fn tier_label(status: &ArrayStatus) -> String {
    let text = tier_text(status.tier);
    if status.tier_capped {
        format!("{text} · capped")
    } else {
        text.to_owned()
    }
}

fn solved_at(status: &ArrayStatus) -> Option<SystemTime> {
    let solved = matches!(
        status.cal,
        CalPhase::Solved | CalPhase::Warm | CalPhase::Stale
    );
    let at = status
        .last_solve_at
        .as_deref()?
        .parse::<jiff::Timestamp>()
        .ok()?;
    solved.then(|| SystemTime::from(at))
}

#[must_use]
pub fn cal_label(status: &ArrayStatus, now: SystemTime) -> String {
    let text = status.cal.label();
    match solved_at(status) {
        Some(at) => {
            let age = now.duration_since(at).unwrap_or(Duration::ZERO);
            format!("{text} {} ago", age_label(Some(age)))
        }
        None => text.to_owned(),
    }
}

#[must_use]
pub fn cal_title(status: &ArrayStatus) -> Option<String> {
    if let Some(failure) = &status.failure {
        return Some(failure.to_string());
    }
    status
        .next_check_in_s
        .map(|next| format!("Next check in {} s", next.max(0.0).round()))
}

#[must_use]
pub fn sync_label(status: &ArrayStatus) -> String {
    let text = status.sync.label();
    status
        .drift_ppm
        .map_or_else(|| text.to_owned(), |ppm| format!("{text} · {ppm:.1} ppm"))
}

#[must_use]
pub fn subtitle(status: Option<&ArrayStatus>, wired: usize) -> String {
    if wired == 0 {
        return NO_LANES.to_owned();
    }
    if status.is_some_and(|status| status.failure.is_some()) {
        return FAILED.to_owned();
    }
    status
        .map_or(SyncState::Idle, |status| status.sync)
        .label()
        .to_lowercase()
}

#[must_use]
pub fn failure_title(failure: &ArrayFailure) -> String {
    match failure {
        ArrayFailure::Stopped { message } => message.clone(),
        _ => failure.to_string(),
    }
}

fn offset_and_width(source: &ArrayCalSource) -> (f64, f64) {
    match *source {
        ArrayCalSource::Pilot {
            offset_hz,
            bandwidth_hz,
        }
        | ArrayCalSource::Emitter {
            offset_hz,
            bandwidth_hz,
            ..
        } => (offset_hz, bandwidth_hz),
        _ => (0.0, ARRAY_LIMITS.cal_bandwidth_seed_hz),
    }
}

#[must_use]
pub fn cal_source_of(kind: Option<CalSourceKind>, from: &ArrayCalSource) -> ArrayCalSource {
    let (offset_hz, bandwidth_hz) = offset_and_width(from);
    match kind {
        Some(CalSourceKind::Noise) => ArrayCalSource::Noise,
        Some(CalSourceKind::Pilot) => ArrayCalSource::Pilot {
            offset_hz,
            bandwidth_hz,
        },
        Some(CalSourceKind::Emitter) => ArrayCalSource::Emitter {
            offset_hz,
            bandwidth_hz,
            bearing_deg: match *from {
                ArrayCalSource::Emitter { bearing_deg, .. } => bearing_deg,
                _ => 0.0,
            },
        },
        None => ArrayCalSource::Off,
    }
}

#[must_use]
pub fn cal_sources() -> Vec<(Option<CalSourceKind>, String)> {
    [
        (Some(CalSourceKind::Noise), "Noise"),
        (Some(CalSourceKind::Pilot), "Pilot"),
        (Some(CalSourceKind::Emitter), "Emitter"),
        (None, "Off"),
    ]
    .into_iter()
    .map(|(kind, label)| (kind, label.to_owned()))
    .collect()
}

#[must_use]
pub fn lane_quality_percent(quality: f32) -> u8 {
    (quality.clamp(0.0, 1.0) * 100.0).round() as u8
}

#[must_use]
pub fn check_options(current: u32) -> Vec<(u32, String)> {
    let mut options: Vec<(u32, String)> = CHECK_INTERVALS
        .iter()
        .map(|(value, label)| (*value, (*label).to_owned()))
        .collect();
    if !options.iter().any(|(value, _)| *value == current) {
        options.push((current, format!("{current} s")));
        options.sort_by_key(|(value, _)| *value);
    }
    options
}

#[must_use]
pub fn whole_degrees(deg: f64) -> f64 {
    deg.rem_euclid(360.0).round() % 360.0
}

#[must_use]
pub fn degrees_text(deg: f64) -> String {
    format!("{}°", whole_degrees(deg))
}

#[must_use]
pub fn switched_orientation(
    heading: bool,
    current: ArrayOrientation,
    azimuth_deg: Option<f64>,
) -> ArrayOrientation {
    match (heading, current) {
        (false, ArrayOrientation::Fixed { .. }) | (true, ArrayOrientation::Heading { .. }) => {
            current
        }
        (false, _) => ArrayOrientation::Fixed {
            azimuth_deg: whole_degrees(azimuth_deg.unwrap_or(0.0)),
        },
        (true, _) => ArrayOrientation::Heading {
            mount_offset_deg: 0.0,
        },
    }
}

#[must_use]
pub const fn heading_source_label(source: HeadingSource) -> &'static str {
    match source {
        HeadingSource::Compass => "compass",
        HeadingSource::Course => "GPS course",
        HeadingSource::Fused => "fused",
        HeadingSource::Gnss => "GNSS",
        HeadingSource::Sensor => "sensor",
    }
}

#[must_use]
pub fn heading_label(orientation: ArrayOrientation, status: Option<&ArrayStatus>) -> String {
    if let ArrayOrientation::Fixed { azimuth_deg } = orientation {
        return format!("Fixed {}", degrees_text(azimuth_deg));
    }
    let Some(azimuth) = status.and_then(|status| status.azimuth_deg) else {
        return NONE.to_owned();
    };
    match status.and_then(|status| status.heading_source) {
        Some(source) => format!("{} {}", degrees_text(azimuth), heading_source_label(source)),
        None => degrees_text(azimuth),
    }
}

#[must_use]
pub fn lane_gaps(status: &ArrayStatus) -> u64 {
    status.lanes.iter().map(|lane| lane.gaps).sum()
}

#[must_use]
pub fn delay_label(samples: f64) -> String {
    let size = samples.abs();
    DELAY_DIGITS
        .iter()
        .find(|(below, _)| size < *below)
        .map_or_else(
            || format!("{:.0}k", samples / 1_000.0),
            |(_, digits)| format!("{samples:.digits$}"),
        )
}

#[must_use]
pub fn recording_label(recording: &ArrayRecordingStatus, sample_rate: f64) -> String {
    if let Some(error) = &recording.error {
        return error.clone();
    }
    let seconds = if sample_rate > 0.0 {
        (recording.samples as f64 / sample_rate).floor()
    } else {
        0.0
    };
    if recording.dropped > 0 {
        format!("{seconds} s · {} dropped", recording.dropped)
    } else {
        format!("{seconds} s")
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::array::ArrayLaneStatus;

    use super::*;

    fn status() -> ArrayStatus {
        ArrayStatus {
            node: "north".to_owned(),
            ..ArrayStatus::default()
        }
    }

    #[test]
    fn tiers_read_truthfully_and_say_when_capped() {
        assert_eq!(tier_label(&status()), "None");
        let capped = ArrayStatus {
            tier: Coherence::TimeSync,
            tier_capped: true,
            ..status()
        };
        assert_eq!(tier_label(&capped), "Shared clock · capped");
        let shared = ArrayStatus {
            tier: Coherence::PhaseCoherent,
            ..status()
        };
        assert_eq!(tier_label(&shared), "Shared LO");
    }

    #[test]
    fn an_unset_tier_offers_pick_one_first() {
        let set = tier_options(Coherence::TimeSync);
        assert_eq!(
            set,
            vec![
                (Coherence::TimeSync, "Shared clock".to_owned()),
                (Coherence::PhaseCoherent, "Shared LO".to_owned())
            ]
        );
        let unset = tier_options(Coherence::None);
        assert_eq!(unset[0], (Coherence::None, PICK_ONE.to_owned()));
        assert_eq!(unset[1..], set[..]);
    }

    #[test]
    fn calibration_says_how_long_ago_it_solved() {
        let at: jiff::Timestamp = "2026-09-28T12:00:00Z".parse().expect("timestamp");
        let solved_at = SystemTime::from(at);
        let solved = ArrayStatus {
            cal: CalPhase::Solved,
            last_solve_at: Some("2026-09-28T12:00:00Z".to_owned()),
            ..status()
        };
        assert_eq!(
            cal_label(&solved, solved_at + Duration::from_millis(12_300)),
            "Calibrated 12 s ago"
        );
        assert_eq!(
            cal_label(&solved, solved_at + Duration::from_secs(185)),
            "Calibrated 3 min ago"
        );
        assert_eq!(cal_label(&status(), solved_at), "No cal");
        let failed = ArrayStatus {
            cal: CalPhase::Failed,
            ..solved.clone()
        };
        assert_eq!(cal_label(&failed, solved_at), "Cal failed");
    }

    #[test]
    fn calibration_is_titled_by_the_failure_or_the_next_check() {
        let failed = ArrayStatus {
            failure: Some(ArrayFailure::NoiseNotSeen),
            ..status()
        };
        assert_eq!(cal_title(&failed).as_deref(), Some("Noise not seen"));
        let waiting = ArrayStatus {
            next_check_in_s: Some(39.6),
            ..status()
        };
        assert_eq!(cal_title(&waiting).as_deref(), Some("Next check in 40 s"));
        assert_eq!(cal_title(&status()), None);
    }

    #[test]
    fn sync_carries_the_clock_drift() {
        let locked = ArrayStatus {
            sync: SyncState::Locked,
            drift_ppm: Some(0.24),
            ..status()
        };
        assert_eq!(sync_label(&locked), "Locked · 0.2 ppm");
        let searching = ArrayStatus {
            sync: SyncState::Searching,
            ..status()
        };
        assert_eq!(sync_label(&searching), "Syncing");
    }

    #[test]
    fn the_subtitle_reads_lanes_then_failure_then_sync() {
        assert_eq!(subtitle(None, 0), NO_LANES);
        assert_eq!(subtitle(None, 2), "idle");
        let failed = ArrayStatus {
            failure: Some(ArrayFailure::Unwired),
            ..status()
        };
        assert_eq!(subtitle(Some(&failed), 3), FAILED);
        let drifting = ArrayStatus {
            sync: SyncState::Drifting,
            ..status()
        };
        assert_eq!(subtitle(Some(&drifting), 3), "drifting");
    }

    #[test]
    fn a_stopped_array_is_titled_by_its_message() {
        let stopped = ArrayFailure::Stopped {
            message: "USB gone".to_owned(),
        };
        assert_eq!(failure_title(&stopped), "USB gone");
        assert_eq!(
            failure_title(&ArrayFailure::LaneGap { lane: 1 }),
            "Lane 2 unwired"
        );
    }

    #[test]
    fn the_heading_is_named_by_orientation_and_source() {
        let heading = ArrayOrientation::Heading {
            mount_offset_deg: 0.0,
        };
        assert_eq!(
            heading_label(ArrayOrientation::Fixed { azimuth_deg: 90.0 }, None),
            "Fixed 90°"
        );
        assert_eq!(heading_label(heading, None), "None");
        let compass = ArrayStatus {
            azimuth_deg: Some(123.4),
            heading_source: Some(HeadingSource::Compass),
            ..status()
        };
        assert_eq!(heading_label(heading, Some(&compass)), "123° compass");
        let course = ArrayStatus {
            azimuth_deg: Some(45.0),
            heading_source: Some(HeadingSource::Course),
            ..status()
        };
        assert_eq!(heading_label(heading, Some(&course)), "45° GPS course");
        let north = ArrayStatus {
            azimuth_deg: Some(-0.2),
            ..status()
        };
        assert_eq!(heading_label(heading, Some(&north)), "0°");
    }

    #[test]
    fn quality_stays_inside_the_bar() {
        assert_eq!(lane_quality_percent(0.826), 83);
        assert_eq!(lane_quality_percent(1.4), 100);
        assert_eq!(lane_quality_percent(-1.0), 0);
    }

    #[test]
    fn a_stored_check_interval_joins_the_list_in_order() {
        let labels: Vec<String> = check_options(60)
            .into_iter()
            .map(|(_, label)| label)
            .collect();
        assert_eq!(labels, vec!["Off", "10 s", "60 s", "5 min"]);
        let values: Vec<u32> = check_options(30)
            .into_iter()
            .map(|(value, _)| value)
            .collect();
        assert_eq!(values, vec![0, 10, 30, 60, 300]);
    }

    #[test]
    fn switching_cal_source_keeps_offset_and_width() {
        let pilot = cal_source_of(Some(CalSourceKind::Pilot), &ArrayCalSource::Noise);
        assert_eq!(
            pilot,
            ArrayCalSource::Pilot {
                offset_hz: 0.0,
                bandwidth_hz: 20_000.0
            }
        );
        let tuned = ArrayCalSource::Pilot {
            offset_hz: 12_500.0,
            bandwidth_hz: 5_000.0,
        };
        let emitter = ArrayCalSource::Emitter {
            offset_hz: 12_500.0,
            bandwidth_hz: 5_000.0,
            bearing_deg: 0.0,
        };
        assert_eq!(cal_source_of(Some(CalSourceKind::Emitter), &tuned), emitter);
        let aimed = ArrayCalSource::Emitter {
            offset_hz: 12_500.0,
            bandwidth_hz: 5_000.0,
            bearing_deg: 42.0,
        };
        assert_eq!(cal_source_of(Some(CalSourceKind::Emitter), &aimed), aimed);
        assert_eq!(cal_source_of(Some(CalSourceKind::Pilot), &aimed), tuned);
        assert_eq!(cal_source_of(None, &aimed), ArrayCalSource::Off);
        assert_eq!(cal_sources().len(), 4);
    }

    #[test]
    fn a_fixed_array_starts_where_the_heading_last_pointed() {
        let fixed = ArrayOrientation::Fixed { azimuth_deg: 45.0 };
        assert_eq!(switched_orientation(false, fixed, Some(200.0)), fixed);
        assert_eq!(
            switched_orientation(true, fixed, Some(200.0)),
            ArrayOrientation::Heading {
                mount_offset_deg: 0.0
            }
        );
        let heading = ArrayOrientation::Heading {
            mount_offset_deg: 10.0,
        };
        assert_eq!(
            switched_orientation(false, heading, Some(359.7)),
            ArrayOrientation::Fixed { azimuth_deg: 0.0 }
        );
        assert_eq!(
            switched_orientation(false, heading, Some(-91.2)),
            ArrayOrientation::Fixed { azimuth_deg: 269.0 }
        );
        assert_eq!(
            switched_orientation(false, heading, None),
            ArrayOrientation::Fixed { azimuth_deg: 0.0 }
        );
    }

    #[test]
    fn a_recording_says_how_long_it_runs_and_what_it_dropped() {
        let running = ArrayRecordingStatus {
            stem: "north-1".to_owned(),
            samples: 4_096_000,
            ..ArrayRecordingStatus::default()
        };
        assert_eq!(recording_label(&running, 2_048_000.0), "2 s");
        let dropping = ArrayRecordingStatus {
            dropped: 12,
            ..running.clone()
        };
        assert_eq!(recording_label(&dropping, 2_048_000.0), "2 s · 12 dropped");
        let broken = ArrayRecordingStatus {
            error: Some("Disk full".to_owned()),
            ..running
        };
        assert_eq!(recording_label(&broken, 2_048_000.0), "Disk full");
    }

    #[test]
    fn a_lane_delay_stays_within_six_characters() {
        assert_eq!(delay_label(0.0), "0.00");
        assert_eq!(delay_label(-4.567), "-4.57");
        assert_eq!(delay_label(-123.45), "-123.5");
        assert_eq!(delay_label(16_803.25), "16803");
        assert_eq!(delay_label(-99_999.4), "-99999");
        assert_eq!(delay_label(1_234_567.0), "1235k");
        assert_eq!(delay_label(-999.96), "-1000");
        assert_eq!(delay_label(-99_999.6), "-100k");
        for delay in [
            -9.999,
            -999.96,
            -16_803.25,
            -99_999.6,
            -480_000.0,
            -1_234_567.0,
        ] {
            assert!(delay_label(delay).len() <= 6, "{delay}");
        }
    }

    #[test]
    fn lane_gaps_add_up() {
        let lanes = [2, 0, 5]
            .map(|gaps| ArrayLaneStatus {
                gaps,
                ..ArrayLaneStatus::default()
            })
            .to_vec();
        assert_eq!(lane_gaps(&ArrayStatus { lanes, ..status() }), 7);
    }
}
