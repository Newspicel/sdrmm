use sdrmm_wire::{
    array::ProcessorGate,
    processor::df::{DfPeak, DfReading},
};

use crate::ui::faces::processor::Chip;

pub const NEEDS_HEADING: &str = "Needs heading";
const MAX_WEDGE_DEG: f64 = 179.9;

pub const COMPASS_MARKS: [(f64, &str); 8] = [
    (0.0, "N"),
    (45.0, "NE"),
    (90.0, "E"),
    (135.0, "SE"),
    (180.0, "S"),
    (225.0, "SW"),
    (270.0, "W"),
    (315.0, "NW"),
];

pub const TRUE_MARKS: [(f64, &str); 4] = [(0.0, "N"), (90.0, "E"), (180.0, "S"), (270.0, "W")];
pub const RELATIVE_MARKS: [(f64, &str); 4] = [(0.0, "F"), (90.0, "R"), (180.0, "B"), (270.0, "L")];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BearingFrame {
    Relative,
    True,
}

#[must_use]
pub fn frame_options(true_available: bool) -> Vec<(BearingFrame, &'static str)> {
    let mut options = vec![(BearingFrame::Relative, "Rel")];
    if true_available {
        options.push((BearingFrame::True, "True"));
    }
    options
}

#[must_use]
pub fn frame_title(true_available: bool) -> &'static str {
    if true_available {
        "Relative to the array or from true north"
    } else {
        NEEDS_HEADING
    }
}

#[must_use]
pub fn shown_frame(picked: Option<BearingFrame>, true_available: bool) -> BearingFrame {
    if true_available {
        picked.unwrap_or(BearingFrame::True)
    } else {
        BearingFrame::Relative
    }
}

#[must_use]
pub fn rose_letters(frame: BearingFrame) -> &'static [(f64, &'static str)] {
    match frame {
        BearingFrame::True => &TRUE_MARKS,
        BearingFrame::Relative => &RELATIVE_MARKS,
    }
}

#[must_use]
pub fn frame_rotation(reading: Option<&DfReading>, frame: BearingFrame) -> f64 {
    match frame {
        BearingFrame::True => reading.and_then(|found| found.azimuth_deg).unwrap_or(0.0),
        BearingFrame::Relative => 0.0,
    }
}

#[must_use]
pub fn heading_tick(reading: Option<&DfReading>, frame: BearingFrame) -> Option<f64> {
    (frame == BearingFrame::True)
        .then(|| reading.and_then(|found| found.azimuth_deg))
        .flatten()
}

#[must_use]
pub fn polar_point(bearing_deg: f64, radius: f64, centre: f64) -> (f64, f64) {
    let angle = (bearing_deg - 90.0).to_radians();
    (centre + radius * angle.cos(), centre + radius * angle.sin())
}

#[must_use]
pub fn spectrum_points(
    spectrum: &[u8],
    centre: f64,
    inner: f64,
    outer: f64,
    rotate_deg: f64,
) -> Vec<(f64, f64)> {
    let points = spectrum.len() as f64;
    spectrum
        .iter()
        .enumerate()
        .map(|(index, level)| {
            let radius = inner + (outer - inner) * f64::from(*level) / 255.0;
            polar_point(index as f64 * 360.0 / points + rotate_deg, radius, centre)
        })
        .collect()
}

#[must_use]
pub fn peak_deg(peak: &DfPeak, frame: BearingFrame) -> Option<f64> {
    match frame {
        BearingFrame::True => peak.true_deg.map(f64::from),
        BearingFrame::Relative => Some(f64::from(peak.relative_deg)),
    }
}

