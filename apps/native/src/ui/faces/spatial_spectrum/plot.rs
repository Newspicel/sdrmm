use std::collections::VecDeque;

use sdrmm_wire::frame::SpatialSpectrumFrame;

use super::logic::{
    SpatialView, bearing_hue, below_peak_db, bin_hz, cell_at, column_argmax, cursor_text, hue_rgb,
    row_deg, row_shift, source_row,
};
use crate::ui::{
    faces::scope::colormap::Colormap,
    kit_raster::{PLOT_BG, Pen, Raster, Rgba, Scene, WHITE},
};

pub const TRAIL_ROWS: usize = 160;
const CROSS_PX: f32 = 4.0;
const DIM_KEEP: f32 = 0.4;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grid {
    pub center_hz: f64,
    pub span_hz: f64,
    pub bearings: usize,
    pub bins: usize,
    pub db_min: f32,
    pub db_max: f32,
    pub cells: Vec<u8>,
}

impl Grid {
    #[must_use]
    pub fn of(frame: &SpatialSpectrumFrame<'_>) -> Self {
        Self {
            center_hz: frame.center_hz,
            span_hz: f64::from(frame.span_hz),
            bearings: usize::from(frame.bearings),
            bins: usize::from(frame.bins),
            db_min: frame.db_min,
            db_max: frame.db_max,
            cells: frame.cells.to_vec(),
        }
    }

    #[must_use]
    pub fn low_hz(&self) -> f64 {
        self.center_hz - self.span_hz / 2.0
    }

    fn holds(&self) -> bool {
        self.bins > 0 && self.bearings > 0 && self.cells.len() >= self.bins * self.bearings
    }

    fn level(&self, row: usize, col: usize) -> u8 {
        self.cells.get(row * self.bins + col).copied().unwrap_or(0)
    }

    fn trail_row(&self, offset_deg: f64) -> Vec<Rgba> {
        (0..self.bins)
            .map(|col| {
                let (row, level) = column_argmax(&self.cells, self.bearings, self.bins, col);
                let hue = row_deg(row, self.bearings) + offset_deg;
                let [r, g, b] = hue_rgb(hue, f64::from(level) / 255.0);
                [r, g, b, 255]
            })
            .collect()
    }
}

#[must_use]
pub fn dimmed(colour: Rgba, dim: bool) -> Rgba {
    if !dim {
        return colour;
    }
    let mix = |index: usize| {
        let (fore, back) = (f32::from(colour[index]), f32::from(PLOT_BG[index]));
        (back + (fore - back) * DIM_KEEP).round() as u8
    };
    [mix(0), mix(1), mix(2), 255]
}

fn palette(colormap: Colormap) -> Vec<Rgba> {
    colormap
        .lut()
        .as_chunks::<3>()
        .0
        .iter()
        .map(|[r, g, b]| [*r, *g, *b, 255])
        .collect()
}

#[derive(Debug)]
pub struct SpatialScene {
    grid: Option<Grid>,
    view: SpatialView,
    offset_deg: f64,
    marks: Vec<(f64, f64)>,
    dim: bool,
    table: Vec<Rgba>,
    trail: VecDeque<Vec<Rgba>>,
    stamp: u64,
}

impl Default for SpatialScene {
    fn default() -> Self {
        Self {
            grid: None,
            view: SpatialView::Map,
            offset_deg: 0.0,
            marks: Vec::new(),
            dim: false,
            table: palette(Colormap::default()),
            trail: VecDeque::with_capacity(TRAIL_ROWS),
            stamp: 0,
        }
    }
}

impl SpatialScene {
    pub fn show(&mut self, grid: Grid) {
        if self.trail.front().is_some_and(|row| row.len() != grid.bins) {
            self.trail.clear();
        }
        if self.trail.len() == TRAIL_ROWS {
            self.trail.pop_back();
        }
        self.trail.push_front(grid.trail_row(self.offset_deg));
        self.grid = Some(grid);
        self.stamp += 1;
    }

