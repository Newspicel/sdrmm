use crate::array::manifold::{Geometry, Winding};

const LINE_AXIS_DEG: f64 = 90.0;

pub fn uca_positions(radius_m: f64, count: usize) -> Result<Vec<[f64; 3]>, String> {
    Geometry::uca(radius_m, count, 0.0, Winding::Clockwise)
        .map(|geometry| points(&geometry))
        .map_err(|err| err.to_string())
}

pub fn circle(radius_m: f64, count: usize) -> Vec<[f64; 3]> {
    uca_positions(radius_m, count).unwrap_or_default()
}

pub fn line(spacing_m: f64, count: usize) -> Vec<[f64; 3]> {
    Geometry::ula(spacing_m, count, LINE_AXIS_DEG)
        .map(|geometry| points(&geometry))
        .unwrap_or_default()
}

fn points(geometry: &Geometry) -> Vec<[f64; 3]> {
    geometry
        .positions()
        .iter()
        .map(|p| [p.x, p.y, p.z])
        .collect()
}
