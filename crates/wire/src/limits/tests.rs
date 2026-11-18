use serde_json::Value;

use super::*;
use crate::{
    ArrayCal, ArrayCalSource, ArrayGain, ArrayNode, FusionDecay, TriangulationParams,
    hunt::HuntSweepParams,
};

const WRITTEN: &str = include_str!("../../../../web/src/generated/limits.json");

fn parsed(text: &str) -> Value {
    serde_json::from_str(text).expect("limits json parses")
}

fn fusion(edit: impl FnOnce(&mut TriangulationParams)) -> Option<&'static str> {
    let mut params = TriangulationParams::default();
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

fn hunt(edit: impl FnOnce(&mut HuntSweepParams)) -> Option<&'static str> {
    checked(edit, HuntSweepParams::problem)
}

fn pilot(offset_hz: f64, bandwidth_hz: f64) -> bool {
    ArrayNode {
        cal: ArrayCal {
            source: ArrayCalSource::Pilot {
                offset_hz,
                bandwidth_hz,
            },
            ..ArrayCal::default()
        },
        ..ArrayNode::default()
    }
    .valid()
}

#[test]
fn limits_json_matches_the_wire_constants() {
    assert_eq!(
        parsed(WRITTEN),
        parsed(&generated().expect("limits serialize")),
        "run cargo xtask codegen"
    );
}

#[test]
fn only_an_open_bound_names_itself() {
    let closed = serde_json::to_value(Bounds::new(50, 2_000)).expect("bounds serialize");
    assert_eq!(closed, serde_json::json!({"min": 50, "max": 2000}));
    let open = serde_json::to_value(Bounds::above(0.0, 1.0)).expect("bounds serialize");
    assert_eq!(open["above"], true);
    let json = parsed(&generated().expect("limits serialize"));
    assert_eq!(json["fusion"]["fixed_half_life_s"], 60);
    assert_eq!(json["light_speed_m_s"], 299_792_458.0);
    assert_eq!(
        json["range_doppler_db"],
        serde_json::json!({"min": -3.0, "max": 30.0})
    );
    assert_eq!(json["band_seed_hz"], 200_000.0);
    assert_eq!(json["spectrum_signal_margin_db"], 10.0);
    assert_eq!(json["scan"]["targets"], crate::MAX_SCAN_TARGETS);
}

#[test]
fn the_published_limits_name_their_wire_constants() {
    let json = parsed(&generated().expect("limits serialize"));
    assert_eq!(json["filter"]["kinds"], crate::filter::MAX_FILTER_KINDS);
    assert_eq!(
        json["filter"]["text_len"],
        crate::filter::MAX_FILTER_TEXT_LEN
    );
    assert_eq!(
        json["patch"]["channels_per_radio"],
        crate::patch::MAX_CHANNELS_PER_RADIO
    );
    assert_eq!(
        json["patch"]["monitors_per_radio"],
        crate::patch::MAX_MONITORS_PER_RADIO
    );
    assert_eq!(json["audio"]["rate_hz"], crate::audio::AUDIO_RATE_HZ);
    assert_eq!(json["audio"]["notches"], crate::audio::MAX_AUDIO_NOTCHES);
    assert_eq!(
        json["audio"]["blanker_threshold_seed"],
        crate::audio::DEFAULT_BLANKER_THRESHOLD
    );
    assert_eq!(
        json["audio"]["click_threshold_seed"],
        crate::audio::DEFAULT_CLICK_THRESHOLD
    );
    assert_eq!(
        json["audio"]["squelch_auto_margin_seed_db"],
        crate::channel::DEFAULT_SQUELCH_AUTO_MARGIN_DB
    );
    assert_eq!(
        json["audio"]["denoise_strength_seed"],
        crate::audio::DEFAULT_DENOISE_STRENGTH
    );
    assert_eq!(
        json["audio"]["filter_low_seed_hz"],
        crate::audio::DEFAULT_FILTER_LOW_HZ
    );
    assert_eq!(
        json["audio"]["filter_high_seed_hz"],
        crate::audio::DEFAULT_FILTER_HIGH_HZ
    );
    assert_eq!(
        json["audio"]["notch_freq_seed_hz"],
        crate::audio::DEFAULT_NOTCH_FREQ_HZ
    );
    assert_eq!(
        json["audio"]["notch_width_seed_hz"],
        crate::audio::DEFAULT_NOTCH_WIDTH_HZ
    );
    assert_eq!(
        json["audio"]["peak_gain_seed_db"],
        crate::audio::DEFAULT_PEAK_GAIN_DB
    );
    assert_eq!(
        json["channel_frequency_seed_hz"],
        crate::channel::DEFAULT_FREQUENCY_HZ
    );
    assert_eq!(json["survey"]["cell_m"], crate::survey::SURVEY_CELL_M);
    assert_eq!(
        json["survey"]["metres_per_degree_lon"],
        crate::survey::METRES_PER_DEGREE_LON
    );
    assert_eq!(
        json["survey"]["metres_per_degree_lat"],
        crate::survey::METRES_PER_DEGREE_LAT
    );
    assert_eq!(json["survey"]["min_cos_lat"], crate::survey::MIN_COS_LAT);
    assert_eq!(json["survey"]["cells"], crate::survey::MAX_SURVEY_CELLS);
    assert_eq!(
        json["workspace_state_version"],
        crate::workspace_state::WORKSPACE_STATE_VERSION
    );
}

