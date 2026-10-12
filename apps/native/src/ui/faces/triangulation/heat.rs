use sdrmm_wire::{
    frame::{FusionGridFrame, FusionGridOwned},
    fusion::{DfEstimate, DfStation},
};

use crate::ui::{
    faces::{map::df::STATION, scope::colormap::Colormap},
    kit_raster::{PLOT_BG, Pen, Raster, Rgba, Scene},
    map::ACCENT,
};

const CROSS_PX: f32 = 6.0;
const STATION_PX: f32 = 3.5;
const PALETTE_STEPS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub south: f64,
    pub west: f64,
    pub north: f64,
    pub east: f64,
}

#[must_use]
pub fn geo_to_grid(lat: f64, lon: f64, bounds: &Bounds, width: f32, height: f32) -> (f32, f32) {
    let span_lon = bounds.east - bounds.west;
    let span_lat = bounds.north - bounds.south;
    let x = if span_lon == 0.0 {
        width / 2.0
    } else {
        ((lon - bounds.west) / span_lon) as f32 * width
    };
    let y = if span_lat == 0.0 {
        height / 2.0
    } else {
        ((bounds.north - lat) / span_lat) as f32 * height
    };
    (x, y)
}

#[must_use]
pub fn inside_box(at: (f32, f32), width: f32, height: f32) -> bool {
    at.0 >= 0.0 && at.0 <= width && at.1 >= 0.0 && at.1 <= height
}

