use sdrmm_wire::{
    limits::{Bounds, SPATIAL_LIMITS as LIMITS},
    processor::spatial::{SpatialMethod, SpatialPeak, SpatialReading, SpatialSpectrumParams},
};

use crate::ui::faces::{
    df::logic::BearingFrame,
    processor::Chip,
    scope::colormap::{COLORMAPS, Colormap},
};

pub const PEAK_ROWS: usize = 3;
pub const BEARING_TICKS: [u32; 5] = [0, 90, 180, 270, 360];
const FULL_CIRCLE_DEG: u32 = 360;
const SATURATION: f64 = 0.85;
const MAX_TICKS: usize = 1_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpatialView {
    #[default]
    Map,
    Trail,
}

#[must_use]
pub fn view_options() -> Vec<(SpatialView, &'static str)> {
    vec![(SpatialView::Map, "Map"), (SpatialView::Trail, "Trail")]
}

#[must_use]
pub fn view_title(view: SpatialView) -> &'static str {
    match view {
        SpatialView::Map => "Bearing over frequency",
        SpatialView::Trail => "Frequency over time, colour is bearing",
    }
}

#[must_use]
pub fn method_options() -> Vec<(SpatialMethod, String)> {
    vec![
        (SpatialMethod::Bartlett, "Bartlett".to_owned()),
        (SpatialMethod::Capon, "Capon".to_owned()),
        (SpatialMethod::Music, "MUSIC".to_owned()),
    ]
}

#[must_use]
pub fn method_title(method: SpatialMethod) -> &'static str {
    match method {
        SpatialMethod::Bartlett => "Robust, wide peaks",
        SpatialMethod::Capon => "Sharper peaks",
        SpatialMethod::Music => "Sharpest, needs clean signals",
    }
}

#[must_use]
pub fn powers_of_two(bounds: Bounds<u32>) -> Vec<u32> {
    std::iter::successors(Some(1_u32), |value| value.checked_mul(2))
        .take_while(|value| *value <= bounds.max)
        .filter(|value| bounds.contains(*value))
        .collect()
}

#[must_use]
pub fn bin_options() -> Vec<(u32, String)> {
    powers_of_two(LIMITS.bins)
        .into_iter()
        .map(|value| (value, value.to_string()))
        .collect()
}

#[must_use]
pub fn column_options(bins: u32, current: u32) -> Vec<(u32, String)> {
    powers_of_two(LIMITS.columns)
        .into_iter()
        .filter(|value| *value <= bins || *value == current)
        .map(|value| (value, value.to_string()))
        .collect()
}

#[must_use]
pub fn with_bins(params: &SpatialSpectrumParams, bins: u32) -> SpatialSpectrumParams {
    SpatialSpectrumParams {
        bins,
        columns: params.columns.min(bins),
        ..*params
    }
}

#[must_use]
pub fn step_options() -> Vec<(u32, String)> {
    let low = LIMITS.azimuth_step_deg.min.ceil() as u32;
    let high = LIMITS.azimuth_step_deg.max.floor() as u32;
    (low..=high)
        .filter(|deg| FULL_CIRCLE_DEG.is_multiple_of(*deg))
        .map(|deg| (deg, format!("{deg}\u{b0}")))
        .collect()
}

#[must_use]
pub fn colour_options() -> Vec<(Colormap, String)> {
    COLORMAPS
        .iter()
        .map(|map| {
            let name = map.name();
            let mut chars = name.chars();
            let label = chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            });
            (*map, label)
        })
        .collect()
}

#[must_use]
pub fn bin_hz(col: usize, bins: usize, center_hz: f64, span_hz: f64) -> f64 {
    center_hz - span_hz / 2.0 + (col as f64 + 0.5) * span_hz / bins.max(1) as f64
}

#[must_use]
pub fn row_deg(row: usize, bearings: usize) -> f64 {
    if bearings == 0 {
        0.0
    } else {
        row as f64 * 360.0 / bearings as f64
    }
}

#[must_use]
pub fn row_shift(offset_deg: f64, bearings: usize) -> usize {
    if bearings == 0 {
        return 0;
    }
    let count = bearings as i64;
    ((offset_deg * bearings as f64 / 360.0).round() as i64).rem_euclid(count) as usize
}

#[must_use]
pub fn source_row(row: usize, shift: usize, bearings: usize) -> usize {
    (row + bearings - shift % bearings.max(1)) % bearings.max(1)
}

#[must_use]
pub fn frame_offset(frame: BearingFrame, reading: Option<&SpatialReading>) -> f64 {
    match frame {
        BearingFrame::True => reading.and_then(|found| found.azimuth_deg).unwrap_or(0.0),
        BearingFrame::Relative => 0.0,
    }
}

