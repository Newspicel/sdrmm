use sdrmm_wire::{
    frame::{VisibilityFrame, VisibilityOwned},
    limits::CORRELATOR_LIMITS as LIMITS,
    processor::correlator::{Baseline, CorrelatorParams, CorrelatorReading},
};

pub const FRINGE_POINTS: usize = 60;
pub const PHASE_SPAN: Range = Range {
    lo: -180.0,
    hi: 180.0,
};
pub const PHASE_TICKS: [f64; 5] = [-180.0, -90.0, 0.0, 90.0, 180.0];
const MIN_PERIOD_MS: f64 = 1_000.0;
const MAX_TICKS: usize = 1_000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub lo: f64,
    pub hi: f64,
}

impl Range {
    #[must_use]
    pub fn fraction(self, value: f64) -> f64 {
        if self.hi > self.lo {
            (value - self.lo) / (self.hi - self.lo)
        } else {
            0.0
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gutters {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Series {
    pub hz: Vec<f64>,
    pub db: Vec<f64>,
    pub deg: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
    pub at: f64,
    pub label: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fringe {
    key: Option<(usize, String)>,
    pub history: Vec<f64>,
}

impl Fringe {
    pub fn heard(&mut self, index: usize, at: &str, phase_deg: f64) -> bool {
        if self
            .key
            .as_ref()
            .is_some_and(|(held, stamp)| *held == index && stamp == at)
        {
            return false;
        }
        let same = self.key.as_ref().is_some_and(|(held, _)| *held == index);
        if !same {
            self.history.clear();
        }
        self.history.push(phase_deg);
        let extra = self.history.len().saturating_sub(FRINGE_POINTS);
        self.history.drain(..extra);
        self.key = Some((index, at.to_owned()));
        true
    }
}

#[must_use]
pub fn baseline_label(baseline: &Baseline) -> String {
    format!("{}-{}", baseline.a + 1, baseline.b + 1)
}

#[must_use]
pub fn baseline_options(baselines: &[Baseline]) -> Vec<(usize, String)> {
    baselines
        .iter()
        .enumerate()
        .map(|(index, baseline)| (index, baseline_label(baseline)))
        .collect()
}

#[must_use]
pub fn baseline_text(baseline: &Baseline) -> String {
    format!(
        "{:.2} \u{2220}{}\u{b0} {:.1} ns {} dB",
        baseline.coherence,
        baseline.phase_deg.round(),
        baseline.delay_ns,
        baseline.snr_db.round()
    )
}

#[must_use]
pub fn baseline_rows(
    reading: Option<&CorrelatorReading>,
    baseline: Option<&Baseline>,
    age: String,
) -> Vec<(String, String)> {
    let dash = || "-".to_owned();
    let mut rows = Vec::with_capacity(7);
    if let Some(baseline) = baseline {
        rows.push((baseline_label(baseline), baseline_text(baseline)));
    }
    rows.extend([
        (
            "Delay".to_owned(),
            baseline.map_or_else(dash, |found| format!("{:.2} ns", found.delay_ns)),
        ),
        (
            "Coh".to_owned(),
            baseline.map_or_else(dash, |found| format!("{:.2}", found.coherence)),
        ),
        (
            "Length".to_owned(),
            baseline.map_or_else(dash, |found| format!("{:.2} m", found.length_m)),
        ),
        (
            "Azimuth".to_owned(),
            baseline.map_or_else(dash, |found| format!("{}\u{b0}", found.azimuth_deg.round())),
        ),
        (
            "Int".to_owned(),
            reading.map_or_else(dash, |found| format!("{:.1} s", found.integrated_s)),
        ),
        ("Age".to_owned(), age),
    ]);
    rows
}

#[must_use]
pub fn chosen_index(chosen: usize, count: usize) -> usize {
    if chosen < count { chosen } else { 0 }
}

#[must_use]
pub fn period_ms(settings: Option<&CorrelatorParams>) -> u32 {
    let integrate_s = settings.map_or(1.0, |found| f64::from(found.integrate_s));
    (integrate_s * 1_000.0).max(MIN_PERIOD_MS).round() as u32
}

#[must_use]
pub fn powers_of_two(min: u32, max: u32) -> Vec<u32> {
    std::iter::successors(Some(min.max(1).next_power_of_two()), |value| {
        value.checked_mul(2)
    })
    .take_while(|value| *value <= max)
    .collect()
}

#[must_use]
pub fn bin_options() -> Vec<(u32, String)> {
    powers_of_two(LIMITS.bins.min, LIMITS.bins.max)
        .into_iter()
        .map(|value| (value, value.to_string()))
        .collect()
}

#[must_use]
pub fn channel_options(bins: u32) -> Vec<(u32, String)> {
    powers_of_two(LIMITS.channels.min, LIMITS.channels.max)
        .into_iter()
        .filter(|value| *value <= bins)
        .map(|value| (value, value.to_string()))
        .collect()
}

pub fn with_bins(settings: &mut CorrelatorParams, bins: u32) {
    settings.bins = bins;
    settings.channels = settings.channels.min(bins);
}

#[must_use]
pub fn owned(frame: &VisibilityFrame<'_>) -> VisibilityOwned {
    VisibilityOwned {
        stream_id: frame.stream_id,
        seq: frame.seq,
        timestamp: frame.timestamp,
        center_hz: frame.center_hz,
        span_hz: frame.span_hz,
        baselines: frame.baselines,
        bins: frame.bins,
        db_min: frame.db_min,
        db_max: frame.db_max,
        amplitude: frame.amplitude.to_vec(),
        phase: frame.phase.to_vec(),
    }
}

#[must_use]
pub fn frequency_range(frame: &VisibilityOwned) -> Range {
    let half = f64::from(frame.span_hz) / 2.0;
    Range {
        lo: frame.center_hz - half,
        hi: frame.center_hz + half,
    }
}

#[must_use]
pub fn level_range(frame: &VisibilityOwned) -> Range {
    Range {
        lo: f64::from(frame.db_min),
        hi: f64::from(frame.db_max),
    }
}

#[must_use]
pub fn bin_hz(bin: usize, frame: &VisibilityOwned) -> f64 {
    let span = f64::from(frame.span_hz);
    frame.center_hz - span / 2.0 + (bin as f64 + 0.5) * span / f64::from(frame.bins.max(1))
}

#[must_use]
pub fn baseline_series(frame: &VisibilityOwned, index: usize) -> Series {
    if index >= usize::from(frame.baselines) {
        return Series::default();
    }
    let bins = usize::from(frame.bins);
    let start = index * bins;
    let levels = level_range(frame);
    let mut series = Series {
        hz: Vec::with_capacity(bins),
        db: Vec::with_capacity(bins),
        deg: Vec::with_capacity(bins),
    };
    for bin in 0..bins {
        let amplitude = f64::from(frame.amplitude.get(start + bin).copied().unwrap_or(0));
        let phase = f64::from(frame.phase.get(start + bin).copied().unwrap_or(0));
        series.hz.push(bin_hz(bin, frame));
        series
            .db
            .push(levels.lo + amplitude / 255.0 * (levels.hi - levels.lo));
        series.deg.push(phase / 255.0 * 360.0 - 180.0);
    }
    series
}

#[must_use]
pub fn plot_rects(width: f64, height: f64, gutters: Gutters, gap: f64) -> Option<(Rect, Rect)> {
    let w = (width - gutters.left - gutters.right).max(0.0);
    let h = (height - gutters.top - gutters.bottom).max(0.0);
    let half = ((h - gap) / 2.0).max(0.0);
    if w <= 0.0 || half <= 0.0 {
        return None;
    }
    Some((
        Rect {
            x: gutters.left,
            y: gutters.top,
            w,
            h: half,
        },
        Rect {
            x: gutters.left,
            y: gutters.top + half + gap,
            w,
            h: half,
        },
    ))
}

fn point(rect: Rect, x: Range, y: Range, at: (f64, f64)) -> (f64, f64) {
    (
        rect.x + x.fraction(at.0) * rect.w,
        rect.y + rect.h - y.fraction(at.1) * rect.h,
    )
}

#[must_use]
pub fn trace(xs: &[f64], ys: &[f64], rect: Rect, x: Range, y: Range) -> Vec<(f64, f64)> {
    xs.iter()
        .zip(ys)
        .map(|(hz, value)| point(rect, x, y, (*hz, *value)))
        .collect()
}

#[must_use]
pub fn phase_runs(xs: &[f64], deg: &[f64], rect: Rect, x: Range) -> Vec<Vec<(f64, f64)>> {
    let mut runs: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut previous: Option<f64> = None;
    for (hz, value) in xs.iter().zip(deg) {
        let at = point(rect, x, PHASE_SPAN, (*hz, *value));
        match runs.last_mut() {
            Some(run) if previous.is_some_and(|last| (value - last).abs() <= 180.0) => {
                run.push(at);
            }
            _ => runs.push(vec![at]),
        }
        previous = Some(*value);
    }
    runs
}

#[must_use]
pub fn fringe_runs(history: &[f64], rect: Rect) -> Vec<Vec<(f64, f64)>> {
    let xs: Vec<f64> = (0..history.len()).map(|index| index as f64).collect();
    let span = Range {
        lo: 0.0,
        hi: (FRINGE_POINTS as f64 - 1.0).max(1.0),
    };
    phase_runs(&xs, history, rect, span)
}

#[must_use]
pub fn nice_step(span: f64, target: f64) -> f64 {
    if !(span > 0.0 && target > 0.0 && span.is_finite()) {
        return 0.0;
    }
    let raw = span / target;
    let magnitude = 10f64.powf(raw.log10().floor());
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

#[must_use]
pub fn step_decimals(step: f64) -> usize {
    if !(step > 0.0 && step.is_finite()) {
        return 0;
    }
    (-(step.log10() + 1e-9).floor()).max(0.0) as usize
}

#[must_use]
pub fn linear_ticks(range: Range, target: f64) -> Vec<Tick> {
    let (low, high) = (range.lo.min(range.hi), range.lo.max(range.hi));
    let step = nice_step(high - low, target);
    if step == 0.0 {
        return Vec::new();
    }
    let decimals = step_decimals(step);
    let first = (low / step - 1e-9).ceil() as i64;
    (first..)
        .map(|index| index as f64 * step)
        .take_while(|value| *value <= high + step * 1e-9)
        .take(MAX_TICKS)
        .map(|value| {
            let value = if value.abs() < step * 1e-9 {
                0.0
            } else {
                value
            };
            Tick {
                at: range.fraction(value),
                label: format!("{value:.decimals$}"),
            }
        })
        .collect()
}

#[must_use]
pub fn phase_ticks() -> Vec<Tick> {
    PHASE_TICKS
        .iter()
        .map(|deg| Tick {
            at: PHASE_SPAN.fraction(*deg),
            label: format!("{deg:.0}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn baseline() -> Baseline {
        Baseline {
            a: 0,
            b: 2,
            delay_ns: 3.456,
            coherence: 0.934,
            phase_deg: 12.4,
            length_m: 1.5,
            azimuth_deg: 44.6,
            snr_db: 18.2,
        }
    }

    fn frame(amplitude: Vec<u8>, phase: Vec<u8>, baselines: u16, bins: u16) -> VisibilityOwned {
        VisibilityOwned {
            center_hz: 100e6,
            span_hz: 2e6,
            baselines,
            bins,
            db_min: -100.0,
            db_max: 0.0,
            amplitude,
            phase,
            ..VisibilityOwned::default()
        }
    }

    const BOX: Rect = Rect {
        x: 10.0,
        y: 20.0,
        w: 100.0,
        h: 50.0,
    };

    #[test]
    fn a_baseline_is_named_by_its_lanes_counted_from_one() {
        assert_eq!(baseline_label(&baseline()), "1-3");
        let options = baseline_options(&[
            baseline(),
            Baseline {
                a: 1,
                b: 2,
                ..baseline()
            },
        ]);
        assert_eq!(options, vec![(0, "1-3".to_owned()), (1, "2-3".to_owned())]);
    }

    #[test]
    fn a_baseline_reads_coherence_phase_delay_and_snr() {
        assert_eq!(
            baseline_text(&baseline()),
            "0.93 \u{2220}12\u{b0} 3.5 ns 18 dB"
        );
    }

    #[test]
    fn readouts_show_dashes_until_a_baseline_is_heard() {
        let rows = baseline_rows(None, None, "-".to_owned());
        assert_eq!(rows[0], ("Delay".to_owned(), "-".to_owned()));
        assert!(rows.iter().all(|(_, value)| value == "-"));
        let reading = CorrelatorReading {
            integrated_s: 2.5,
            ..CorrelatorReading::default()
        };
        let rows = baseline_rows(Some(&reading), Some(&baseline()), "1.0 s".to_owned());
        assert_eq!(rows[0].0, "1-3");
        assert!(rows.contains(&("Delay".to_owned(), "3.46 ns".to_owned())));
        assert!(rows.contains(&("Length".to_owned(), "1.50 m".to_owned())));
        assert!(rows.contains(&("Azimuth".to_owned(), "45\u{b0}".to_owned())));
        assert!(rows.contains(&("Int".to_owned(), "2.5 s".to_owned())));
    }

    #[test]
    fn a_vanished_baseline_falls_back_to_the_first() {
        assert_eq!(chosen_index(2, 3), 2);
        assert_eq!(chosen_index(3, 3), 0);
        assert_eq!(chosen_index(0, 0), 0);
    }

    #[test]
    fn the_report_period_follows_integration_but_never_under_a_second() {
        assert_eq!(period_ms(None), 1_000);
        let quick = CorrelatorParams {
            integrate_s: 0.05,
            ..CorrelatorParams::default()
        };
        assert_eq!(period_ms(Some(&quick)), 1_000);
        let slow = CorrelatorParams {
            integrate_s: 4.0,
            ..quick
        };
        assert_eq!(period_ms(Some(&slow)), 4_000);
    }

    #[test]
    fn bins_and_channels_offer_powers_of_two_within_limits() {
        assert_eq!(powers_of_two(64, 512), vec![64, 128, 256, 512]);
        assert_eq!(powers_of_two(50, 64), vec![64]);
        let bins = bin_options();
        assert_eq!(bins.first().map(|(value, _)| *value), Some(LIMITS.bins.min));
        assert_eq!(bins.last().map(|(value, _)| *value), Some(LIMITS.bins.max));
        let channels = channel_options(64);
        assert_eq!(
            channels.iter().map(|(value, _)| *value).collect::<Vec<_>>(),
            vec![16, 32, 64]
        );
    }

    #[test]
    fn fewer_bins_pull_the_channels_down_with_them() {
        let mut settings = CorrelatorParams::default();
        with_bins(&mut settings, 128);
        assert_eq!((settings.bins, settings.channels), (128, 128));
        assert!(settings.valid());
        with_bins(&mut settings, 4_096);
        assert_eq!((settings.bins, settings.channels), (4_096, 128));
    }

    #[test]
    fn a_series_maps_bytes_to_decibels_degrees_and_bin_centres() {
        let shown = frame(vec![0, 255, 0, 0, 255, 0], vec![0, 255, 0, 0, 0, 0], 3, 2);
        let series = baseline_series(&shown, 0);
        assert_eq!(series.db, vec![-100.0, 0.0]);
        assert_eq!(series.deg, vec![-180.0, 180.0]);
        assert_eq!(series.hz, vec![99.5e6, 100.5e6]);
        assert_eq!(baseline_series(&shown, 1).db, vec![-100.0, -100.0]);
        assert_eq!(baseline_series(&shown, 3), Series::default());
    }

    #[test]
    fn a_short_frame_reads_missing_cells_as_the_floor() {
        let shown = frame(vec![255], vec![], 1, 2);
        let series = baseline_series(&shown, 0);
        assert_eq!(series.db, vec![0.0, -100.0]);
        assert_eq!(series.deg, vec![-180.0, -180.0]);
    }

    #[test]
    fn the_plot_splits_into_two_equal_halves_with_a_gap() {
        let gutters = Gutters {
            left: 34.0,
            right: 6.0,
            top: 12.0,
            bottom: 18.0,
        };
        let (amplitude, phase) = plot_rects(240.0, 200.0, gutters, 16.0).expect("room");
        assert_eq!(amplitude.w, 200.0);
        assert_eq!(amplitude.h, 77.0);
        assert_eq!(phase.y, 12.0 + 77.0 + 16.0);
        assert!(plot_rects(40.0, 200.0, gutters, 16.0).is_none());
        assert!(plot_rects(240.0, 40.0, gutters, 16.0).is_none());
    }

    #[test]
    fn a_trace_puts_low_values_at_the_bottom_and_high_at_the_top() {
        let x = Range { lo: 0.0, hi: 10.0 };
        let y = Range {
            lo: -100.0,
            hi: 0.0,
        };
        let points = trace(&[0.0, 10.0], &[-100.0, 0.0], BOX, x, y);
        assert_eq!(points, vec![(10.0, 70.0), (110.0, 20.0)]);
        let flat = Range { lo: 1.0, hi: 1.0 };
        assert_eq!(trace(&[5.0], &[5.0], BOX, flat, flat), vec![(10.0, 70.0)]);
    }

    #[test]
    fn phase_breaks_where_it_wraps() {
        let x = Range { lo: 0.0, hi: 3.0 };
        let runs = phase_runs(
            &[0.0, 1.0, 2.0, 3.0],
            &[170.0, 179.0, -179.0, -170.0],
            BOX,
            x,
        );
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].len(), 2);
        assert_eq!(runs[1].len(), 2);
        assert_eq!(runs[1][0].0, 10.0 + 2.0 / 3.0 * 100.0);
    }

    #[test]
    fn the_fringe_keeps_the_last_points_of_one_baseline() {
        let mut fringe = Fringe::default();
        assert!(fringe.heard(0, "a", 10.0));
        assert!(!fringe.heard(0, "a", 11.0));
        assert!(fringe.heard(0, "b", 20.0));
        assert_eq!(fringe.history, vec![10.0, 20.0]);
        assert!(fringe.heard(1, "b", 30.0));
        assert_eq!(fringe.history, vec![30.0]);
        for step in 0..100 {
            fringe.heard(1, &step.to_string(), f64::from(step));
        }
        assert_eq!(fringe.history.len(), FRINGE_POINTS);
        assert_eq!(fringe.history.last(), Some(&99.0));
    }

    #[test]
    fn the_fringe_spans_its_box_at_full_history() {
        let history = vec![0.0; FRINGE_POINTS];
        let runs = fringe_runs(&history, BOX);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].first().map(|at| at.0), Some(10.0));
        assert_eq!(runs[0].last().map(|at| at.0), Some(110.0));
        assert_eq!(runs[0][0].1, 45.0);
    }

    #[test]
    fn ticks_land_on_round_values() {
        assert_eq!(nice_step(10.0, 5.0), 2.0);
        assert_eq!(nice_step(0.0, 5.0), 0.0);
        assert_eq!(step_decimals(0.5), 1);
        assert_eq!(step_decimals(2.0), 0);
        let ticks = linear_ticks(
            Range {
                lo: 99.0,
                hi: 101.0,
            },
            5.0,
        );
        let labels: Vec<&str> = ticks.iter().map(|tick| tick.label.as_str()).collect();
        assert_eq!(labels, vec!["99.0", "99.5", "100.0", "100.5", "101.0"]);
        assert_eq!(ticks[2].at, 0.5);
        assert!(linear_ticks(Range { lo: 1.0, hi: 1.0 }, 5.0).is_empty());
    }

    #[test]
    fn phase_ticks_span_the_full_circle() {
        let ticks = phase_ticks();
        assert_eq!(ticks.first().map(|tick| tick.at), Some(0.0));
        assert_eq!(ticks.last().map(|tick| tick.at), Some(1.0));
        assert_eq!(ticks[2].label, "0");
    }
}
