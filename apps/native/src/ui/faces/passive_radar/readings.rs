use std::time::Duration;

use sdrmm_wire::{
    array::ProcessorGate,
    radar::{
        AoaState, Illuminator, RadarAoa, RadarAxes, RadarHealth, RadarProblem, RadarTrack,
        RadarUpdate, ReferenceMode, TrackState,
    },
};

use crate::ui::faces::processor::{Chip, NO_ARRAY, STALE, age_label};

pub const TRACK_ROWS: usize = 6;
pub const NO_TX: &str = "no tx";
const STALE_SLACK_MS: f64 = 1_000.0;
const TABLE_TITLE: &str = "Bearings use the ideal geometry";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subtitle {
    pub text: String,
    pub warn: bool,
}

impl Subtitle {
    fn calm(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            warn: false,
        }
    }

    fn warn(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            warn: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RadarState<'a> {
    pub wired: bool,
    pub tx_wired: bool,
    pub gate: Option<ProcessorGate>,
    pub update: Option<&'a RadarUpdate>,
    pub stale: bool,
    pub illuminator: Illuminator,
    pub lanes: usize,
}

#[must_use]
pub fn subtitle(state: &RadarState<'_>) -> Subtitle {
    if !state.wired {
        return Subtitle::calm(NO_ARRAY);
    }
    if let Some(gate) = state.gate {
        return Subtitle::warn(gate.label().to_lowercase());
    }
    if !state.tx_wired {
        return Subtitle::warn(NO_TX);
    }
    if let Some(problem) = state.update.and_then(|update| headline(&update.problems)) {
        return Subtitle::warn(problem.label());
    }
    if state.stale {
        return Subtitle::warn(STALE);
    }
    if let Some(update) = state.update {
        return Subtitle::calm(format!(
            "{} {:.2} MHz",
            state.illuminator.label(),
            update.axes.carrier_hz / 1e6
        ));
    }
    let noun = if state.lanes == 1 { "lane" } else { "lanes" };
    Subtitle::calm(format!("{} {noun}", state.lanes))
}

fn chip_title(problem: &RadarProblem) -> Option<&'static str> {
    matches!(problem, RadarProblem::TableOutOfRange).then_some(TABLE_TITLE)
}

fn headline(problems: &[RadarProblem]) -> Option<&RadarProblem> {
    problems
        .iter()
        .find(|problem| chip_title(problem).is_none())
}

#[must_use]
pub fn problems(update: Option<&RadarUpdate>) -> Vec<String> {
    update.map_or_else(Vec::new, |update| {
        update
            .problems
            .iter()
            .filter(|problem| chip_title(problem).is_none())
            .map(|problem| problem.label().to_owned())
            .collect()
    })
}

#[must_use]
pub fn chips(update: Option<&RadarUpdate>) -> Vec<Chip> {
    update.map_or_else(Vec::new, |update| {
        update
            .problems
            .iter()
            .filter_map(|problem| {
                chip_title(problem).map(|title| Chip::warn(problem.label(), title))
            })
            .collect()
    })
}

#[must_use]
pub fn is_stale(age: Duration, hop_ms: f32) -> bool {
    age.as_secs_f64() * 1_000.0 > 3.0 * f64::from(hop_ms) + STALE_SLACK_MS
}

#[must_use]
pub fn reference_text(update: &RadarUpdate) -> String {
    let reference = update.health.reference;
    if reference.mode == ReferenceMode::Raw {
        return "Raw".to_owned();
    }
    if !reference.locked {
        return "Lost".to_owned();
    }
    let mode = if reference.mode == ReferenceMode::Cma {
        "CMA"
    } else {
        "DAB"
    };
    format!("{mode} {:.0} dB", reference.quality_db)
}

