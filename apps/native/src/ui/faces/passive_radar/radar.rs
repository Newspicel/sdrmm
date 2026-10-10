use sdrmm_wire::{
    array::LIGHT_SPEED_M_S,
    frame::RangeDopplerFrame,
    limits::RADAR_LIMITS,
    radar::{RadarAxes, RadarUpdate, TrackState},
};

use super::readings::{signed, track_name};
use crate::ui::{
    faces::scope::colormap::Colormap,
    kit_raster::{PLOT_BG, PLOT_HOLD, Pen, Raster, Rgba, Scene, WHITE},
};

pub const TICKS: f64 = 5.0;
pub const ACCENT: Rgba = [122, 168, 255, 255];
const MAX_TICKS: usize = 1_000;
const DETECTION_RADIUS_PX: f32 = 4.0;
const TRACK_HALF_PX: f32 = 4.0;
const TRUTH_RADIUS_PX: f32 = 5.0;
const DIM: f32 = 0.4;
const PALETTE_STEPS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotAxes {
    pub range_first_m: f64,
    pub range_step_m: f64,
    pub ranges: u32,
    pub doppler_first_hz: f64,
    pub doppler_step_hz: f64,
    pub dopplers: u32,
    pub carrier_hz: f64,
    pub db_min: f64,
    pub db_max: f64,
}

impl PlotAxes {
    #[must_use]
    pub fn of_frame(frame: &RangeDopplerFrame<'_>) -> Self {
        Self {
            range_first_m: f64::from(frame.range_first_m),
            range_step_m: f64::from(frame.range_step_m),
            ranges: u32::from(frame.ranges),
            doppler_first_hz: f64::from(frame.doppler_first_hz),
            doppler_step_hz: f64::from(frame.doppler_step_hz),
            dopplers: u32::from(frame.dopplers),
            carrier_hz: frame.carrier_hz,
            db_min: f64::from(frame.db_min),
            db_max: f64::from(frame.db_max),
        }
    }

    #[must_use]
    pub fn of_reading(axes: &RadarAxes) -> Option<Self> {
        (axes.gates > 0 && axes.doppler_rows > 0).then(|| Self {
            range_first_m: 0.0,
            range_step_m: f64::from(axes.range_step_m),
            ranges: axes.gates,
            doppler_first_hz: -f64::from(axes.doppler_rows - 1) / 2.0
                * f64::from(axes.doppler_step_hz),
            doppler_step_hz: f64::from(axes.doppler_step_hz),
            dopplers: axes.doppler_rows,
            carrier_hz: axes.carrier_hz,
            db_min: f64::from(RADAR_LIMITS.surface_db.min),
            db_max: f64::from(RADAR_LIMITS.surface_db.max),
        })
    }

    #[must_use]
    pub fn range_span_km(&self) -> (f64, f64) {
        let lo = self.range_first_m - self.range_step_m / 2.0;
        (
            lo / 1_000.0,
            (lo + f64::from(self.ranges) * self.range_step_m) / 1_000.0,
        )
    }

    #[must_use]
    pub fn doppler_span_hz(&self) -> (f64, f64) {
        let lo = self.doppler_first_hz - self.doppler_step_hz / 2.0;
        (lo, lo + f64::from(self.dopplers) * self.doppler_step_hz)
    }

    #[must_use]
    pub fn place(&self, range_km: f64, doppler_hz: f64) -> (f64, f64) {
        let (range_lo, range_hi) = self.range_span_km();
        let (doppler_lo, doppler_hi) = self.doppler_span_hz();
        let across = range_hi - range_lo;
        let down = doppler_hi - doppler_lo;
        (
            if across > 0.0 {
                (range_km - range_lo) / across
            } else {
                0.0
            },
            if down > 0.0 {
                1.0 - (doppler_hz - doppler_lo) / down
            } else {
                0.0
            },
        )
    }

    #[must_use]
    pub fn point_at(&self, x: f64, y: f64) -> (f64, f64) {
        let (range_lo, range_hi) = self.range_span_km();
        let (doppler_lo, doppler_hi) = self.doppler_span_hz();
        (
            range_lo + x * (range_hi - range_lo),
            doppler_lo + (1.0 - y) * (doppler_hi - doppler_lo),
        )
    }