    pub fn set_view(&mut self, view: SpatialView) {
        self.view = view;
        self.stamp += 1;
    }

    pub fn set_offset(&mut self, offset_deg: f64) {
        self.offset_deg = offset_deg;
        self.stamp += 1;
    }

    pub fn set_marks(&mut self, marks: Vec<(f64, f64)>) {
        self.marks = marks;
        self.stamp += 1;
    }

    pub fn set_dim(&mut self, dim: bool) {
        self.dim = dim;
        self.stamp += 1;
    }

    pub fn set_colormap(&mut self, colormap: Colormap) {
        self.table = palette(colormap);
        self.stamp += 1;
    }

    #[must_use]
    pub fn cursor(&self, fx: f64, fy: f64) -> Option<String> {
        let grid = self.grid.as_ref().filter(|grid| grid.holds())?;
        let cell = cell_at(fx, fy, grid.bins, grid.bearings);
        let shift = row_shift(self.offset_deg, grid.bearings);
        let level = grid.level(source_row(cell.row, shift, grid.bearings), cell.col);
        Some(cursor_text(
            bin_hz(cell.col, grid.bins, grid.center_hz, grid.span_hz),
            row_deg(cell.row, grid.bearings),
            Some(below_peak_db(level, grid.db_min, grid.db_max)),
        ))
    }

    fn paint_map(&self, grid: &Grid, raster: &mut Raster) {
        let (width, height) = (raster.width as usize, raster.height as usize);
        let shift = row_shift(self.offset_deg, grid.bearings);
        for y in 0..height {
            let row = (y * grid.bearings / height).min(grid.bearings - 1);
            let source = source_row(row, shift, grid.bearings);
            for x in 0..width {
                let col = (x * grid.bins / width).min(grid.bins - 1);
                let colour = self.table[usize::from(grid.level(source, col))];
                raster.put(x as i64, y as i64, dimmed(colour, self.dim));
            }
        }
        self.paint_marks(grid, raster);
    }

    fn paint_marks(&self, grid: &Grid, raster: &mut Raster) {
        if grid.span_hz <= 0.0 {
            return;
        }
        let (width, height) = (raster.width as f32, raster.height as f32);
        let reach = CROSS_PX * raster.scale.max(1.0);
        let pen = Pen::new(WHITE, 1.0, 1.5 * raster.scale.max(1.0));
        for (hz, deg) in &self.marks {
            let x = ((hz - grid.low_hz()) / grid.span_hz) as f32 * width;
            let y = (bearing_hue(*deg) / 360.0) as f32 * height;
            raster.line((x - reach, y - reach), (x + reach, y + reach), pen);
            raster.line((x - reach, y + reach), (x + reach, y - reach), pen);
        }
    }

    fn paint_trail(&self, raster: &mut Raster) {
        let (width, height) = (raster.width as usize, raster.height as usize);
        for y in 0..height {
            let Some(line) = self.trail.get(y * TRAIL_ROWS / height) else {
                break;
            };
            if line.is_empty() {
                continue;
            }
            for x in 0..width {
                let colour = line[(x * line.len() / width).min(line.len() - 1)];
                raster.put(x as i64, y as i64, dimmed(colour, self.dim));
            }
        }
    }
}

impl Scene for SpatialScene {
    fn stamp(&self) -> u64 {
        self.stamp
    }