#[must_use]
pub fn bearing_hue(deg: f64) -> f64 {
    deg.rem_euclid(360.0)
}

#[must_use]
pub fn below_peak_db(level: u8, db_min: f32, db_max: f32) -> f64 {
    (f64::from(level) / 255.0 - 1.0) * f64::from(db_max - db_min)
}

fn mhz(hz: f64) -> String {
    format!("{:.3} MHz", hz / 1e6)
}

fn degree_text(value: f64) -> String {
    format!("{}\u{b0}", bearing_hue(value).round() as u32 % 360)
}

#[must_use]
pub fn cursor_text(hz: f64, bearing: f64, db: Option<f64>) -> String {
    let mut parts = vec![mhz(hz), degree_text(bearing)];
    if let Some(db) = db {
        parts.push(format!("{db:.0} dB"));
    }
    parts.join(" ")
}

#[must_use]
pub fn peak_bearing(peak: &SpatialPeak, frame: BearingFrame, offset_deg: f64) -> f64 {
    match frame {
        BearingFrame::True => peak
            .true_deg
            .map_or(f64::from(peak.bearing_deg) + offset_deg, f64::from),
        BearingFrame::Relative => f64::from(peak.bearing_deg),
    }
}

#[must_use]
pub fn peak_text(peak: &SpatialPeak, frame: BearingFrame, offset_deg: f64) -> String {
    format!(
        "{} {}",
        mhz(peak.freq_hz),
        degree_text(peak_bearing(peak, frame, offset_deg))
    )
}

#[must_use]
pub fn top_peaks(reading: Option<&SpatialReading>) -> Vec<SpatialPeak> {
    let mut peaks = reading.map(|found| found.peaks.clone()).unwrap_or_default();
    peaks.sort_by(|a, b| b.db.total_cmp(&a.db));
    peaks.truncate(PEAK_ROWS);
    peaks
}

#[must_use]
pub fn readout_rows(
    reading: Option<&SpatialReading>,
    frame: BearingFrame,
    offset_deg: f64,
    age: String,
) -> Vec<(String, String)> {
    let peaks = top_peaks(reading);
    let first = peaks
        .first()
        .map_or_else(|| "-".to_owned(), |peak| peak_text(peak, frame, offset_deg));
    let mut rows = vec![("Peak".to_owned(), first)];
    rows.extend(
        peaks
            .iter()
            .enumerate()
            .skip(1)
            .map(|(rank, peak)| (format!("#{}", rank + 1), peak_text(peak, frame, offset_deg))),
    );
    rows.push(("Age".to_owned(), age));
    rows
}

#[must_use]
pub fn spatial_chips(reading: Option<&SpatialReading>, bad_frames: u64) -> Vec<Chip> {
    let mut chips = Vec::new();
    let dropped = reading.map_or(0, |found| found.dropped_frames);
    if dropped > 0 {
        chips.push(Chip::danger(format!("Drops {dropped}"), "Frames lost"));
    }
    if bad_frames > 0 {
        chips.push(Chip::danger(
            format!("Bad {bad_frames}"),
            "Frames that did not decode",
        ));
    }
    chips
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub col: usize,
    pub row: usize,
}

#[must_use]
pub fn cell_at(fx: f64, fy: f64, bins: usize, bearings: usize) -> Cell {
    let index = |fraction: f64, count: usize| {
        ((fraction.clamp(0.0, 1.0) * count as f64).floor() as usize).min(count.saturating_sub(1))
    };
    Cell {
        col: index(fx, bins),
        row: index(fy, bearings),
    }
}

#[must_use]
pub fn plot_fraction(origin: (f32, f32), size: (f32, f32), at: (f32, f32)) -> Option<(f64, f64)> {
    (size.0 > 0.0 && size.1 > 0.0).then(|| {
        (
            f64::from(((at.0 - origin.0) / size.0).clamp(0.0, 1.0)),
            f64::from(((at.1 - origin.1) / size.1).clamp(0.0, 1.0)),
        )
    })
}

fn nice_step(span: f64, target: usize) -> f64 {
    if !(span > 0.0 && span.is_finite()) || target == 0 {
        return 0.0;
    }
    let raw = span / target as f64;
    let magnitude = 10_f64.powf(raw.log10().floor());
    let norm = raw / magnitude;
    let factor = if norm <= 1.0 {
        1.0
    } else if norm <= 2.0 {
        2.0
    } else if norm <= 5.0 {
        5.0
    } else {
        10.0
    };
    factor * magnitude
}

fn step_decimals(step: f64) -> usize {
    if step > 0.0 && step.is_finite() {
        (-(step.log10() + 1e-9).floor()).max(0.0) as usize
    } else {
        0
    }
}

