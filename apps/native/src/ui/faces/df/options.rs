use sdrmm_wire::{
    limits::DF_LIMITS as LIMITS,
    processor::df::{DfAlgorithm, SourceRule, UlaSide},
};

pub const NEEDS_LINE_OR_CIRCLE: &str = "Needs a line or circle";
pub const TOO_MANY_SOURCES: &str = "Too many sources";
pub const AUTO_SOURCES: u32 = 0;
const KHZ: f64 = 1_000.0;
const OFF: &str = "off";

pub struct AlgorithmInfo {
    pub value: DfAlgorithm,
    pub label: &'static str,
    pub title: &'static str,
    pub structured: bool,
}

pub const DF_ALGORITHMS: [AlgorithmInfo; 5] = [
    AlgorithmInfo {
        value: DfAlgorithm::Bartlett,
        label: "Bartlett",
        title: "Plain beam scan, robust",
        structured: false,
    },
    AlgorithmInfo {
        value: DfAlgorithm::Capon,
        label: "Capon",
        title: "Sharper beam scan",
        structured: false,
    },
    AlgorithmInfo {
        value: DfAlgorithm::Music,
        label: "MUSIC",
        title: "Subspace scan, splits close sources",
        structured: false,
    },
    AlgorithmInfo {
        value: DfAlgorithm::RootMusic,
        label: "Root-MUSIC",
        title: "Grid-free MUSIC",
        structured: true,
    },
    AlgorithmInfo {
        value: DfAlgorithm::Esprit,
        label: "ESPRIT",
        title: "Grid-free rotation fit",
        structured: true,
    },
];

fn info(algorithm: DfAlgorithm) -> Option<&'static AlgorithmInfo> {
    DF_ALGORITHMS.iter().find(|entry| entry.value == algorithm)
}

#[must_use]
pub fn algorithm_options(structured_ok: bool, current: DfAlgorithm) -> Vec<(DfAlgorithm, String)> {
    DF_ALGORITHMS
        .iter()
        .filter(|entry| structured_ok || !entry.structured || entry.value == current)
        .map(|entry| (entry.value, entry.label.to_owned()))
        .collect()
}

#[must_use]
pub fn algorithm_title(algorithm: DfAlgorithm, structured_ok: bool) -> &'static str {
    match info(algorithm) {
        Some(entry) if entry.structured && !structured_ok => NEEDS_LINE_OR_CIRCLE,
        Some(entry) => entry.title,
        None => "Method",
    }
}

#[must_use]
pub fn smoothing_title(structured_ok: bool) -> &'static str {
    if structured_ok {
        "Spatial smoothing for reflections, 0 off"
    } else {
        NEEDS_LINE_OR_CIRCLE
    }
}

#[must_use]
pub fn smoothing_locked(structured_ok: bool, smoothing: u8) -> bool {
    !structured_ok && smoothing == 0
}

fn sources_fit(lanes: u32) -> u32 {
    lanes
        .saturating_sub(1)
        .clamp(LIMITS.sources.min, LIMITS.sources.max)
}

#[must_use]
pub fn source_options(lanes: u32, stored: Option<u32>) -> Vec<(u32, String)> {
    let top = sources_fit(lanes).max(stored.unwrap_or(0));
    std::iter::once((AUTO_SOURCES, "Auto".to_owned()))
        .chain((1..=top).map(|count| (count, count.to_string())))
        .collect()
}

#[must_use]
pub fn sources_title(lanes: u32, stored: Option<u32>) -> &'static str {
    match stored {
        None => "Count sources from the signal",
        Some(count) if count > sources_fit(lanes) => TOO_MANY_SOURCES,
        Some(_) => "Sources",
    }
}

#[must_use]
pub fn rule_options() -> Vec<(SourceRule, String)> {
    vec![
        (SourceRule::Dominance, "Dominance".to_owned()),
        (SourceRule::Mdl, "MDL".to_owned()),
    ]
}

#[must_use]
pub fn rule_title(rule: SourceRule) -> &'static str {
    match rule {
        SourceRule::Dominance => "Count sources clearly above the rest",
        SourceRule::Mdl => "Information criterion",
    }
}

#[must_use]
pub fn peak_options() -> Vec<(u8, String)> {
    (LIMITS.peaks.min..=LIMITS.peaks.max)
        .map(|count| (count, count.to_string()))
        .collect()
}

#[must_use]
pub fn side_options() -> Vec<(UlaSide, String)> {
    vec![
        (UlaSide::Both, "Both".to_owned()),
        (UlaSide::Front, "Front".to_owned()),
        (UlaSide::Back, "Back".to_owned()),
    ]
}

#[must_use]
pub fn side_title(side: UlaSide) -> &'static str {
    match side {
        UlaSide::Both => "Report front and mirror",
        UlaSide::Front => "Only the side ahead of the line",
        UlaSide::Back => "Only the side behind the line",
    }
}

#[must_use]
pub fn is_structured(algorithm: DfAlgorithm, smoothing: u8) -> bool {
    smoothing > 0 || info(algorithm).is_some_and(|entry| entry.structured)
}

#[must_use]
pub fn elevation_block(
    collinear: bool,
    algorithm: DfAlgorithm,
    smoothing: u8,
) -> Option<&'static str> {
    if collinear {
        Some("Needs a 2D array")
    } else if is_structured(algorithm, smoothing) {
        Some("Needs a grid method")
    } else {
        None
    }
}

#[must_use]
pub fn elevation_locked(block: Option<&str>, on: bool) -> bool {
    block.is_some() && !on
}

#[must_use]
pub fn to_khz(hz: f64) -> f64 {
    hz / KHZ
}

