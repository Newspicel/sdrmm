use std::sync::Arc;

use sdrmm_wire::frame::{FrameKind, FusionGridFrame, FusionGridOwned};
use zgui::prelude::*;

use crate::{
    store::Store,
    ui::{
        faces::{processor, scope::colormap::Colormap, triangulation::heat::owned},
        map::{overlay::Image, raster::PALETTE_STEPS},
    },
};

pub const HEAT_OPACITY: f32 = 0.75;

#[must_use]
pub fn inferno() -> Arc<Vec<u32>> {
    Arc::new(
        (0..PALETTE_STEPS)
            .map(|step| {
                let [r, g, b] = Colormap::Inferno.sample(step as f64 / (PALETTE_STEPS - 1) as f64);
                let channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u32;
                (channel(r) << 16) | (channel(g) << 8) | channel(b)
            })
            .collect(),
    )
}

#[must_use]
pub fn image_of(grid: &FusionGridOwned, palette: Arc<Vec<u32>>) -> Image {
    Image {
        south: grid.south,
        west: grid.west,
        north: grid.north,
        east: grid.east,
        cols: usize::from(grid.cols),
        rows: usize::from(grid.rows),
        cells: Arc::new(grid.cells.clone()),
        palette,
        opacity: HEAT_OPACITY,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Held {
    pub node: String,
    pub image: Image,
}

pub fn feed(store: Store, node: String, held: RwSignal<Option<Held>>) {
    let palette = inferno();
    let named = node.clone();
    processor::surface(store, node, FrameKind::FusionGrid, None, move |bytes| {
        match FusionGridFrame::decode(bytes) {
            Ok(frame) => held.set(Some(Held {
                node: named.clone(),
                image: image_of(&owned(&frame), palette.clone()),
            })),
            Err(error) => {
                tracing::warn!(node = %named, %error, "a fusion grid frame did not decode")
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_heat_is_placed_where_the_grid_says_and_kept_whole() {
        let grid = FusionGridOwned {
            south: 51.0,
            west: 7.0,
            north: 52.0,
            east: 8.0,
            cols: 2,
            rows: 1,
            cells: vec![0, 255],
            ..FusionGridOwned::default()
        };
        let image = image_of(&grid, inferno());
        assert_eq!((image.cols, image.rows), (2, 1));
        assert_eq!(image.cells.as_slice(), [0, 255]);
        assert_eq!((image.south, image.east), (51.0, 8.0));
        assert_eq!(image.opacity, HEAT_OPACITY);
    }

    #[test]
    fn the_inferno_ramp_runs_dark_to_bright() {
        let table = inferno();
        assert_eq!(table.len(), PALETTE_STEPS);
        let brightness =
            |colour: u32| ((colour >> 16) & 0xff) + ((colour >> 8) & 0xff) + (colour & 0xff);
        assert!(brightness(table[0]) < brightness(table[PALETTE_STEPS - 1]));
    }
}