#[must_use]
pub fn ticks(low: f64, high: f64, target: usize) -> Vec<(f64, String)> {
    let step = nice_step(high - low, target);
    if step == 0.0 {
        return Vec::new();
    }
    let decimals = step_decimals(step);
    let scale = 10_f64.powi(decimals as i32);
    let first = (low / step - 1e-9).ceil() as i64;
    (first..)
        .map(|index| (index as f64 * step * scale).round() / scale)
        .take_while(|value| *value <= high + step * 1e-9)
        .take(MAX_TICKS)
        .map(|value| {
            let at = ((value - low) / (high - low)).clamp(0.0, 1.0);
            (at, format!("{value:.decimals$}"))
        })
        .collect()
}

#[must_use]
pub fn column_argmax(cells: &[u8], bearings: usize, bins: usize, col: usize) -> (usize, u8) {
    (0..bearings)
        .map(|row| (row, cells.get(row * bins + col).copied().unwrap_or(0)))
        .fold(
            (0, 0),
            |best, next| if next.1 > best.1 { next } else { best },
        )
}

#[must_use]
pub fn hue_rgb(hue_deg: f64, value: f64) -> [u8; 3] {
    let h = bearing_hue(hue_deg) / 60.0;
    let v = value.clamp(0.0, 1.0);
    let chroma = v * SATURATION;
    let second = chroma * (1.0 - ((h % 2.0) - 1.0).abs());
    let base = v - chroma;
    let (r, g, b) = match h.floor() as u32 % 6 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    [r, g, b].map(|channel| ((channel + base) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(freq_hz: f64, bearing_deg: f32, db: f32, true_deg: Option<f32>) -> SpatialPeak {
        SpatialPeak {
            freq_hz,
            bearing_deg,
            db,
            true_deg,
        }
    }

    #[test]
    fn a_bin_maps_to_its_centre_frequency_and_a_row_to_its_bearing() {
        assert!((bin_hz(0, 4, 100e6, 4e6) - 98.5e6).abs() < 1e-3);
        assert!((bin_hz(3, 4, 100e6, 4e6) - 101.5e6).abs() < 1e-3);
        assert!(row_deg(0, 180).abs() < 1e-9);
        assert!((row_deg(45, 180) - 90.0).abs() < 1e-9);
        assert!(row_deg(3, 0).abs() < 1e-9);
    }

    #[test]
    fn rows_rotate_by_the_array_azimuth() {
        let rotated = |offset: f64| {
            let shift = row_shift(offset, 4);
            (0..4)
                .map(|row| source_row(row, shift, 4))
                .collect::<Vec<_>>()
        };
        assert_eq!(rotated(90.0), [3, 0, 1, 2]);
        assert_eq!(rotated(-90.0), [1, 2, 3, 0]);
        assert_eq!(rotated(360.0), [0, 1, 2, 3]);
        assert_eq!(row_shift(450.0, 4), 1);
        assert_eq!(row_shift(90.0, 0), 0);
    }

    #[test]
    fn the_cursor_reads_frequency_bearing_and_level() {
        let cell = cell_at(0.5, 0.25, 4, 4);
        assert_eq!(cell, Cell { col: 2, row: 1 });
        assert_eq!(cell_at(1.0, 1.0, 4, 4), Cell { col: 3, row: 3 });
        let hz = bin_hz(cell.col, 4, 433_920_000.0, 1_000_000.0);
        assert_eq!(
            cursor_text(hz, row_deg(cell.row, 4), Some(-62.4)),
            "434.045 MHz 90\u{b0} -62 dB"
        );
        assert!(below_peak_db(255, -80.0, -20.0).abs() < 1e-9);
        assert!((below_peak_db(0, -80.0, -20.0) + 60.0).abs() < 1e-9);
        assert_eq!(
            plot_fraction((10.0, 10.0), (100.0, 50.0), (60.0, 35.0)),
            Some((0.5, 0.5))
        );
        assert_eq!(plot_fraction((0.0, 0.0), (0.0, 50.0), (1.0, 1.0)), None);
    }

    #[test]
    fn the_three_strongest_peaks_read_in_the_chosen_frame() {
        let reading = SpatialReading {
            peaks: vec![
                peak(433_920_000.0, 137.0, -60.0, Some(227.0)),
                peak(145_000_000.0, 10.0, -40.0, None),
                peak(1e6, 0.0, -90.0, None),
                peak(2e6, 0.0, -95.0, None),
            ],
            ..SpatialReading::default()
        };
        let peaks = top_peaks(Some(&reading));
        assert_eq!(
            peaks.iter().map(|found| found.db).collect::<Vec<_>>(),
            [-40.0, -60.0, -90.0]
        );
        assert_eq!(
            peak_text(&peaks[1], BearingFrame::True, 0.0),
            "433.920 MHz 227\u{b0}"
        );
        assert_eq!(
            peak_text(&peaks[1], BearingFrame::Relative, 0.0),
            "433.920 MHz 137\u{b0}"
        );
        assert_eq!(
            peak_text(&peaks[0], BearingFrame::True, 90.0),
            "145.000 MHz 100\u{b0}"
        );
        let rows = readout_rows(
            Some(&reading),
            BearingFrame::Relative,
            0.0,
            "1 s".to_owned(),
        );
        let labels: Vec<&str> = rows.iter().map(|(label, _)| label.as_str()).collect();
        assert_eq!(labels, ["Peak", "#2", "#3", "Age"]);
        assert_eq!(
            readout_rows(None, BearingFrame::Relative, 0.0, "-".to_owned()),
            vec![
                ("Peak".to_owned(), "-".to_owned()),
                ("Age".to_owned(), "-".to_owned())
            ]
        );
    }

    #[test]
    fn the_offset_applies_only_to_the_true_frame() {
        let reading = SpatialReading {
            azimuth_deg: Some(45.0),
            ..SpatialReading::default()
        };
        assert!((frame_offset(BearingFrame::True, Some(&reading)) - 45.0).abs() < 1e-9);
        assert!(frame_offset(BearingFrame::Relative, Some(&reading)).abs() < 1e-9);
        assert!(frame_offset(BearingFrame::True, None).abs() < 1e-9);
    }

    #[test]
    fn the_choice_lists_follow_the_limits() {
        assert_eq!(
            bin_options()
                .iter()
                .map(|(value, _)| *value)
                .collect::<Vec<_>>(),
            [256, 512, 1_024, 2_048, 4_096]
        );
        assert_eq!(
            column_options(256, 64)
                .iter()
                .map(|(value, _)| *value)
                .collect::<Vec<_>>(),
            [64, 128, 256]
        );
        assert_eq!(column_options(256, 1_024).len(), 4);
        assert_eq!(
            step_options()
                .iter()
                .map(|(value, _)| *value)
                .collect::<Vec<_>>(),
            [1, 2, 3, 4, 5, 6, 8, 9, 10]
        );
        assert_eq!(method_options().len(), 3);
        assert!(!method_title(SpatialMethod::Music).is_empty());
        assert_eq!(colour_options()[0].1, "Classic");
        assert_eq!(view_options().len(), 2);
        assert!(!view_title(SpatialView::Trail).is_empty());
    }

    #[test]
    fn fewer_bins_pull_the_columns_down_with_them() {
        let params = SpatialSpectrumParams {
            columns: 1_024,
            bins: 2_048,
            ..SpatialSpectrumParams::default()
        };
        let next = with_bins(&params, 512);
        assert_eq!((next.bins, next.columns), (512, 512));
        assert!(with_bins(&SpatialSpectrumParams::default(), 4_096).valid());
    }

    #[test]
    fn ticks_land_on_round_values() {
        let labels = |found: Vec<(f64, String)>| {
            found
                .into_iter()
                .map(|(_, label)| label)
                .collect::<Vec<_>>()
        };
        assert_eq!(labels(ticks(0.0, 32.0, 5)), ["0", "10", "20", "30"]);
        assert_eq!(
            labels(ticks(0.0, 1.0, 5)),
            ["0.0", "0.2", "0.4", "0.6", "0.8", "1.0"]
        );
        let span = ticks(433.4, 434.4, 5);
        assert!(span.iter().all(|(at, _)| (0.0..=1.0).contains(at)));
        assert!(ticks(5.0, 5.0, 4).is_empty());
    }

    #[test]
    fn drops_and_undecodable_frames_become_chips() {
        assert!(spatial_chips(None, 0).is_empty());
        let reading = SpatialReading {
            dropped_frames: 3,
            ..SpatialReading::default()
        };
        let chips = spatial_chips(Some(&reading), 2);
        assert_eq!(chips[0].label, "Drops 3");
        assert_eq!(chips[1].label, "Bad 2");
        assert!(chips.iter().all(|chip| chip.danger));
    }

    #[test]
    fn the_trail_colours_by_bearing_and_brightens_with_power() {
        assert_eq!(hue_rgb(0.0, 1.0), [255, 38, 38]);
        assert_eq!(hue_rgb(120.0, 0.0), [0, 0, 0]);
        assert_eq!(column_argmax(&[1, 9, 7, 2, 3, 4], 3, 2, 0), (1, 7));
        assert_eq!(column_argmax(&[1, 9, 7, 2, 3, 4], 3, 2, 1), (0, 9));
    }

    #[test]
    fn powers_of_two_stay_inside_their_bounds() {
        assert_eq!(powers_of_two(Bounds::new(3, 20)), [4, 8, 16]);
        assert!(powers_of_two(Bounds::new(0, 0)).is_empty());
    }
}