    fn paint(&mut self, raster: &mut Raster) {
        raster.fill(PLOT_BG);
        if raster.width == 0 || raster.height == 0 {
            return;
        }
        match self.view {
            SpatialView::Map => {
                if let Some(grid) = self.grid.as_ref().filter(|grid| grid.holds()) {
                    self.paint_map(grid, raster);
                }
            }
            SpatialView::Trail => self.paint_trail(raster),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(bearings: usize, bins: usize, cells: Vec<u8>) -> Grid {
        Grid {
            center_hz: 433_920_000.0,
            span_hz: 1_000_000.0,
            bearings,
            bins,
            db_min: -80.0,
            db_max: -20.0,
            cells,
        }
    }

    fn scene_with(grid: Grid) -> SpatialScene {
        let mut scene = SpatialScene::default();
        scene.set_colormap(Colormap::Gray);
        scene.show(grid);
        scene
    }

    #[test]
    fn bearing_zero_is_painted_at_the_top() {
        let scene = scene_with(grid(2, 1, vec![255, 0]));
        let mut raster = Raster::sized(1, 2);
        let mut scene = scene;
        scene.paint(&mut raster);
        assert_eq!(raster.at(0, 0), Some([255, 255, 255, 255]));
        assert_eq!(raster.at(0, 1), Some([0, 0, 0, 255]));
    }

    #[test]
    fn the_true_frame_turns_the_rows() {
        let mut scene = scene_with(grid(4, 1, vec![255, 0, 0, 0]));
        scene.set_offset(90.0);
        let mut raster = Raster::sized(1, 4);
        scene.paint(&mut raster);
        assert_eq!(raster.at(0, 1), Some([255, 255, 255, 255]));
        assert_eq!(raster.at(0, 0), Some([0, 0, 0, 255]));
    }

    #[test]
    fn a_short_grid_paints_only_the_background() {
        let mut scene = scene_with(grid(4, 4, vec![255; 3]));
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
    fn a_stale_picture_fades_toward_the_background() {
        assert_eq!(dimmed([255, 255, 255, 255], false), [255, 255, 255, 255]);
        let faded = dimmed([255, 255, 255, 255], true);
        assert!(faded[0] < 255 && faded[0] > PLOT_BG[0]);
    }

    #[test]
    fn a_peak_is_marked_with_a_cross() {
        let mut scene = scene_with(grid(4, 4, vec![0; 16]));
        scene.set_marks(vec![(433_920_000.0, 180.0)]);
        let mut raster = Raster::sized(40, 40);
        scene.paint(&mut raster);
        assert_eq!(raster.at(20, 20), Some(WHITE));
    }

    #[test]
    fn the_trail_keeps_the_newest_row_on_top_and_forgets_the_oldest() {
        let mut scene = scene_with(grid(2, 1, vec![255, 0]));
        for _ in 0..TRAIL_ROWS + 5 {
            scene.show(grid(2, 1, vec![0, 0]));
        }
        assert_eq!(scene.trail.len(), TRAIL_ROWS);
        scene.show(grid(2, 1, vec![255, 0]));
        scene.set_view(SpatialView::Trail);
        let mut raster = Raster::sized(1, TRAIL_ROWS as u32);
        scene.paint(&mut raster);
        assert_eq!(raster.at(0, 0), Some([255, 38, 38, 255]));
        assert_eq!(raster.at(0, 1), Some([0, 0, 0, 255]));
    }

    #[test]
    fn a_new_bin_count_starts_a_fresh_trail() {
        let mut scene = scene_with(grid(2, 1, vec![255, 0]));
        scene.show(grid(2, 2, vec![0; 4]));
        assert_eq!(scene.trail.len(), 1);
    }

    #[test]
    fn the_cursor_reads_the_cell_under_the_pointer() {
        let scene = scene_with(grid(4, 4, vec![255; 16]));
        assert_eq!(
            scene.cursor(0.5, 0.25).as_deref(),
            Some("434.045 MHz 90\u{b0} 0 dB")
        );
        assert_eq!(SpatialScene::default().cursor(0.5, 0.5), None);
    }

    #[test]
    fn every_change_asks_for_a_repaint() {
        let mut scene = SpatialScene::default();
        let before = scene.stamp();
        scene.set_dim(true);
        scene.set_view(SpatialView::Trail);
        assert!(scene.stamp() > before + 1);
    }
}