#[must_use]
pub fn from_khz(khz: f64) -> f64 {
    (khz * KHZ).round()
}

#[must_use]
pub fn squelch_text(db: f32) -> String {
    if db == 0.0 {
        OFF.to_owned()
    } else {
        crate::ui::kit_raster::shown(f64::from(db))
    }
}

pub fn read_squelch(text: &str) -> Result<f32, String> {
    let text = text.trim();
    if text.eq_ignore_ascii_case(OFF) {
        return Ok(0.0);
    }
    let bounds = LIMITS.squelch_db;
    let value: f32 = text
        .parse()
        .map_err(|_| format!("{text} is not a number"))?;
    if bounds.contains(value) {
        Ok(value)
    } else {
        Err(format!("{OFF} or {} to {}", bounds.min, bounds.max))
    }
}

pub fn station_id(text: &str) -> Result<Option<String>, String> {
    let trimmed = text.trim();
    if trimmed.len() > LIMITS.station_len {
        return Err(format!("at most {} characters", LIMITS.station_len));
    }
    Ok((!trimmed.is_empty()).then(|| trimmed.to_owned()))
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::processor::df::DfParams;

    use super::*;

    fn values(options: &[(DfAlgorithm, String)]) -> Vec<DfAlgorithm> {
        options.iter().map(|(value, _)| *value).collect()
    }

    #[test]
    fn grid_free_methods_are_offered_only_when_the_geometry_allows() {
        assert_eq!(algorithm_options(true, DfAlgorithm::Music).len(), 5);
        assert_eq!(
            values(&algorithm_options(false, DfAlgorithm::Music)),
            [
                DfAlgorithm::Bartlett,
                DfAlgorithm::Capon,
                DfAlgorithm::Music
            ]
        );
        assert!(
            values(&algorithm_options(false, DfAlgorithm::Esprit)).contains(&DfAlgorithm::Esprit)
        );
        assert_eq!(
            algorithm_title(DfAlgorithm::Esprit, false),
            NEEDS_LINE_OR_CIRCLE
        );
        assert_eq!(
            algorithm_title(DfAlgorithm::Capon, false),
            "Sharper beam scan"
        );
        assert_eq!(smoothing_title(false), NEEDS_LINE_OR_CIRCLE);
        assert_ne!(smoothing_title(true), NEEDS_LINE_OR_CIRCLE);
        assert!(smoothing_locked(false, 0));
        assert!(!smoothing_locked(false, 2));
    }

    #[test]
    fn sources_count_up_to_one_less_than_the_lanes_and_keep_a_stored_count() {
        let labels = |options: Vec<(u32, String)>| {
            options
                .into_iter()
                .map(|(_, label)| label)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            labels(source_options(5, None)),
            ["Auto", "1", "2", "3", "4"]
        );
        assert_eq!(labels(source_options(0, None)), ["Auto", "1"]);
        assert_eq!(source_options(16, None).len(), 16);
        let kept = source_options(3, Some(4));
        assert_eq!(
            kept.iter().map(|(value, _)| *value).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4]
        );
        assert_eq!(sources_title(3, Some(4)), TOO_MANY_SOURCES);
        assert_eq!(sources_title(3, Some(2)), "Sources");
        assert_ne!(sources_title(3, None), TOO_MANY_SOURCES);
    }

    #[test]
    fn elevation_is_refused_on_a_line_and_with_a_grid_free_method_or_smoothing() {
        assert_eq!(
            elevation_block(true, DfAlgorithm::Music, 0),
            Some("Needs a 2D array")
        );
        assert_eq!(
            elevation_block(false, DfAlgorithm::Esprit, 0),
            Some("Needs a grid method")
        );
        assert_eq!(
            elevation_block(false, DfAlgorithm::Music, 2),
            Some("Needs a grid method")
        );
        assert_eq!(elevation_block(false, DfAlgorithm::Capon, 0), None);
        assert!(elevation_locked(Some("Needs a 2D array"), false));
        assert!(!elevation_locked(Some("Needs a 2D array"), true));
        assert!(!elevation_locked(None, false));
    }

    #[test]
    fn every_choice_list_covers_its_limits() {
        assert_eq!(peak_options().len(), usize::from(LIMITS.peaks.max));
        assert_eq!(rule_options().len(), 2);
        assert_eq!(side_options().len(), 3);
        assert!(!rule_title(SourceRule::Mdl).is_empty());
        assert!(!side_title(UlaSide::Front).is_empty());
    }

    #[test]
    fn squelch_reads_off_at_zero() {
        assert_eq!(squelch_text(0.0), "off");
        assert_eq!(squelch_text(6.5), "6.5");
        assert_eq!(read_squelch("off"), Ok(0.0));
        assert_eq!(read_squelch(" 12 "), Ok(12.0));
        assert!(read_squelch("99").is_err());
        assert!(read_squelch("loud").is_err());
    }

    #[test]
    fn band_edits_round_to_whole_hertz() {
        assert!((to_khz(20_000.0) - 20.0).abs() < 1e-9);
        assert!((from_khz(12.3456) - 12_346.0).abs() < 1e-9);
    }

    #[test]
    fn a_blank_station_is_unnamed_and_a_long_one_is_refused() {
        assert_eq!(station_id("  "), Ok(None));
        assert_eq!(station_id(" roof "), Ok(Some("roof".to_owned())));
        assert!(station_id(&"x".repeat(LIMITS.station_len + 1)).is_err());
    }

    #[test]
    fn the_default_finder_is_one_the_server_accepts() {
        assert!(DfParams::default().valid());
    }
}