fn mirror_deg(peak: &DfPeak, frame: BearingFrame) -> Option<f64> {
    match frame {
        BearingFrame::True => peak.mirror_true_deg,
        BearingFrame::Relative => peak.mirror_deg,
    }
    .map(f64::from)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeedleWeight {
    Primary,
    Secondary,
    Mirror,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Needle {
    pub deg: f64,
    pub weight: NeedleWeight,
}

#[must_use]
pub fn rose_needles(peaks: &[DfPeak], frame: BearingFrame) -> Vec<Needle> {
    let mut needles = Vec::new();
    for (rank, peak) in peaks.iter().enumerate() {
        if let Some(deg) = peak_deg(peak, frame) {
            let weight = if rank == 0 {
                NeedleWeight::Primary
            } else {
                NeedleWeight::Secondary
            };
            needles.push(Needle { deg, weight });
        }
        if let Some(deg) = mirror_deg(peak, frame) {
            needles.push(Needle {
                deg,
                weight: NeedleWeight::Mirror,
            });
        }
    }
    distinct_needles(needles)
}

#[must_use]
pub fn distinct_needles(needles: Vec<Needle>) -> Vec<Needle> {
    let same = |a: &Needle, b: &Needle| {
        a.weight == b.weight && (a.deg * 100.0).round() == (b.deg * 100.0).round()
    };
    let mut kept: Vec<Needle> = Vec::with_capacity(needles.len());
    for needle in needles {
        if !kept.iter().any(|held| same(held, &needle)) {
            kept.push(needle);
        }
    }
    kept
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wedge {
    pub deg: f64,
    pub sigma_deg: f64,
}

#[must_use]
pub fn rose_wedge(peaks: &[DfPeak], frame: BearingFrame) -> Option<Wedge> {
    let first = peaks.first()?;
    Some(Wedge {
        deg: peak_deg(first, frame)?,
        sigma_deg: f64::from(first.sigma_deg),
    })
}

#[must_use]
pub fn wedge_points(wedge: Wedge, centre: f64, radius: f64) -> Vec<(f64, f64)> {
    let half = wedge.sigma_deg.clamp(0.0, MAX_WEDGE_DEG);
    let steps = (half * 2.0).ceil().max(1.0) as usize;
    let start = wedge.deg - half;
    let arc = (0..=steps).map(|step| {
        polar_point(
            start + 2.0 * half * step as f64 / steps as f64,
            radius,
            centre,
        )
    });
    std::iter::once((centre, centre)).chain(arc).collect()
}

#[must_use]
pub fn bearing_label(deg: impl Into<f64>) -> String {
    let wrapped = deg.into().round().rem_euclid(360.0);
    format!("{:03}\u{b0}", wrapped as u32 % 360)
}

#[must_use]
pub fn peak_text(peak: &DfPeak, frame: BearingFrame) -> String {
    peak_deg(peak, frame).map_or_else(|| "-".to_owned(), bearing_label)
}

#[must_use]
pub fn sigma_text(sigma_deg: f32) -> String {
    if sigma_deg < 1.0 {
        format!("{sigma_deg:.1}\u{b0}")
    } else {
        format!("{}\u{b0}", sigma_deg.round())
    }
}

#[must_use]
pub fn percent_text(fraction: f32) -> String {
    format!("{}%", (fraction.clamp(0.0, 1.0) * 100.0).round())
}

#[must_use]
pub fn peak_summary(peak: &DfPeak, frame: BearingFrame) -> String {
    format!(
        "{} {} \u{b1}{}",
        peak_text(peak, frame),
        percent_text(peak.confidence),
        sigma_text(peak.sigma_deg)
    )
}

#[must_use]
pub fn readout_rows(
    reading: Option<&DfReading>,
    frame: BearingFrame,
    age: String,
) -> Vec<(String, String)> {
    let first = reading.and_then(|found| found.peaks.first());
    let dash = || "-".to_owned();
    let mut rows = vec![
        (
            "Bearing".to_owned(),
            first.map_or_else(dash, |peak| peak_text(peak, frame)),
        ),
        (
            "\u{b1}".to_owned(),
            first.map_or_else(dash, |peak| sigma_text(peak.sigma_deg)),
        ),
        (
            "Fit".to_owned(),
            reading.map_or_else(dash, |found| percent_text(found.fit)),
        ),
        (
            "SNR".to_owned(),
            reading.map_or_else(dash, |found| format!("{} dB", found.snr_db.round())),
        ),
        ("Src".to_owned(), reading.map_or_else(dash, sources_text)),
        ("Age".to_owned(), age),
    ];
    let others = reading.map_or(&[][..], |found| found.peaks.get(1..).unwrap_or_default());
    rows.extend(
        others
            .iter()
            .enumerate()
            .map(|(index, peak)| (format!("#{}", index + 2), peak_summary(peak, frame))),
    );
    rows
}

fn sources_text(reading: &DfReading) -> String {
    let rule = if reading.sources_auto {
        "auto"
    } else {
        "fixed"
    };
    format!("{} {rule}", reading.sources)
}

struct ReadingChip {
    on: fn(&DfReading) -> bool,
    label: &'static str,
    title: &'static str,
}

const READING_CHIPS: [ReadingChip; 9] = [
    ReadingChip {
        on: |reading| reading.squelched,
        label: "Squelch",
        title: "Peak below squelch",
    },
    ReadingChip {
        on: |reading| reading.azimuth_deg.is_none(),
        label: "No heading",
        title: "Wire a GPS with heading",
    },
    ReadingChip {
        on: |reading| reading.station.is_none(),
        label: "No position",
        title: "Wire a GPS to the array to send bearings",
    },
    ReadingChip {
        on: |reading| reading.aliasing,
        label: "Aliased",
        title: "Elements more than half a wavelength apart",
    },
    ReadingChip {
        on: |reading| reading.mode_aliasing,
        label: "Mode alias",
        title: "Too high for phase modes on this circle",
    },
    ReadingChip {
        on: |reading| reading.mirror,
        label: "Mirror",
        title: "A line array cannot tell front from back",
    },
    ReadingChip {
        on: |reading| reading.rotating,
        label: "Rotating",
        title: "Turning too fast, blocks skipped",
    },
    ReadingChip {
        on: |reading| reading.singular,
        label: "Singular",
        title: "Covariance could not be inverted",
    },
    ReadingChip {
        on: |reading| reading.table_out_of_range,
        label: "Table off",
        title: "Outside the calibration table",
    },
];

#[must_use]
pub fn df_chips(
    reading: Option<&DfReading>,
    gate: Option<ProcessorGate>,
    stale: bool,
) -> Vec<Chip> {
    let mut chips = Vec::new();
    if let Some(gate) = gate {
        chips.push(Chip::danger(gate.label(), "The array holds this finder"));
    }
    if stale {
        chips.push(Chip::danger("Stale", "No reading for three report periods"));
    }
    if let Some(reading) = reading {
        chips.extend(
            READING_CHIPS
                .iter()
                .filter(|chip| (chip.on)(reading))
                .map(|chip| Chip::warn(chip.label, chip.title)),
        );
    }
    chips
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::geo::LatLon;

    use super::*;

    fn peak() -> DfPeak {
        DfPeak {
            relative_deg: 47.0,
            true_deg: Some(137.0),
            power_db: -40.0,
            confidence: 0.82,
            sigma_deg: 3.1,
            ..DfPeak::default()
        }
    }

    fn reading() -> DfReading {
        DfReading {
            peaks: vec![peak()],
            azimuth_deg: Some(90.0),
            station: Some(LatLon {
                lat: 52.0,
                lon: 13.0,
            }),
            sources: 1,
            sources_auto: true,
            ..DfReading::default()
        }
    }

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
    }

    #[test]
    fn puts_north_at_the_top_and_runs_clockwise() {
        assert!(close(polar_point(0.0, 50.0, 100.0), (100.0, 50.0)));
        assert!(close(polar_point(90.0, 50.0, 100.0), (150.0, 100.0)));
        assert!((polar_point(180.0, 50.0, 100.0).1 - 150.0).abs() < 1e-6);
    }

    #[test]
    fn the_spectrum_turns_with_the_array_azimuth() {
        let spectrum = [255, 0, 0, 0];
        let rotation = frame_rotation(Some(&reading()), BearingFrame::True);
        assert!((rotation - 90.0).abs() < 1e-9);
        assert!(close(
            spectrum_points(&spectrum, 50.0, 10.0, 40.0, rotation)[0],
            (90.0, 50.0)
        ));
        let still = frame_rotation(Some(&reading()), BearingFrame::Relative);
        assert!(close(
            spectrum_points(&spectrum, 50.0, 10.0, 40.0, still)[0],
            (50.0, 10.0)
        ));
        assert!(spectrum_points(&[], 50.0, 10.0, 40.0, 0.0).is_empty());
    }

    #[test]
    fn true_is_offered_and_picked_only_with_a_heading() {
        assert_eq!(frame_options(false), vec![(BearingFrame::Relative, "Rel")]);
        assert_eq!(frame_options(true).len(), 2);
        assert_eq!(frame_title(false), NEEDS_HEADING);
        assert_eq!(shown_frame(None, true), BearingFrame::True);
        assert_eq!(
            shown_frame(Some(BearingFrame::Relative), true),
            BearingFrame::Relative
        );
        assert_eq!(
            shown_frame(Some(BearingFrame::True), false),
            BearingFrame::Relative
        );
    }

    #[test]
    fn the_letters_switch_with_the_frame() {
        let labels = |frame| {
            rose_letters(frame)
                .iter()
                .map(|(_, label)| *label)
                .collect::<Vec<_>>()
        };
        assert_eq!(labels(BearingFrame::Relative), ["F", "R", "B", "L"]);
        assert_eq!(labels(BearingFrame::True), ["N", "E", "S", "W"]);
        assert_eq!(
            heading_tick(Some(&reading()), BearingFrame::True),
            Some(90.0)
        );
        assert_eq!(heading_tick(Some(&reading()), BearingFrame::Relative), None);
    }

    #[test]
    fn each_peak_gets_a_needle_and_a_mirror_in_the_chosen_frame() {
        let first = DfPeak {
            mirror_deg: Some(313.0),
            mirror_true_deg: Some(43.0),
            ..peak()
        };
        let second = DfPeak {
            relative_deg: 200.0,
            true_deg: Some(290.0),
            confidence: 0.4,
            sigma_deg: 0.42,
            ..peak()
        };
        let needles = rose_needles(&[first.clone(), second.clone()], BearingFrame::True);
        assert_eq!(
            needles,
            vec![
                Needle {
                    deg: 137.0,
                    weight: NeedleWeight::Primary
                },
                Needle {
                    deg: 43.0,
                    weight: NeedleWeight::Mirror
                },
                Needle {
                    deg: 290.0,
                    weight: NeedleWeight::Secondary
                },
            ]
        );
        assert_eq!(peak_text(&first, BearingFrame::True), "137\u{b0}");
        assert_eq!(peak_text(&first, BearingFrame::Relative), "047\u{b0}");
        let unheaded = DfPeak {
            true_deg: None,
            ..peak()
        };
        assert_eq!(peak_text(&unheaded, BearingFrame::True), "-");
        assert_eq!(
            peak_summary(&second, BearingFrame::True),
            "290\u{b0} 40% \u{b1}0.4\u{b0}"
        );
    }

    #[test]
    fn identical_needles_are_drawn_once() {
        let twice = vec![
            Needle {
                deg: 10.001,
                weight: NeedleWeight::Mirror,
            },
            Needle {
                deg: 10.0,
                weight: NeedleWeight::Mirror,
            },
            Needle {
                deg: 10.0,
                weight: NeedleWeight::Primary,
            },
        ];
        assert_eq!(distinct_needles(twice).len(), 2);
    }

    #[test]
    fn the_wedge_spans_one_sigma_either_side_and_never_closes_the_circle() {
        let wedge = rose_wedge(&[peak()], BearingFrame::Relative);
        assert_eq!(
            wedge,
            Some(Wedge {
                deg: 47.0,
                sigma_deg: f64::from(3.1_f32)
            })
        );
        assert_eq!(rose_wedge(&[], BearingFrame::True), None);
        let points = wedge_points(
            Wedge {
                deg: 0.0,
                sigma_deg: 10.0,
            },
            100.0,
            50.0,
        );
        assert!(close(points[0], (100.0, 100.0)));
        assert!(close(points[1], polar_point(-10.0, 50.0, 100.0)));
        assert!(close(
            points[points.len() - 1],
            polar_point(10.0, 50.0, 100.0)
        ));
        let wide = wedge_points(
            Wedge {
                deg: 0.0,
                sigma_deg: 400.0,
            },
            100.0,
            50.0,
        );
        assert!(!close(wide[1], wide[wide.len() - 1]));
    }

    #[test]
    fn bearings_read_as_three_whole_degrees() {
        assert_eq!(bearing_label(359.7), "000\u{b0}");
        assert_eq!(bearing_label(-5.0), "355\u{b0}");
        assert_eq!(bearing_label(7.24_f32), "007\u{b0}");
        assert_eq!(sigma_text(3.1), "3\u{b0}");
        assert_eq!(sigma_text(0.42), "0.4\u{b0}");
        assert_eq!(percent_text(1.7), "100%");
    }

    #[test]
    fn the_readout_lists_the_first_peak_then_the_rest() {
        let mut found = reading();
        found.fit = 0.9;
        found.snr_db = 17.6;
        found.peaks.push(DfPeak {
            relative_deg: 200.0,
            true_deg: Some(290.0),
            ..peak()
        });
        let rows = readout_rows(Some(&found), BearingFrame::True, "1.0 s".to_owned());
        let value = |label: &str| {
            rows.iter()
                .find(|(name, _)| name == label)
                .map(|(_, value)| value.clone())
        };
        assert_eq!(value("Bearing").as_deref(), Some("137\u{b0}"));
        assert_eq!(value("Fit").as_deref(), Some("90%"));
        assert_eq!(value("SNR").as_deref(), Some("18 dB"));
        assert_eq!(value("Src").as_deref(), Some("1 auto"));
        assert!(value("#2").is_some_and(|text| text.starts_with("290")));
        let empty = readout_rows(None, BearingFrame::Relative, "-".to_owned());
        assert_eq!(empty.len(), 6);
        assert!(empty.iter().all(|(_, value)| value == "-"));
    }

    #[test]
    fn chips_list_the_gate_first_then_stale_then_every_flag() {
        let everything = DfReading {
            squelched: true,
            azimuth_deg: None,
            station: None,
            aliasing: true,
            mode_aliasing: true,
            mirror: true,
            rotating: true,
            singular: true,
            table_out_of_range: true,
            ..reading()
        };
        let chips = df_chips(Some(&everything), Some(ProcessorGate::Phase), true);
        let labels: Vec<&str> = chips.iter().map(|chip| chip.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Needs cal",
                "Stale",
                "Squelch",
                "No heading",
                "No position",
                "Aliased",
                "Mode alias",
                "Mirror",
                "Rotating",
                "Singular",
                "Table off"
            ]
        );
        assert!(chips[0].danger && chips[1].danger && !chips[2].danger);
        assert!(chips.iter().all(|chip| !chip.title.is_empty()));
        assert!(df_chips(Some(&reading()), None, false).is_empty());
        assert!(df_chips(None, None, false).is_empty());
    }
}