#[must_use]
pub fn owned(frame: &FusionGridFrame<'_>) -> FusionGridOwned {
    FusionGridOwned {
        stream_id: frame.stream_id,
        seq: frame.seq,
        timestamp: frame.timestamp,
        south: frame.south,
        west: frame.west,
        north: frame.north,
        east: frame.east,
        cols: frame.cols,
        rows: frame.rows,
        cells: frame.cells.to_vec(),
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Marks {
    pub estimate: Option<DfEstimate>,
    pub emitters: Vec<DfEstimate>,
    pub stations: Vec<DfStation>,
}

#[derive(Debug)]
pub struct HeatScene {
    grid: Option<FusionGridOwned>,
    marks: Marks,
    stamp: u64,
    table: Vec<Rgba>,
}

fn rgba(colour: u32) -> Rgba {
    let [_, r, g, b] = colour.to_be_bytes();
    [r, g, b, 255]
}

impl HeatScene {
    #[must_use]
    pub fn new(palette: Colormap) -> Self {
        let table = (0..PALETTE_STEPS)
            .map(|step| {
                let [r, g, b] = palette.sample(step as f64 / (PALETTE_STEPS - 1) as f64);
                [
                    (r * 255.0).round() as u8,
                    (g * 255.0).round() as u8,
                    (b * 255.0).round() as u8,
                    255,
                ]
            })
            .collect();
        Self {
            grid: None,
            marks: Marks::default(),
            stamp: 0,
            table,
        }
    }

    pub fn show(&mut self, grid: FusionGridOwned) {
        self.grid = Some(grid);
        self.stamp += 1;
    }

    pub fn mark(&mut self, marks: Marks) {
        if self.marks != marks {
            self.marks = marks;
            self.stamp += 1;
        }
    }

    fn paint_cells(&self, grid: &FusionGridOwned, raster: &mut Raster) {
        let (cols, rows) = (usize::from(grid.cols), usize::from(grid.rows));
        if cols == 0 || rows == 0 || grid.cells.len() < cols * rows {
            return;
        }
        let (width, height) = (raster.width as usize, raster.height as usize);
        for y in 0..height {
            let row = (y * rows / height).min(rows - 1);
            for x in 0..width {
                let col = (x * cols / width).min(cols - 1);
                let level = grid.cells[row * cols + col];
                raster.put(x as i64, y as i64, self.table[usize::from(level)]);
            }
        }
    }

    fn paint_marks(&self, bounds: &Bounds, raster: &mut Raster) {
        let (width, height) = (raster.width as f32, raster.height as f32);
        let scale = raster.scale.max(1.0);
        let place = |lat, lon| geo_to_grid(lat, lon, bounds, width, height);
        for station in &self.marks.stations {
            let at = place(station.lat, station.lon);
            if inside_box(at, width, height) {
                disc(raster, at, STATION_PX * scale, rgba(STATION));
                raster.circle(at, STATION_PX * scale, Pen::new(PLOT_BG, 1.0, scale));
            }
        }
        let estimate = self.marks.estimate.as_ref();
        let others = crate::ui::faces::map::df::other_emitters(estimate, &self.marks.emitters);
        for emitter in others {
            let pen = Pen::new(rgba(ACCENT), 0.5, scale);
            cross(
                raster,
                place(emitter.lat, emitter.lon),
                CROSS_PX * scale,
                pen,
            );
        }
        if let Some(estimate) = estimate {
            let pen = Pen::new(rgba(ACCENT), 1.0, 2.0 * scale);
            cross(
                raster,
                place(estimate.lat, estimate.lon),
                CROSS_PX * scale,
                pen,
            );
        }
    }
}

fn disc(raster: &mut Raster, centre: (f32, f32), radius: f32, colour: Rgba) {
    let reach = radius.ceil() as i64;
    let (cx, cy) = (centre.0.round() as i64, centre.1.round() as i64);
    for dy in -reach..=reach {
        for dx in -reach..=reach {
            if ((dx * dx + dy * dy) as f32) <= radius * radius {
                raster.put(cx + dx, cy + dy, colour);
            }
        }
    }
}

fn cross(raster: &mut Raster, at: (f32, f32), arm: f32, pen: Pen) {
    if !inside_box(at, raster.width as f32, raster.height as f32) {
        return;
    }
    raster.line((at.0 - arm, at.1), (at.0 + arm, at.1), pen);
    raster.line((at.0, at.1 - arm), (at.0, at.1 + arm), pen);
}

impl Scene for HeatScene {
    fn stamp(&self) -> u64 {
        self.stamp
    }

    fn paint(&mut self, raster: &mut Raster) {
        raster.fill(PLOT_BG);
        let Some(grid) = &self.grid else {
            return;
        };
        self.paint_cells(grid, raster);
        let bounds = Bounds {
            south: grid.south,
            west: grid.west,
            north: grid.north,
            east: grid.east,
        };
        self.paint_marks(&bounds, raster);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: Bounds = Bounds {
        south: 51.0,
        west: 7.0,
        north: 52.0,
        east: 8.0,
    };

    fn grid(cols: u16, rows: u16, cells: Vec<u8>) -> FusionGridOwned {
        FusionGridOwned {
            south: BOX.south,
            west: BOX.west,
            north: BOX.north,
            east: BOX.east,
            cols,
            rows,
            cells,
            ..FusionGridOwned::default()
        }
    }

    fn spot(lat: f64, lon: f64) -> DfEstimate {
        DfEstimate {
            lat,
            lon,
            ..DfEstimate::default()
        }
    }

    #[test]
    fn the_north_west_corner_lands_top_left() {
        assert_eq!(geo_to_grid(52.0, 7.0, &BOX, 100.0, 50.0), (0.0, 0.0));
        assert_eq!(geo_to_grid(51.0, 8.0, &BOX, 100.0, 50.0), (100.0, 50.0));
        assert_eq!(geo_to_grid(51.5, 7.5, &BOX, 100.0, 50.0), (50.0, 25.0));
        let flat = Bounds {
            north: 51.0,
            east: 7.0,
            ..BOX
        };
        assert_eq!(geo_to_grid(9.0, 9.0, &flat, 100.0, 50.0), (50.0, 25.0));
    }

    #[test]
    fn a_point_on_the_edge_is_inside_and_one_past_it_is_not() {
        assert!(inside_box((0.0, 50.0), 100.0, 50.0));
        assert!(!inside_box((-0.5, 10.0), 100.0, 50.0));
        assert!(!inside_box((10.0, 50.5), 100.0, 50.0));
    }

    #[test]
    fn the_first_row_of_cells_is_painted_at_the_top() {
        let mut scene = HeatScene::new(Colormap::Gray);
        scene.show(grid(1, 2, vec![255, 0]));
        let mut raster = Raster::sized(1, 2);
        scene.paint(&mut raster);
        assert_eq!(raster.at(0, 0), Some(scene.table[255]));
        assert_eq!(raster.at(0, 1), Some(scene.table[0]));
    }

    #[test]
    fn a_short_grid_or_no_grid_paints_only_the_background() {
        let mut scene = HeatScene::new(Colormap::Inferno);
        let mut raster = Raster::sized(2, 2);
        scene.paint(&mut raster);
        assert_eq!(raster.at(1, 1), Some(PLOT_BG));
        scene.show(grid(4, 4, vec![255; 3]));
        scene.paint(&mut raster);
        assert_eq!(raster.at(1, 1), Some(PLOT_BG));
    }

    #[test]
    fn the_estimate_is_crossed_in_the_accent_and_stations_are_dotted() {
        let mut scene = HeatScene::new(Colormap::Gray);
        scene.show(grid(1, 1, vec![0]));
        scene.mark(Marks {
            estimate: Some(spot(51.5, 7.5)),
            emitters: vec![spot(51.5, 7.5)],
            stations: vec![DfStation {
                lat: 51.9,
                lon: 7.1,
                ..DfStation::default()
            }],
        });
        let mut raster = Raster::sized(100, 100);
        scene.paint(&mut raster);
        assert_eq!(raster.at(50, 50), Some(rgba(ACCENT)));
        assert_eq!(raster.at(50 + CROSS_PX as i64, 50), Some(rgba(ACCENT)));
        assert_eq!(raster.at(10, 10), Some(rgba(STATION)));
        assert_eq!(raster.at(30, 30), Some(scene.table[0]));
    }

    #[test]
    fn marks_off_the_grid_are_not_drawn() {
        let mut scene = HeatScene::new(Colormap::Gray);
        scene.show(grid(1, 1, vec![0]));
        scene.mark(Marks {
            estimate: Some(spot(60.0, 7.5)),
            ..Marks::default()
        });
        let mut raster = Raster::sized(20, 20);
        scene.paint(&mut raster);
        assert!(
            raster
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == scene.table[0])
        );
    }

    #[test]
    fn only_new_cells_or_changed_marks_ask_for_a_repaint() {
        let mut scene = HeatScene::new(Colormap::Inferno);
        scene.show(grid(1, 1, vec![0]));
        let shown = scene.stamp();
        scene.mark(Marks::default());
        assert_eq!(scene.stamp(), shown);
        scene.mark(Marks {
            estimate: Some(spot(51.5, 7.5)),
            ..Marks::default()
        });
        assert!(scene.stamp() > shown);
    }

    #[test]
    fn a_decoded_frame_is_kept_whole() {
        let held = grid(2, 1, vec![3, 4]);
        let copy = owned(&held.frame());
        assert_eq!(copy, held);
    }
}