#[test]
fn the_audio_seeds_sit_inside_the_published_bounds() {
    let limits = AUDIO_LIMITS;
    assert!(
        limits
            .blanker_threshold
            .contains(limits.blanker_threshold_seed)
    );
    assert!(limits.click_threshold.contains(limits.click_threshold_seed));
    assert!(
        limits
            .squelch_auto_margin_db
            .contains(limits.squelch_auto_margin_seed_db)
    );
    assert!(limits.notch_width_hz.contains(limits.notch_width_seed_hz));
    assert!(limits.tone_hz.contains(limits.notch_freq_seed_hz));
    assert!(limits.tone_hz.contains(limits.filter_low_seed_hz));
    assert!(limits.tone_hz.contains(limits.filter_high_seed_hz));
    assert!(limits.tone_hz.contains(limits.peak_freq_seed_hz));
    assert!(limits.notch_width_hz.contains(limits.peak_width_seed_hz));
    assert!(limits.peak_gain_db.contains(limits.peak_gain_seed_db));
}

#[test]
fn bounds_hold_their_edges() {
    let closed = Bounds::new(1.0_f32, 2.0);
    assert!(closed.contains(1.0) && closed.contains(2.0));
    assert!(!closed.contains(0.99) && !closed.contains(2.01) && !closed.contains(f32::NAN));
    let open = Bounds::above(0.0_f32, 1.0);
    assert!(!open.contains(0.0) && open.contains(f32::MIN_POSITIVE) && open.contains(1.0));
}

#[test]
fn fusion_validation_follows_the_limits() {
    let limits = FUSION_LIMITS;
    for seconds in [
        limits.half_life_s.min,
        limits.half_life_seed_s,
        limits.half_life_s.max,
    ] {
        assert_eq!(
            fusion(|p| p.decay = FusionDecay::HalfLife { seconds }),
            None
        );
    }
    assert!(
        fusion(|p| p.decay = FusionDecay::HalfLife {
            seconds: limits.half_life_s.max + 1
        })
        .is_some()
    );
    assert!(fusion(|p| p.extent_km = limits.extent_km.max + 0.1).is_some());
    assert_eq!(fusion(|p| p.probe_km = limits.probe_km.min), None);
    assert!(fusion(|p| p.min_confidence = limits.min_confidence.max + 0.1).is_some());
    assert_eq!(fusion(|p| p.max_emitters = limits.emitters.max), None);
    assert!(fusion(|p| p.max_emitters = limits.emitters.max + 1).is_some());
}

#[test]
fn array_limits_follow_the_validation() {
    let limits = ARRAY_LIMITS;
    assert!(pilot(limits.cal_offset_hz.min, limits.cal_bandwidth_hz.max));
    assert!(pilot(
        limits.cal_offset_hz.max,
        limits.cal_bandwidth_seed_hz
    ));
    assert!(!pilot(0.0, limits.cal_bandwidth_hz.min - 1.0));
    for db in [limits.gain_db.min, limits.gain_db.max] {
        assert_eq!(ArrayGain::Manual { db }.problem(), None);
    }
    assert!(
        ArrayGain::Manual {
            db: limits.gain_db.max + 0.5
        }
        .problem()
        .is_some()
    );
}

#[test]
fn hunt_sweep_validation_follows_the_limits() {
    let limits = HUNT_LIMITS;
    assert_eq!(hunt(|p| p.beamwidth_deg = limits.beamwidth_deg.min), None);
    assert!(hunt(|p| p.beamwidth_deg = limits.beamwidth_deg.max + 1.0).is_some());
    assert_eq!(hunt(|p| p.front_back_db = limits.front_back_db.max), None);
    assert!(hunt(|p| p.min_span_deg = limits.min_span_deg.min - 1.0).is_some());
    assert_eq!(
        hunt(|p| p.min_contrast_db = limits.min_contrast_db.max),
        None
    );
    assert!(hunt(|p| p.mount_offset_deg = limits.mount_offset_deg.max + 1.0).is_some());
    assert_eq!(
        hunt(|p| p.mount_offset_deg = limits.mount_offset_deg.min),
        None
    );
}
