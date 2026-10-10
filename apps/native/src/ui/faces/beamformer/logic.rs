use std::f64::consts::PI;

use sdrmm_wire::processor::beamformer::{
    Adaptation, BeamMode, BeamformerParams, BeamformerReading, LaneWeight, NoiseModel, SteerSource,
};

use crate::ui::{faces::processor::Chip, kit_sources::units::sample_rate};

pub const NULL_CLOSE_DEG: f64 = 10.0;
pub const STEER_UNWIRED: &str = "Wire a DF to steer";

pub const MODES: [(BeamMode, &str, &str); 7] = [
    (BeamMode::Mrc, "MRC", "Best SNR from every lane"),
    (BeamMode::Das, "Beam", "Steer with equal weights"),
    (BeamMode::Mvdr, "MVDR", "Steer and suppress the rest"),
    (BeamMode::Lcmv, "Nulls", "Steer with nulls"),
    (
        BeamMode::Gsc,
        "GSC",
        "Steer and cancel the rest as it moves",
    ),
    (
        BeamMode::Canceller,
        "Cancel",
        "Subtract what reference lanes hear",
    ),
    (BeamMode::Cma, "CMA", "Lock onto a steady envelope"),
];

pub const ADAPTATIONS: [(Adaptation, &str, &str); 2] = [
    (Adaptation::Nlms, "NLMS", "Light, follows slowly"),
    (Adaptation::Rls, "RLS", "Heavier, follows fast"),
];

pub const NOISE: [(NoiseModel, &str, &str); 2] = [
    (
        NoiseModel::Measured,
        "Measured",
        "Noise of each lane as measured",
    ),
    (NoiseModel::White, "White", "Same noise on every lane"),
];

#[must_use]
pub fn options<T: Copy>(table: &[(T, &str, &str)]) -> Vec<(T, String)> {
    table
        .iter()
        .map(|(value, label, _)| (*value, (*label).to_owned()))
        .collect()
}

