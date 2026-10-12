use std::sync::Arc;

use zgui::{
    canvas::{CanvasScene, ShapeBuilder},
    elements::{kurbo, kurbo::Shape as _},
};

use super::{
    geo::{Geo, View},
    paint::solid,
};

pub const PALETTE_STEPS: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub south: f64,
    pub west: f64,
    pub north: f64,
    pub east: f64,
    pub cols: usize,
    pub rows: usize,
    pub cells: Arc<Vec<u8>>,
    pub palette: Arc<Vec<u32>>,
    pub opacity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Run {
    pub level: u8,
    pub row: usize,
    pub first: usize,
    pub last: usize,
}

#[must_use]
pub fn runs(image: &Image) -> Vec<Run> {
    let (cols, rows) = (image.cols, image.rows);
    if cols == 0 || rows == 0 || image.cells.len() < cols * rows {
        return Vec::new();
    }
    let mut out = Vec::new();
    for row in 0..rows {
        let line = &image.cells[row * cols..(row + 1) * cols];
        let mut start = 0;
        for col in 1..=cols {
            if col < cols && line[col] == line[start] {
                continue;
            }
            if line[start] > 0 {
                out.push(Run {
                    level: line[start],
                    row,
                    first: start,
                    last: col - 1,
                });
            }
            start = col;
        }
    }
    out
}

pub fn draw(scene: &mut CanvasScene, view: &View, image: &Image) {
    let (left, top) = view.screen(Geo::new(image.north, image.west));
    let (right, bottom) = view.screen(Geo::new(image.south, image.east));
    let right = if right < left {
        right + view.world()
    } else {
        right
    };
    let cell_w = (right - left) / image.cols.max(1) as f64;
    let cell_h = (bottom - top) / image.rows.max(1) as f64;
    let mut paths: Vec<kurbo::BezPath> = vec![kurbo::BezPath::new(); PALETTE_STEPS];
    for run in runs(image) {
        let x0 = left + run.first as f64 * cell_w;
        let x1 = left + (run.last + 1) as f64 * cell_w;
        let y0 = top + run.row as f64 * cell_h;
        let rect = kurbo::Rect::new(x0, y0, x1, y0 + cell_h);
        paths[usize::from(run.level)].extend(rect.to_path(0.1));
    }
    for (level, path) in paths.into_iter().enumerate() {
        if path.elements().is_empty() {
            continue;
        }
        let colour = image.palette.get(level).copied().unwrap_or(0);
        scene.push(
            ShapeBuilder::new(path)
                .fill(solid(colour, image.opacity))
                .build(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(cols: usize, rows: usize, cells: Vec<u8>) -> Image {
        Image {
            south: 51.0,
            west: 7.0,
            north: 52.0,
            east: 8.0,
            cols,
            rows,
            cells: Arc::new(cells),
            palette: Arc::new(vec![0; PALETTE_STEPS]),
            opacity: 0.75,
        }
    }

    #[test]
    fn equal_neighbours_merge_and_the_floor_is_left_clear() {
        let found = runs(&image(4, 2, vec![3, 3, 0, 5, 0, 0, 0, 0]));
        assert_eq!(
            found,
            [
                Run {
                    level: 3,
                    row: 0,
                    first: 0,
                    last: 1
                },
                Run {
                    level: 5,
                    row: 0,
                    first: 3,
                    last: 3
                },
            ]
        );
    }

    #[test]
    fn a_short_grid_draws_nothing() {
        assert!(runs(&image(4, 4, vec![9; 3])).is_empty());
        assert!(runs(&image(0, 0, Vec::new())).is_empty());
    }
}