#[must_use]
pub fn clutter_text(suppression: &[f32]) -> (String, String) {
    if suppression.is_empty() {
        return ("-".to_owned(), "Direct path and clutter removed".to_owned());
    }
    let mean = suppression.iter().sum::<f32>() / suppression.len() as f32;
    let title = suppression
        .iter()
        .enumerate()
        .map(|(lane, db)| format!("L{} {db:.0} dB", lane + 1))
        .collect::<Vec<_>>()
        .join(" \u{b7} ");
    (format!("{mean:.0} dB"), title)
}

#[must_use]
pub fn load_text(load: f32) -> String {
    format!("{:.0}%", (load * 100.0).round())
}

#[must_use]
pub const fn aoa_text(state: AoaState) -> &'static str {
    match state {
        AoaState::Off => "Off",
        AoaState::Ready => "Ready",
        AoaState::PhaseUnknown => "No phase",
        AoaState::OneLane => "One lane",
    }
}

#[must_use]
pub fn lost_cpis(health: &RadarHealth, axes: &RadarAxes, input_rate_hz: Option<f64>) -> u64 {
    let rate = input_rate_hz
        .filter(|rate| *rate > 0.0)
        .unwrap_or(axes.sample_rate_hz);
    let hop_s = f64::from(axes.hop_ms) / 1_000.0;
    let samples = if health.dropped_samples > 0 && rate > 0.0 && hop_s > 0.0 {
        (health.dropped_samples as f64 / rate / hop_s).ceil() as u64
    } else {
        0
    };
    health
        .dropped_cpis
        .saturating_add(health.dropped_reports)
        .saturating_add(health.lagged_updates)
        .saturating_add(samples)
}