    #[must_use]
    pub fn level_db(&self, level: u8) -> f64 {
        self.db_min + f64::from(level) / 255.0 * (self.db_max - self.db_min)
    }
}

#[must_use]
pub fn plot_axes(frame: Option<PlotAxes>, update: Option<&RadarUpdate>) -> Option<PlotAxes> {
    frame.or_else(|| update.and_then(|update| PlotAxes::of_reading(&update.axes)))
}

#[must_use]
pub fn velocity_mps(doppler_hz: f64, carrier_hz: f64) -> Option<f64> {
    (carrier_hz > 0.0).then(|| -(LIGHT_SPEED_M_S / carrier_hz) * doppler_hz)
}

#[must_use]
pub fn hover_text(range_km: f64, doppler_hz: f64, carrier_hz: f64, db: Option<f64>) -> String {
    let mut parts = vec![
        format!("{range_km:.1} km"),
        format!("{} Hz", signed(doppler_hz, 0)),
    ];
    if let Some(speed) = velocity_mps(doppler_hz, carrier_hz) {
        parts.push(format!("{} m/s", signed(speed, 0)));
    }
    if let Some(db) = db {
        parts.push(format!("{db:.0} dB"));
    }
    parts.join(" \u{b7} ")
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

fn step_decimals(step: f64) -> usize {
    if !(step > 0.0 && step.is_finite()) {
        return 0;
    }
    (-(step.log10() + 1e-9).floor()).max(0.0) as usize
}

#[must_use]
pub fn nice_ticks(lo: f64, hi: f64, target: f64) -> Vec<f64> {
    let (low, high) = (lo.min(hi), lo.max(hi));
    let step = nice_step(high - low, target);
    if step == 0.0 {
        return if low.is_finite() {
            vec![low]
        } else {
            Vec::new()
        };
    }
    let first = (low / step - 1e-9).ceil() as i64;
    (first..)
        .map(|index| index as f64 * step)
        .take_while(|value| *value <= high + step * 1e-9)
        .take(MAX_TICKS)
        .map(|value| if value == 0.0 { 0.0 } else { value })
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
    pub at: f64,
    pub label: String,
}

fn ticks(lo: f64, hi: f64, at: impl Fn(f64) -> f64) -> Vec<Tick> {
    let decimals = step_decimals(nice_step((hi - lo).abs(), TICKS));
    nice_ticks(lo, hi, TICKS)
        .into_iter()
        .map(|value| Tick {
            at: at(value),
            label: format!("{:.decimals$}", value + 0.0),
        })
        .collect()
}

#[must_use]
pub fn range_ticks(axes: &PlotAxes) -> Vec<Tick> {
    let (lo, hi) = axes.range_span_km();
    ticks(lo, hi, |km| axes.place(km, 0.0).0)
}

#[must_use]
pub fn doppler_ticks(axes: &PlotAxes) -> Vec<Tick> {
    let (lo, hi) = axes.doppler_span_hz();
    ticks(lo, hi, |hz| axes.place(0.0, hz).1)
}

#[must_use]
pub fn speed_ticks(axes: &PlotAxes) -> Vec<Tick> {
    let (lo, hi) = axes.doppler_span_hz();
    let (Some(fast), Some(slow)) = (
        velocity_mps(lo, axes.carrier_hz),
        velocity_mps(hi, axes.carrier_hz),
    ) else {
        return Vec::new();
    };
    ticks(slow, fast, |mps| {
        axes.place(0.0, -mps * axes.carrier_hz / LIGHT_SPEED_M_S).1
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackMark {
    pub id: u32,
    pub head: (f64, f64),
    pub trail: Vec<(f64, f64)>,
    pub coasting: bool,
    pub selected: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Marks {
    pub detections: Vec<(f64, f64)>,
    pub tracks: Vec<TrackMark>,
    pub truth: Vec<(f64, f64)>,
}

#[must_use]
pub fn marks(
    update: Option<&RadarUpdate>,
    axes: Option<&PlotAxes>,
    selected: Option<u32>,
) -> Marks {
    let (Some(update), Some(axes)) = (update, axes) else {
        return Marks::default();
    };
    let at =
        |range_km: f32, doppler_hz: f32| axes.place(f64::from(range_km), f64::from(doppler_hz));
    Marks {
        detections: update
            .detections
            .iter()
            .map(|hit| at(hit.range_km, hit.doppler_hz))
            .collect(),
        tracks: update
            .tracks
            .iter()
            .map(|track| TrackMark {
                id: track.id,
                head: at(track.range_km, track.doppler_hz),
                trail: track
                    .trail
                    .iter()
                    .map(|point| at(point.range_km, point.doppler_hz))
                    .collect(),
                coasting: track.state == TrackState::Coasting,
                selected: selected == Some(track.id),
            })
            .collect(),
        truth: update
            .truth
            .iter()
            .filter(|truth| truth.in_view)
            .map(|truth| at(truth.range_km, truth.doppler_hz))
            .collect(),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackLabel {
    pub text: String,
    pub x: f64,
    pub y: f64,
}

fn inside((x, y): (f64, f64)) -> bool {
    (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y)
}

#[must_use]
pub fn track_labels(marks: &Marks) -> Vec<TrackLabel> {
    marks
        .tracks
        .iter()
        .filter(|track| inside(track.head))
        .map(|track| TrackLabel {
            text: track_name(track.id),
            x: track.head.0,
            y: track.head.1,
        })
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Surface {
    pub ranges: u16,
    pub dopplers: u16,
    pub cells: Vec<u8>,
}

impl Surface {
    #[must_use]
    pub fn fits(&self) -> bool {
        self.ranges > 0
            && self.dopplers > 0
            && self.cells.len() >= usize::from(self.ranges) * usize::from(self.dopplers)
    }

    #[must_use]
    pub fn level_at(&self, x: f64, y: f64) -> Option<u8> {
        if !self.fits() || !inside((x, y)) {
            return None;
        }
        let (ranges, dopplers) = (usize::from(self.ranges), usize::from(self.dopplers));
        let column = ((x * ranges as f64) as usize).min(ranges - 1);
        let row = dopplers - 1 - ((y * dopplers as f64) as usize).min(dopplers - 1);
        self.cells.get(row * ranges + column).copied()
    }
}

#[derive(Debug)]
pub struct RadarScene {
    pub surface: Option<Surface>,
    pub marks: Marks,
    dim: bool,
    stamp: u64,
    table: Vec<Rgba>,
}

fn palette(map: Colormap) -> Vec<Rgba> {
    (0..PALETTE_STEPS)
        .map(|step| {
            let [r, g, b] = map.sample(step as f64 / (PALETTE_STEPS - 1) as f64);
            [
                (r * 255.0).round() as u8,
                (g * 255.0).round() as u8,
                (b * 255.0).round() as u8,
                255,
            ]
        })
        .collect()
}

fn dimmed(colour: Rgba, dim: bool) -> Rgba {
    if !dim {
        return colour;
    }
    let mix = |channel: u8, under: u8| {
        (f32::from(channel) * DIM + f32::from(under) * (1.0 - DIM)).round() as u8
    };
    [
        mix(colour[0], PLOT_BG[0]),
        mix(colour[1], PLOT_BG[1]),
        mix(colour[2], PLOT_BG[2]),
        255,
    ]
}

impl RadarScene {
    #[must_use]
    pub fn new(map: Colormap) -> Self {
        Self {
            surface: None,
            marks: Marks::default(),
            dim: false,
            stamp: 0,
            table: palette(map),
        }
    }

    pub fn recolour(&mut self, map: Colormap) {
        self.table = palette(map);
        self.stamp += 1;
    }

    pub fn show(&mut self, ranges: u16, dopplers: u16, cells: &[u8]) {
        let surface = self.surface.get_or_insert_with(Surface::default);
        surface.ranges = ranges;
        surface.dopplers = dopplers;
        surface.cells.clear();
        surface.cells.extend_from_slice(cells);
        self.stamp += 1;
    }

    pub fn mark(&mut self, marks: Marks) {
        self.marks = marks;
        self.stamp += 1;
    }

    pub fn dim(&mut self, dim: bool) {
        if self.dim != dim {
            self.dim = dim;
            self.stamp += 1;
        }
    }

    fn paint_cells(&self, raster: &mut Raster) {
        let Some(surface) = self.surface.as_ref().filter(|surface| surface.fits()) else {
            return;
        };
        let (ranges, dopplers) = (usize::from(surface.ranges), usize::from(surface.dopplers));
        let (width, height) = (raster.width as usize, raster.height as usize);
        for y in 0..height {
            let row = dopplers - 1 - (y * dopplers / height).min(dopplers - 1);
            for x in 0..width {
                let column = (x * ranges / width).min(ranges - 1);
                let level = surface.cells[row * ranges + column];
                raster.put(
                    x as i64,
                    y as i64,
                    dimmed(self.table[usize::from(level)], self.dim),
                );
            }
        }
    }

    fn paint_marks(&self, raster: &mut Raster) {
        let scale = raster.scale.max(1.0);
        let (width, height) = (raster.width as f32, raster.height as f32);
        let px = |(x, y): (f64, f64)| (x as f32 * width, y as f32 * height);
        let ring = Pen::new(WHITE, 0.6, scale);
        for hit in &self.marks.detections {
            raster.circle(px(*hit), DETECTION_RADIUS_PX * scale, ring);
        }
        for track in &self.marks.tracks {
            paint_track(raster, track, &px, scale);
        }
        let truth = Pen::new(PLOT_HOLD, 1.0, scale);
        for point in &self.marks.truth {
            triangle(raster, px(*point), TRUTH_RADIUS_PX * scale, truth);
        }
    }
}

fn paint_track(
    raster: &mut Raster,
    track: &TrackMark,
    px: &impl Fn((f64, f64)) -> (f32, f32),
    scale: f32,
) {
    let trail = Pen::new(ACCENT, 0.6, scale);
    for pair in track.trail.windows(2) {
        raster.line(px(pair[0]), px(pair[1]), trail);
    }
    let width = if track.selected { 2.5 } else { 1.25 };
    let mut pen = Pen::new(ACCENT, 1.0, width * scale);
    if track.coasting {
        pen = pen.dashed(2.0 * scale, 2.0 * scale);
    }
    square(raster, px(track.head), TRACK_HALF_PX * scale, pen);
}

fn square(raster: &mut Raster, (x, y): (f32, f32), half: f32, pen: Pen) {
    let corners = [
        (x - half, y - half),
        (x + half, y - half),
        (x + half, y + half),
        (x - half, y + half),
    ];
    for side in 0..4 {
        raster.line(corners[side], corners[(side + 1) % 4], pen);
    }
}

fn triangle(raster: &mut Raster, (x, y): (f32, f32), radius: f32, pen: Pen) {
    let corners = [
        (x, y - radius),
        (x + radius, y + radius),
        (x - radius, y + radius),
    ];
    for side in 0..3 {
        raster.line(corners[side], corners[(side + 1) % 3], pen);
    }
}

impl Scene for RadarScene {
    fn stamp(&self) -> u64 {
        self.stamp
    }

    fn paint(&mut self, raster: &mut Raster) {
        raster.fill(PLOT_BG);
        if raster.width == 0 || raster.height == 0 {
            return;
        }
        self.paint_cells(raster);
        self.paint_marks(raster);
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::radar::{AdsbTruth, RadarDetection, RadarTrack, RadarTrailPoint};

    use super::*;

    fn axes() -> PlotAxes {
        PlotAxes {
            range_first_m: 0.0,
            range_step_m: 1_000.0,
            ranges: 10,
            doppler_first_hz: -2.0,
            doppler_step_hz: 1.0,
            dopplers: 5,
            carrier_hz: 100e6,
            db_min: -3.0,
            db_max: 30.0,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn spans_reach_half_a_cell_past_the_first_and_last_centres() {
        let (lo, hi) = axes().range_span_km();
        assert!(close(lo, -0.5) && close(hi, 9.5));
        let (lo, hi) = axes().doppler_span_hz();
        assert!(close(lo, -2.5) && close(hi, 2.5));
    }

    #[test]
    fn zero_doppler_sits_mid_height_and_positive_doppler_above() {
        let (x, y) = axes().place(-0.5, 0.0);
        assert!(close(x, 0.0) && close(y, 0.5));
        let (_, top) = axes().place(0.0, 2.5);
        assert!(close(top, 0.0));
        let (range, doppler) = axes().point_at(1.0, 1.0);
        assert!(close(range, 9.5) && close(doppler, -2.5));
    }

    #[test]
    fn reading_axes_centre_the_doppler_rows_on_zero() {
        let reading = RadarAxes {
            gates: 4,
            doppler_rows: 5,
            doppler_step_hz: 2.0,
            range_step_m: 300.0,
            ..RadarAxes::default()
        };
        let found = PlotAxes::of_reading(&reading).expect("axes");
        assert!(close(found.doppler_first_hz, -4.0));
        assert!(close(found.db_max, f64::from(RADAR_LIMITS.surface_db.max)));
        assert_eq!(PlotAxes::of_reading(&RadarAxes::default()), None);
    }

    #[test]
    fn a_frame_wins_over_the_reading_axes() {
        let update = RadarUpdate {
            axes: RadarAxes {
                gates: 4,
                doppler_rows: 3,
                ..RadarAxes::default()
            },
            ..RadarUpdate::default()
        };
        assert_eq!(plot_axes(Some(axes()), Some(&update)), Some(axes()));
        assert_eq!(
            plot_axes(None, Some(&update)).map(|found| found.ranges),
            Some(4)
        );
        assert_eq!(plot_axes(None, None), None);
    }

    #[test]
    fn levels_spread_linearly_over_the_frame_db_range() {
        assert!(close(axes().level_db(0), -3.0));
        assert!(close(axes().level_db(255), 30.0));
    }

    #[test]
    fn closing_targets_have_positive_doppler_and_negative_rate() {
        let speed = velocity_mps(100.0, 100e6).expect("speed");
        assert!(close(speed, -299.792_458));
        assert_eq!(velocity_mps(1.0, 0.0), None);
        assert_eq!(
            hover_text(12.34, 100.0, 100e6, Some(17.6)),
            "12.3 km \u{b7} +100 Hz \u{b7} -300 m/s \u{b7} 18 dB"
        );
        assert_eq!(hover_text(1.0, 0.0, 0.0, None), "1.0 km \u{b7} 0 Hz");
    }

    #[test]
    fn ticks_land_on_round_steps_inside_the_span() {
        assert!(close(nice_step(10.0, 5.0), 2.0));
        assert!(close(nice_step(0.7, 5.0), 0.2));
        assert_eq!(nice_step(0.0, 5.0), 0.0);
        assert_eq!(nice_ticks(-0.5, 9.5, 5.0), vec![0.0, 2.0, 4.0, 6.0, 8.0]);
        let labels: Vec<_> = range_ticks(&axes())
            .into_iter()
            .map(|tick| tick.label)
            .collect();
        assert_eq!(labels, vec!["0", "2", "4", "6", "8"]);
        let doppler = doppler_ticks(&axes());
        let zero = doppler
            .iter()
            .find(|tick| tick.label == "0")
            .expect("zero tick");
        assert!(close(zero.at, 0.5));
    }

    #[test]
    fn speed_ticks_mirror_doppler_about_zero() {
        let wide = PlotAxes {
            doppler_first_hz: -200.0,
            doppler_step_hz: 100.0,
            ..axes()
        };
        let speeds = speed_ticks(&wide);
        let zero = speeds
            .iter()
            .find(|tick| tick.label == "0")
            .expect("zero tick");
        assert!(close(zero.at, 0.5));
        let fast = speeds
            .iter()
            .find(|tick| tick.label == "500")
            .expect("500 tick");
        assert!(fast.at > 0.5);
        assert!(
            speed_ticks(&PlotAxes {
                carrier_hz: 0.0,
                ..wide
            })
            .is_empty()
        );
    }

    fn update() -> RadarUpdate {
        RadarUpdate {
            detections: vec![RadarDetection {
                range_km: 4.5,
                doppler_hz: 0.0,
                ..RadarDetection::default()
            }],
            tracks: vec![RadarTrack {
                id: 7,
                range_km: 9.5,
                doppler_hz: 2.5,
                state: TrackState::Coasting,
                trail: vec![
                    RadarTrailPoint {
                        range_km: 2.0,
                        doppler_hz: 0.0,
                    },
                    RadarTrailPoint {
                        range_km: 3.0,
                        doppler_hz: 1.0,
                    },
                ],
                ..RadarTrack::default()
            }],
            truth: vec![
                AdsbTruth {
                    in_view: true,
                    range_km: 4.5,
                    ..AdsbTruth::default()
                },
                AdsbTruth::default(),
            ],
            ..RadarUpdate::default()
        }
    }

    #[test]
    fn marks_place_detections_tracks_trails_and_visible_truth() {
        let held = update();
        let found = marks(Some(&held), Some(&axes()), Some(7));
        assert_eq!(found.detections, vec![(0.5, 0.5)]);
        assert_eq!(found.truth.len(), 1);
        let track = &found.tracks[0];
        assert_eq!(track.head, (1.0, 0.0));
        assert_eq!(track.trail.len(), 2);
        assert!(track.coasting && track.selected);
        assert_eq!(marks(Some(&held), None, None), Marks::default());
    }

    #[test]
    fn track_labels_name_tracks_inside_the_plot_only() {
        let mut found = marks(Some(&update()), Some(&axes()), None);
        assert_eq!(
            track_labels(&found),
            vec![TrackLabel {
                text: "T7".into(),
                x: 1.0,
                y: 0.0
            }]
        );
        found.tracks[0].head = (1.2, 0.5);
        assert!(track_labels(&found).is_empty());
    }

    #[test]
    fn the_most_negative_doppler_is_painted_at_the_bottom() {
        let mut scene = RadarScene::new(Colormap::Classic);
        scene.show(1, 2, &[0, 255]);
        let mut raster = Raster::sized(1, 2);
        scene.paint(&mut raster);
        assert_eq!(raster.at(0, 0), Some(scene.table[255]));
        assert_eq!(raster.at(0, 1), Some(scene.table[0]));
    }

    #[test]
    fn a_stale_surface_is_painted_dim() {
        let mut scene = RadarScene::new(Colormap::Gray);
        scene.show(1, 1, &[255]);
        scene.dim(true);
        let mut raster = Raster::sized(1, 1);
        scene.paint(&mut raster);
        assert_eq!(raster.at(0, 0), Some(dimmed(scene.table[255], true)));
        assert_ne!(raster.at(0, 0), Some(scene.table[255]));
    }

    #[test]
    fn a_short_surface_paints_only_the_background() {
        let mut scene = RadarScene::new(Colormap::Classic);
        scene.show(4, 4, &[255; 3]);
        let mut raster = Raster::sized(2, 2);
        scene.paint(&mut raster);
        assert!(
            raster
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == PLOT_BG)
        );
    }

    #[test]
    fn a_track_head_is_drawn_in_the_accent() {
        let mut scene = RadarScene::new(Colormap::Classic);
        scene.mark(Marks {
            tracks: vec![TrackMark {
                id: 1,
                head: (0.5, 0.5),
                trail: Vec::new(),
                coasting: false,
                selected: false,
            }],
            ..Marks::default()
        });
        let mut raster = Raster::sized(20, 20);
        scene.paint(&mut raster);
        assert_eq!(raster.at(6, 6), Some(ACCENT));
        assert_eq!(raster.at(10, 10), Some(PLOT_BG));
    }

    #[test]
    fn the_level_under_the_pointer_follows_the_flipped_rows() {
        let surface = Surface {
            ranges: 2,
            dopplers: 2,
            cells: vec![1, 2, 3, 4],
        };
        assert_eq!(surface.level_at(0.1, 0.9), Some(1));
        assert_eq!(surface.level_at(0.9, 0.1), Some(4));
        assert_eq!(surface.level_at(1.5, 0.1), None);
    }

    #[test]
    fn new_surfaces_marks_colours_and_dimming_ask_for_a_repaint() {
        let mut scene = RadarScene::new(Colormap::Classic);
        let mut last = scene.stamp();
        let mut bumped = |scene: &RadarScene| {
            let moved = scene.stamp() > last;
            last = scene.stamp();
            moved
        };
        scene.mark(Marks::default());
        assert!(bumped(&scene));
        scene.recolour(Colormap::Magma);
        assert!(bumped(&scene));
        scene.dim(true);
        assert!(bumped(&scene));
        scene.dim(true);
        assert!(!bumped(&scene));
    }
}