#[must_use]
pub fn title_of<T: Copy + PartialEq>(table: &[(T, &str, &'static str)], value: T) -> &'static str {
    table
        .iter()
        .find(|(option, _, _)| *option == value)
        .map_or("", |(_, _, title)| *title)
}

#[must_use]
pub fn mode_label(mode: BeamMode) -> &'static str {
    MODES
        .iter()
        .find(|(option, _, _)| *option == mode)
        .map_or("-", |(_, label, _)| *label)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeamSetting {
    Steer,
    Nulls,
    AutoNulls,
    Main,
    Refs,
    Taps,
    Adaptation,
    Step,
    Forget,
    Crossfade,
    Update,
    CarryOver,
    Loading,
    Noise,
    Timeout,
}

fn block_settings(covariance: bool) -> Vec<BeamSetting> {
    let mut shown = vec![BeamSetting::Crossfade, BeamSetting::Update];
    if covariance {
        shown.push(BeamSetting::CarryOver);
    }
    shown
}

fn canceller_settings(params: &BeamformerParams) -> Vec<BeamSetting> {
    if params.taps < 2 {
        return block_settings(true);
    }
    let solver = match params.adaptation {
        Adaptation::Rls => BeamSetting::Forget,
        Adaptation::Nlms => BeamSetting::Step,
    };
    vec![BeamSetting::Adaptation, solver]
}

fn mode_settings(params: &BeamformerParams) -> Vec<BeamSetting> {
    use BeamSetting::{AutoNulls, Loading, Main, Noise, Nulls, Refs, Step, Taps};
    let (head, tail) = match params.mode {
        BeamMode::Mrc => (vec![Noise], block_settings(true)),
        BeamMode::Das => (Vec::new(), block_settings(false)),
        BeamMode::Mvdr => (vec![Loading], block_settings(true)),
        BeamMode::Lcmv => (vec![Nulls, AutoNulls, Loading], block_settings(true)),
        BeamMode::Gsc => (vec![Nulls, AutoNulls, Loading, Step], Vec::new()),
        BeamMode::Canceller => (vec![Main, Refs, Taps], canceller_settings(params)),
        BeamMode::Cma => (vec![Step], Vec::new()),
    };
    head.into_iter().chain(tail).collect()
}

#[must_use]
pub fn steered(mode: BeamMode) -> bool {
    matches!(
        mode,
        BeamMode::Das | BeamMode::Mvdr | BeamMode::Lcmv | BeamMode::Gsc
    )
}

#[must_use]
pub fn visible_settings(params: &BeamformerParams) -> Vec<BeamSetting> {
    let mut shown = mode_settings(params);
    if steered(params.mode) {
        shown.push(BeamSetting::Steer);
        if params.steer == SteerSource::Wired {
            shown.push(BeamSetting::Timeout);
        }
    }
    shown
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SteerKind {
    Wired,
    Fixed,
}

#[must_use]
pub fn steer_kind(steer: SteerSource) -> SteerKind {
    match steer {
        SteerSource::Wired => SteerKind::Wired,
        SteerSource::Fixed { .. } => SteerKind::Fixed,
    }
}

#[must_use]
pub fn steer_pick(current: SteerSource, kind: SteerKind) -> Option<SteerSource> {
    if steer_kind(current) == kind {
        return None;
    }
    Some(match kind {
        SteerKind::Wired => SteerSource::Wired,
        SteerKind::Fixed => SteerSource::Fixed {
            azimuth_deg: 0.0,
            elevation_deg: 0.0,
        },
    })
}

#[must_use]
pub fn with_azimuth(steer: SteerSource, azimuth_deg: f64) -> SteerSource {
    let elevation_deg = match steer {
        SteerSource::Fixed { elevation_deg, .. } => elevation_deg,
        SteerSource::Wired => 0.0,
    };
    SteerSource::Fixed {
        azimuth_deg,
        elevation_deg,
    }
}

#[must_use]
pub fn steer_value(steer: SteerSource) -> String {
    match steer {
        SteerSource::Wired => "DF".to_owned(),
        SteerSource::Fixed { azimuth_deg, .. } => null_label(azimuth_deg),
    }
}

#[must_use]
pub fn steer_title(wired: bool) -> String {
    if wired {
        "Beam direction".to_owned()
    } else {
        format!("Beam direction. {STEER_UNWIRED}")
    }
}

#[must_use]
pub fn amplitude_percent(weights: &[LaneWeight]) -> Vec<u32> {
    let linear: Vec<f64> = weights
        .iter()
        .map(|weight| 10f64.powf(f64::from(weight.amplitude_db) / 20.0))
        .collect();
    let top = linear.iter().copied().fold(0.0, f64::max);
    linear
        .iter()
        .map(|value| {
            if top > 0.0 {
                (value / top * 100.0).round() as u32
            } else {
                0
            }
        })
        .collect()
}

#[must_use]
pub fn weight_title(lane: usize, weight: LaneWeight) -> String {
    format!(
        "L{} {:.1} dB {}°",
        lane + 1,
        weight.amplitude_db,
        weight.phase_deg.round() as i32
    )
}

#[must_use]
pub fn wrap_deg(deg: f64) -> f64 {
    ((deg * 10.0).round() / 10.0).rem_euclid(360.0)
}

#[must_use]
pub fn with_null(nulls: &[f64], deg: f64) -> Vec<f64> {
    if !deg.is_finite() {
        return nulls.to_vec();
    }
    let mut next: Vec<f64> = nulls.iter().copied().map(wrap_deg).collect();
    next.push(wrap_deg(deg));
    next.sort_by(f64::total_cmp);
    next.dedup();
    next
}

#[must_use]
pub fn without_null(nulls: &[f64], deg: f64) -> Vec<f64> {
    let gone = wrap_deg(deg);
    nulls
        .iter()
        .copied()
        .map(wrap_deg)
        .filter(|value| *value != gone)
        .collect()
}

#[must_use]
pub fn null_label(deg: f64) -> String {
    format!("{:03}°", deg.round() as i64)
}

#[must_use]
pub fn nulls_value(params: &BeamformerParams) -> String {
    let fixed = params
        .nulls_deg
        .iter()
        .map(|deg| null_label(*deg))
        .collect::<Vec<_>>()
        .join(" ");
    match (fixed.is_empty(), params.auto_nulls) {
        (true, false) => "none".to_owned(),
        (true, true) => "auto".to_owned(),
        (false, false) => fixed,
        (false, true) => format!("{fixed} +auto"),
    }
}

#[must_use]
pub fn steer_label(reading: &BeamformerReading, steer: SteerSource) -> String {
    let Some(deg) = reading.steer_deg else {
        return "-".to_owned();
    };
    let source = match steer {
        SteerSource::Wired => "DF",
        SteerSource::Fixed { .. } => "fixed",
    };
    format!("{}° {source}", deg.round() as i32)
}

#[must_use]
pub fn out_text(reading: Option<&BeamformerReading>) -> String {
    match reading {
        Some(reading) if reading.out_rate > 0.0 => format!(
            "{:.3} MHz {}",
            reading.out_center_hz / 1e6,
            sample_rate(reading.out_rate)
        ),
        _ => "-".to_owned(),
    }
}

#[must_use]
pub fn gain_label(reading: Option<&BeamformerReading>) -> String {
    let Some(gain) = reading.and_then(|reading| reading.sinr_gain_db) else {
        return "-".to_owned();
    };
    let sign = if gain > 0.0 { "+" } else { "" };
    format!("{sign}{gain:.1} dB")
}

#[must_use]
pub fn snr_label(snr_db: Option<f32>) -> String {
    snr_db.map_or_else(|| "-".to_owned(), |snr| format!("{snr:.0} dB"))
}

#[must_use]
pub fn angle_gap(a: f64, b: f64) -> f64 {
    let gap = (wrap_deg(a) - wrap_deg(b)).abs();
    gap.min(360.0 - gap)
}

#[must_use]
pub fn null_close(deg: f64, steer: Option<f32>) -> bool {
    steer.is_some_and(|steer| angle_gap(deg, f64::from(steer)) < NULL_CLOSE_DEG)
}

#[must_use]
pub fn beam_chips(reading: Option<&BeamformerReading>) -> Vec<Chip> {
    let Some(reading) = reading else {
        return Vec::new();
    };
    let flags = [
        (
            reading.no_steer,
            "No steer",
            "No bearing to steer at".to_owned(),
        ),
        (
            reading.steer_stale,
            "Stale",
            "Last bearing is old".to_owned(),
        ),
        (
            reading.singular,
            "Singular",
            "Lanes too alike to solve".to_owned(),
        ),
        (
            reading.resets > 0 || reading.diverged,
            "Reset",
            format!("Weights restarted {} times", reading.resets),
        ),
        (
            reading.band_full,
            "Band full",
            "Band wider than the lanes".to_owned(),
        ),
        (
            reading
                .nulls_deg
                .iter()
                .any(|deg| null_close(f64::from(*deg), reading.steer_deg)),
            "Null close",
            "A null sits next to the steer".to_owned(),
        ),
    ];
    flags
        .into_iter()
        .filter(|(raised, _, _)| *raised)
        .map(|(_, label, title)| Chip::warn(label, title))
        .collect()
}

#[must_use]
pub fn polar_point(deg: f64, reach: f64, centre: f64) -> (f64, f64) {
    let radians = deg * PI / 180.0;
    (
        centre + reach * radians.sin(),
        centre - reach * radians.cos(),
    )
}

#[must_use]
pub fn polar_tick(deg: f64, inner: f64, outer: f64, centre: f64) -> [(f64, f64); 2] {
    [
        polar_point(deg, inner, centre),
        polar_point(deg, outer, centre),
    ]
}

#[must_use]
pub fn pattern_points(pattern: &[u8], radius: f64, centre: f64) -> Vec<(f64, f64)> {
    let count = pattern.len() as f64;
    pattern
        .iter()
        .enumerate()
        .map(|(index, level)| {
            let deg = index as f64 * 360.0 / count;
            polar_point(deg, f64::from(*level) / 255.0 * radius, centre)
        })
        .collect()
}

#[must_use]
pub fn lane_options(lanes: usize) -> Vec<(u32, String)> {
    (0..lanes.max(1) as u32)
        .map(|lane| (lane, (lane + 1).to_string()))
        .collect()
}

fn others(main: u32, lanes: usize) -> Vec<u32> {
    (0..lanes as u32).filter(|lane| *lane != main).collect()
}

#[must_use]
pub fn reference_lanes(params: &BeamformerParams, lanes: usize) -> Vec<u32> {
    if params.reference_lanes.is_empty() {
        return others(params.main_lane, lanes);
    }
    params
        .reference_lanes
        .iter()
        .copied()
        .filter(|lane| *lane != params.main_lane)
        .collect()
}

#[must_use]
pub fn toggled_reference(params: &BeamformerParams, lane: u32, lanes: usize) -> Vec<u32> {
    let mut current = reference_lanes(params, lanes);
    if let Some(index) = current.iter().position(|found| *found == lane) {
        current.remove(index);
    } else {
        current.push(lane);
    }
    if current.is_empty() {
        return params.reference_lanes.clone();
    }
    if others(params.main_lane, lanes)
        .iter()
        .all(|other| current.contains(other))
    {
        return Vec::new();
    }
    current.sort_unstable();
    current
}

#[must_use]
pub fn refs_value(refs: &[u32]) -> String {
    if refs.is_empty() {
        return "none".to_owned();
    }
    refs.iter()
        .map(|lane| (lane + 1).to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn set_main(params: &mut BeamformerParams, main: u32) {
    params.main_lane = main;
    params.reference_lanes.retain(|lane| *lane != main);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading() -> BeamformerReading {
        BeamformerReading {
            at: "2026-09-28T12:00:00Z".to_owned(),
            output_db: -42.0,
            ..BeamformerReading::default()
        }
    }

    fn weight(amplitude_db: f32, phase_deg: f32) -> LaneWeight {
        LaneWeight {
            amplitude_db,
            phase_deg,
        }
    }

    fn sorted(mut shown: Vec<BeamSetting>) -> Vec<BeamSetting> {
        shown.sort_by_key(|setting| *setting as u8);
        shown
    }

    #[test]
    fn every_mode_has_the_web_label() {
        let labels: Vec<_> = options(&MODES)
            .into_iter()
            .map(|(_, label)| label)
            .collect();
        assert_eq!(
            labels,
            ["MRC", "Beam", "MVDR", "Nulls", "GSC", "Cancel", "CMA"]
        );
        assert_eq!(mode_label(BeamMode::Canceller), "Cancel");
        assert_eq!(title_of(&MODES, BeamMode::Lcmv), "Steer with nulls");
        assert_eq!(
            title_of(&ADAPTATIONS, Adaptation::Rls),
            "Heavier, follows fast"
        );
        assert_eq!(options(&NOISE)[1].1, "White");
    }

    #[test]
    fn mrc_shows_noise_and_the_block_settings() {
        assert_eq!(
            sorted(visible_settings(&BeamformerParams::default())),
            sorted(vec![
                BeamSetting::Noise,
                BeamSetting::Crossfade,
                BeamSetting::Update,
                BeamSetting::CarryOver,
            ])
        );
    }

    #[test]
    fn steered_modes_show_steer_and_the_timeout_only_when_wired() {
        let lcmv = visible_settings(&BeamformerParams {
            mode: BeamMode::Lcmv,
            ..BeamformerParams::default()
        });
        assert!(lcmv.contains(&BeamSetting::Nulls));
        assert!(lcmv.contains(&BeamSetting::Steer));
        assert!(lcmv.contains(&BeamSetting::Timeout));
        let fixed = visible_settings(&BeamformerParams {
            mode: BeamMode::Mvdr,
            steer: SteerSource::Fixed {
                azimuth_deg: 10.0,
                elevation_deg: 0.0,
            },
            ..BeamformerParams::default()
        });
        assert!(fixed.contains(&BeamSetting::Steer));
        assert!(!fixed.contains(&BeamSetting::Timeout));
        let das = visible_settings(&BeamformerParams {
            mode: BeamMode::Das,
            ..BeamformerParams::default()
        });
        assert!(!das.contains(&BeamSetting::CarryOver));
    }

    #[test]
    fn the_canceller_shows_its_solver_only_with_several_taps() {
        let rls = visible_settings(&BeamformerParams {
            mode: BeamMode::Canceller,
            taps: 4,
            adaptation: Adaptation::Rls,
            ..BeamformerParams::default()
        });
        assert!(rls.contains(&BeamSetting::Forget));
        assert!(!rls.contains(&BeamSetting::Step));
        let single = visible_settings(&BeamformerParams {
            mode: BeamMode::Canceller,
            taps: 1,
            ..BeamformerParams::default()
        });
        assert!(!single.contains(&BeamSetting::Adaptation));
        assert!(single.contains(&BeamSetting::Main));
        assert!(single.contains(&BeamSetting::Update));
    }

    #[test]
    fn cma_shows_only_the_step() {
        assert_eq!(
            visible_settings(&BeamformerParams {
                mode: BeamMode::Cma,
                ..BeamformerParams::default()
            }),
            vec![BeamSetting::Step]
        );
    }

    #[test]
    fn weight_bars_scale_to_the_strongest_lane() {
        assert_eq!(
            amplitude_percent(&[weight(0.0, 0.0), weight(-6.0206, 10.0), weight(-20.0, 20.0)]),
            vec![100, 50, 10]
        );
        assert!(amplitude_percent(&[]).is_empty());
    }

    #[test]
    fn a_weight_reads_lane_level_and_phase() {
        assert_eq!(weight_title(0, weight(-1.24, 36.6)), "L1 -1.2 dB 37°");
    }

    #[test]
    fn nulls_stay_unique_wrapped_and_sorted() {
        assert_eq!(with_null(&[90.0, 45.0], 405.0), vec![45.0, 90.0]);
        assert_eq!(with_null(&[90.0], -10.0), vec![90.0, 350.0]);
        assert_eq!(with_null(&[10.04], 10.01), vec![10.0]);
        assert_eq!(with_null(&[10.0], f64::NAN), vec![10.0]);
        assert_eq!(without_null(&[45.0, 90.0], 45.0), vec![90.0]);
    }

    #[test]
    fn nulls_read_as_three_digit_azimuths() {
        assert_eq!(null_label(7.4), "007°");
        let mut params = BeamformerParams::default();
        assert_eq!(nulls_value(&params), "none");
        params.auto_nulls = true;
        assert_eq!(nulls_value(&params), "auto");
        params.nulls_deg = vec![45.0, 270.0];
        assert_eq!(nulls_value(&params), "045° 270° +auto");
    }

    #[test]
    fn the_steer_says_where_it_comes_from() {
        let mut found = reading();
        assert_eq!(steer_label(&found, SteerSource::Wired), "-");
        found.steer_deg = Some(137.4);
        assert_eq!(steer_label(&found, SteerSource::Wired), "137° DF");
        let fixed = SteerSource::Fixed {
            azimuth_deg: 90.0,
            elevation_deg: 0.0,
        };
        found.steer_deg = Some(90.0);
        assert_eq!(steer_label(&found, fixed), "90° fixed");
        assert_eq!(steer_value(fixed), "090°");
        assert_eq!(steer_value(SteerSource::Wired), "DF");
        assert_eq!(steer_title(false), "Beam direction. Wire a DF to steer");
    }

    #[test]
    fn picking_a_steer_kind_resets_only_on_a_change() {
        assert_eq!(steer_pick(SteerSource::Wired, SteerKind::Wired), None);
        assert_eq!(
            steer_pick(SteerSource::Wired, SteerKind::Fixed),
            Some(SteerSource::Fixed {
                azimuth_deg: 0.0,
                elevation_deg: 0.0
            })
        );
        let tilted = SteerSource::Fixed {
            azimuth_deg: 5.0,
            elevation_deg: 30.0,
        };
        assert_eq!(
            with_azimuth(tilted, 120.0),
            SteerSource::Fixed {
                azimuth_deg: 120.0,
                elevation_deg: 30.0
            }
        );
    }

    #[test]
    fn the_out_readout_names_the_beam_lane() {
        let mut found = reading();
        assert_eq!(out_text(Some(&found)), "-");
        assert_eq!(out_text(None), "-");
        found.out_center_hz = 433_920_000.0;
        found.out_rate = 250_000.0;
        assert_eq!(out_text(Some(&found)), "433.920 MHz 250 kS/s");
    }

    #[test]
    fn the_gain_carries_its_sign() {
        let mut found = reading();
        assert_eq!(gain_label(Some(&found)), "-");
        found.sinr_gain_db = Some(6.04);
        assert_eq!(gain_label(Some(&found)), "+6.0 dB");
        found.sinr_gain_db = Some(-2.0);
        assert_eq!(gain_label(Some(&found)), "-2.0 dB");
        assert_eq!(snr_label(Some(12.4)), "12 dB");
    }

    #[test]
    fn every_flag_raises_a_chip() {
        let found = BeamformerReading {
            no_steer: true,
            steer_stale: true,
            singular: true,
            resets: 2,
            band_full: true,
            steer_deg: Some(40.0),
            nulls_deg: vec![45.0],
            ..reading()
        };
        let labels: Vec<_> = beam_chips(Some(&found))
            .into_iter()
            .map(|chip| chip.label)
            .collect();
        assert_eq!(
            labels,
            [
                "No steer",
                "Stale",
                "Singular",
                "Reset",
                "Band full",
                "Null close"
            ]
        );
        let wrapped = BeamformerReading {
            steer_deg: Some(355.0),
            nulls_deg: vec![3.0],
            ..reading()
        };
        let labels: Vec<_> = beam_chips(Some(&wrapped))
            .into_iter()
            .map(|chip| chip.label)
            .collect();
        assert_eq!(labels, ["Null close"]);
        assert!(beam_chips(None).is_empty());
    }

    #[test]
    fn the_pattern_starts_forward_and_runs_clockwise() {
        let points = pattern_points(&[255, 128, 0, 128], 50.0, 60.0);
        assert_eq!(points.len(), 4);
        assert!((points[0].0 - 60.0).abs() < 1e-9);
        assert!((points[0].1 - 10.0).abs() < 1e-9);
        assert!(points[1].0 > 60.0);
        assert!((points[2].0 - 60.0).abs() < 1e-9 && (points[2].1 - 60.0).abs() < 1e-9);
        assert!(pattern_points(&[], 50.0, 60.0).is_empty());
    }

    #[test]
    fn a_tick_runs_outward_along_its_azimuth() {
        let [inner, outer] = polar_tick(90.0, 10.0, 20.0, 50.0);
        assert!((inner.0 - 60.0).abs() < 1e-9 && (inner.1 - 50.0).abs() < 1e-9);
        assert!((outer.0 - 70.0).abs() < 1e-9 && (outer.1 - 50.0).abs() < 1e-9);
    }

    #[test]
    fn reference_lanes_toggle_and_fold_back_to_all_others() {
        let base = BeamformerParams {
            mode: BeamMode::Canceller,
            main_lane: 0,
            ..BeamformerParams::default()
        };
        assert_eq!(reference_lanes(&base, 4), vec![1, 2, 3]);
        let fewer = toggled_reference(&base, 2, 4);
        assert_eq!(fewer, vec![1, 3]);
        let back = BeamformerParams {
            reference_lanes: fewer,
            ..base.clone()
        };
        assert!(toggled_reference(&back, 2, 4).is_empty());
        let one = BeamformerParams {
            reference_lanes: vec![1],
            ..base
        };
        assert_eq!(toggled_reference(&one, 1, 4), vec![1]);
        assert_eq!(refs_value(&[1, 3]), "2 4");
        assert_eq!(refs_value(&[]), "none");
    }

    #[test]
    fn a_new_main_lane_leaves_the_references() {
        let mut params = BeamformerParams {
            reference_lanes: vec![1, 2],
            ..BeamformerParams::default()
        };
        set_main(&mut params, 2);
        assert_eq!(params.main_lane, 2);
        assert_eq!(params.reference_lanes, vec![1]);
        assert_eq!(lane_options(0), vec![(0, "1".to_owned())]);
        assert_eq!(lane_options(3).len(), 3);
    }
}