#[must_use]
pub fn lost_title(health: &RadarHealth) -> String {
    [
        format!("CPIs {}", health.dropped_cpis),
        format!("discarded {}", health.discarded_cpis),
        format!("reports {}", health.dropped_reports),
        format!("lagged {}", health.lagged_updates),
        format!("samples {}", health.dropped_samples),
        format!("tracks {}", health.dropped_tracks),
        format!("detections cut {}", health.truncated_detections),
    ]
    .join(" \u{b7} ")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stat {
    pub label: &'static str,
    pub value: String,
    pub title: String,
    pub danger: bool,
}

fn stat(label: &'static str, value: String, title: &str) -> Stat {
    Stat {
        label,
        value,
        title: title.to_owned(),
        danger: false,
    }
}

#[must_use]
pub fn stats(
    update: Option<&RadarUpdate>,
    input_rate_hz: Option<f64>,
    age: Option<Duration>,
) -> Vec<Stat> {
    let blank = || "-".to_owned();
    let (clutter, clutter_title) =
        clutter_text(update.map_or(&[], |update| &update.health.suppression_db));
    let health = update.map(|update| &update.health);
    let load = health.map_or(0.0, |health| health.load);
    let mut shown = vec![
        stat(
            "Targets",
            confirmed_tracks(update).to_string(),
            "Confirmed tracks",
        ),
        stat(
            "Echoes",
            update.map_or_else(blank, |update| update.detections.len().to_string()),
            "Detections this CPI",
        ),
        stat("Clutter", clutter, &clutter_title),
        Stat {
            danger: load > 1.0,
            ..stat(
                "Load",
                health.map_or_else(blank, |health| load_text(health.load)),
                "Share of real time spent",
            )
        },
        stat(
            "Compute",
            health.map_or_else(blank, |health| format!("{:.0} ms", health.compute_ms)),
            "Time per CPI",
        ),
        stat(
            "Noise",
            health.map_or_else(blank, |health| format!("{:.0} dB", health.noise_floor_db)),
            "Noise floor",
        ),
        stat(
            "Ref",
            update.map_or_else(blank, reference_text),
            "Reference channel",
        ),
        stat(
            "AoA",
            health.map_or_else(blank, |health| aoa_text(health.aoa).to_owned()),
            "Angle of arrival",
        ),
        stat(
            "CPI",
            update.map_or_else(blank, |update| format!("{:.0} ms", update.axes.cpi_ms)),
            "Coherent processing interval",
        ),
        stat("Age", age_label(age), "Since the last update"),
    ];
    if let Some(update) = update {
        let lost = lost_cpis(&update.health, &update.axes, input_rate_hz);
        if lost > 0 {
            shown.push(Stat {
                danger: true,
                ..stat("Drops", lost.to_string(), &lost_title(&update.health))
            });
        }
    }
    shown
}

#[must_use]
pub fn confirmed_tracks(update: Option<&RadarUpdate>) -> usize {
    update.map_or(0, |update| {
        update
            .tracks
            .iter()
            .filter(|track| track.state == TrackState::Confirmed)
            .count()
    })
}

#[must_use]
pub fn sorted_tracks(tracks: &[RadarTrack], limit: usize) -> Vec<RadarTrack> {
    let mut sorted = tracks.to_vec();
    sorted.sort_by(|a, b| b.snr_db.total_cmp(&a.snr_db).then(a.id.cmp(&b.id)));
    sorted.truncate(limit);
    sorted
}

#[must_use]
pub fn bearing_cell(aoa: Option<&RadarAoa>) -> (String, bool) {
    match aoa {
        None => ("-".to_owned(), false),
        Some(RadarAoa {
            bearing_deg: Some(bearing),
            ..
        }) => (format!("{:.0}\u{b0}", bearing.round()), false),
        Some(aoa) => (format!("{:.0}\u{b0}", aoa.azimuth_deg.round()), true),
    }
}

#[must_use]
pub fn adsb_cell(track: &RadarTrack) -> String {
    track.adsb.as_ref().map_or_else(
        || "-".to_owned(),
        |adsb| adsb.callsign.clone().unwrap_or_else(|| adsb.icao.clone()),
    )
}

#[must_use]
pub fn signed(value: f64, digits: usize) -> String {
    let text = format!("{value:.digits$}");
    let zero = text
        .trim_start_matches('-')
        .chars()
        .all(|c| c == '0' || c == '.');
    if zero {
        return text.trim_start_matches('-').to_owned();
    }
    if value > 0.0 {
        format!("+{text}")
    } else {
        text
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackRow {
    pub id: u32,
    pub name: String,
    pub range: String,
    pub rate: String,
    pub doppler: String,
    pub bearing: String,
    pub array_frame: bool,
    pub snr: String,
    pub adsb: String,
    pub coasting: bool,
}

#[must_use]
pub fn track_name(id: u32) -> String {
    format!("T{id}")
}

#[must_use]
pub fn track_row(track: &RadarTrack) -> TrackRow {
    let (bearing, array_frame) = bearing_cell(track.aoa.as_ref());
    TrackRow {
        id: track.id,
        name: track_name(track.id),
        range: format!("{:.1}", track.range_km),
        rate: format!("{:.0}", track.range_rate_mps),
        doppler: signed(f64::from(track.doppler_hz), 0),
        bearing,
        array_frame,
        snr: format!("{:.0}", track.snr_db),
        adsb: adsb_cell(track),
        coasting: track.state == TrackState::Coasting,
    }
}

#[must_use]
pub fn track_rows(update: Option<&RadarUpdate>) -> Vec<TrackRow> {
    update.map_or_else(Vec::new, |update| {
        sorted_tracks(&update.tracks, TRACK_ROWS)
            .iter()
            .map(track_row)
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::radar::{AdsbMatch, ReferenceHealth};

    use super::*;

    fn update() -> RadarUpdate {
        RadarUpdate {
            axes: RadarAxes {
                carrier_hz: 98.5e6,
                hop_ms: 500.0,
                cpi_ms: 500.0,
                sample_rate_hz: 1e6,
                ..RadarAxes::default()
            },
            ..RadarUpdate::default()
        }
    }

    fn state(update: Option<&RadarUpdate>) -> RadarState<'_> {
        RadarState {
            wired: true,
            tx_wired: true,
            gate: None,
            update,
            stale: false,
            illuminator: Illuminator::Fm,
            lanes: 2,
        }
    }

    fn track(id: u32, snr_db: f32) -> RadarTrack {
        RadarTrack {
            id,
            snr_db,
            range_km: 42.14,
            range_rate_mps: -120.4,
            doppler_hz: 40.0,
            ..RadarTrack::default()
        }
    }

    #[test]
    fn the_subtitle_walks_wiring_gate_transmitter_problem_staleness_then_carrier() {
        let held = update();
        assert_eq!(
            subtitle(&RadarState {
                wired: false,
                ..state(None)
            })
            .text,
            NO_ARRAY
        );
        let gated = subtitle(&RadarState {
            gate: Some(ProcessorGate::Phase),
            ..state(None)
        });
        assert_eq!(gated, Subtitle::warn("needs cal"));
        assert_eq!(
            subtitle(&RadarState {
                tx_wired: false,
                ..state(None)
            }),
            Subtitle::warn(NO_TX)
        );
        let troubled = RadarUpdate {
            problems: vec![RadarProblem::TableOutOfRange, RadarProblem::ReferenceLost],
            ..update()
        };
        assert_eq!(subtitle(&state(Some(&troubled))).text, "Reference lost");
        assert_eq!(
            subtitle(&RadarState {
                stale: true,
                ..state(Some(&held))
            })
            .text,
            STALE
        );
        assert_eq!(
            subtitle(&state(Some(&held))),
            Subtitle::calm("FM 98.50 MHz")
        );
        assert_eq!(subtitle(&state(None)).text, "2 lanes");
        assert_eq!(
            subtitle(&RadarState {
                lanes: 1,
                ..state(None)
            })
            .text,
            "1 lane"
        );
    }

    #[test]
    fn only_the_cal_table_problem_becomes_a_chip() {
        let troubled = RadarUpdate {
            problems: vec![
                RadarProblem::TableOutOfRange,
                RadarProblem::Refused("Too wide".into()),
            ],
            ..update()
        };
        let found = chips(Some(&troubled));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].label, "Outside cal table");
        assert_eq!(found[0].title, TABLE_TITLE);
        assert_eq!(problems(Some(&troubled)), vec!["Too wide".to_owned()]);
        assert!(chips(None).is_empty());
    }

    #[test]
    fn a_reading_goes_stale_after_three_hops_and_a_second() {
        assert!(!is_stale(Duration::from_millis(2_400), 500.0));
        assert!(is_stale(Duration::from_millis(2_400), 400.0));
    }

    #[test]
    fn the_reference_reads_raw_lost_or_its_quality() {
        let mut held = update();
        assert_eq!(reference_text(&held), "Raw");
        held.health.reference = ReferenceHealth {
            mode: ReferenceMode::Cma,
            locked: false,
            quality_db: 12.4,
            fallback_frames: 0,
        };
        assert_eq!(reference_text(&held), "Lost");
        held.health.reference.locked = true;
        assert_eq!(reference_text(&held), "CMA 12 dB");
        held.health.reference.mode = ReferenceMode::DabRemod;
        assert_eq!(reference_text(&held), "DAB 12 dB");
    }

    #[test]
    fn clutter_reads_the_mean_and_titles_each_lane() {
        assert_eq!(clutter_text(&[]).0, "-");
        let (value, title) = clutter_text(&[40.0, 50.0]);
        assert_eq!(value, "45 dB");
        assert_eq!(title, "L1 40 dB \u{b7} L2 50 dB");
        assert_eq!(load_text(0.456), "46%");
    }

    #[test]
    fn dropped_samples_count_as_the_hops_they_cover() {
        let held = RadarUpdate {
            health: RadarHealth {
                dropped_cpis: 1,
                lagged_updates: 2,
                dropped_samples: 750_000,
                ..RadarHealth::default()
            },
            ..update()
        };
        assert_eq!(lost_cpis(&held.health, &held.axes, None), 5);
        assert_eq!(lost_cpis(&held.health, &held.axes, Some(3e6)), 4);
        assert!(lost_title(&held.health).starts_with("CPIs 1 \u{b7} discarded 0"));
    }

    #[test]
    fn stats_show_dashes_until_a_reading_and_drops_only_when_lost() {
        let empty = stats(None, None, None);
        assert!(empty.iter().all(|stat| stat.label != "Drops"));
        assert_eq!(
            empty
                .iter()
                .find(|stat| stat.label == "Load")
                .map(|stat| stat.value.as_str()),
            Some("-")
        );
        let held = RadarUpdate {
            health: RadarHealth {
                load: 1.2,
                dropped_reports: 1,
                aoa: AoaState::PhaseUnknown,
                ..RadarHealth::default()
            },
            ..update()
        };
        let shown = stats(Some(&held), None, Some(Duration::from_secs(1)));
        let find = |label| shown.iter().find(|stat| stat.label == label);
        assert!(find("Load").is_some_and(|stat| stat.danger && stat.value == "120%"));
        assert!(find("Drops").is_some_and(|stat| stat.danger && stat.value == "1"));
        assert_eq!(
            find("AoA").map(|stat| stat.value.as_str()),
            Some("No phase")
        );
        assert_eq!(find("CPI").map(|stat| stat.value.as_str()), Some("500 ms"));
    }

    #[test]
    fn tracks_sort_by_snr_then_id_and_stop_at_the_limit() {
        let sorted = sorted_tracks(&[track(4, 10.0), track(2, 20.0), track(1, 10.0)], 2);
        assert_eq!(
            sorted.iter().map(|track| track.id).collect::<Vec<_>>(),
            vec![2, 1]
        );
    }

    #[test]
    fn only_confirmed_tracks_count_as_targets() {
        let held = RadarUpdate {
            tracks: vec![
                track(1, 10.0),
                RadarTrack {
                    state: TrackState::Coasting,
                    ..track(2, 9.0)
                },
            ],
            ..update()
        };
        assert_eq!(confirmed_tracks(Some(&held)), 1);
        assert_eq!(confirmed_tracks(None), 0);
    }

    #[test]
    fn a_bearing_prefers_true_north_and_flags_the_array_frame() {
        assert_eq!(bearing_cell(None), ("-".to_owned(), false));
        let array = RadarAoa {
            azimuth_deg: 231.4,
            ..RadarAoa::default()
        };
        assert_eq!(bearing_cell(Some(&array)), ("231\u{b0}".to_owned(), true));
        let north = RadarAoa {
            bearing_deg: Some(12.6),
            ..array
        };
        assert_eq!(bearing_cell(Some(&north)), ("13\u{b0}".to_owned(), false));
    }

    #[test]
    fn a_track_row_reads_short_with_its_adsb_match() {
        let matched = RadarTrack {
            adsb: Some(AdsbMatch {
                icao: "3C6444".into(),
                callsign: Some("DLH4AB".into()),
            }),
            state: TrackState::Coasting,
            ..track(3, 14.2)
        };
        let row = track_row(&matched);
        assert_eq!(row.name, "T3");
        assert_eq!(row.range, "42.1");
        assert_eq!(row.rate, "-120");
        assert_eq!(row.doppler, "+40");
        assert_eq!(row.snr, "14");
        assert_eq!(row.adsb, "DLH4AB");
        assert!(row.coasting);
        let bare = AdsbMatch {
            icao: "3C6444".into(),
            callsign: None,
        };
        assert_eq!(
            adsb_cell(&RadarTrack {
                adsb: Some(bare),
                ..track(1, 1.0)
            }),
            "3C6444"
        );
        assert_eq!(adsb_cell(&track(1, 1.0)), "-");
    }

    #[test]
    fn signed_values_carry_a_plus_but_zero_never_does() {
        assert_eq!(signed(12.3, 0), "+12");
        assert_eq!(signed(-4.0, 0), "-4");
        assert_eq!(signed(0.2, 0), "0");
        assert_eq!(signed(-0.2, 0), "0");
    }

    #[test]
    fn aoa_states_read_short() {
        assert_eq!(aoa_text(AoaState::OneLane), "One lane");
        assert_eq!(aoa_text(AoaState::Ready), "Ready");
    }
}
