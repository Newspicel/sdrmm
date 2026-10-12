use std::f64::consts::PI;

use sdrmm_wire::array::{ArrayElement, ArrayGeometry, MIN_ARRAY_LANES, Winding};

pub const OVERLAP_M: f64 = 0.005;
pub const FALLBACK_SPACING_M: f64 = 0.5;
pub const LINE_AXIS_DEG: f64 = 90.0;
const STEPS_PER_M: f64 = 1_000.0;
pub const MIN_SIZE_M: f64 = 0.01;
const PREVIEW_FILL: f64 = 0.36;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Circle,
    Line,
    Custom,
}

impl Shape {
    pub const ALL: [(Self, &'static str); 3] = [
        (Self::Circle, "UCA"),
        (Self::Line, "ULA"),
        (Self::Custom, "Custom"),
    ];

    #[must_use]
    pub const fn of(geometry: &ArrayGeometry) -> Self {
        match geometry {
            ArrayGeometry::Uca { .. } => Self::Circle,
            ArrayGeometry::Ula { .. } => Self::Line,
            ArrayGeometry::Explicit { .. } => Self::Custom,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::Z => "z",
        }
    }

    #[must_use]
    pub const fn of(self, element: &ArrayElement) -> f64 {
        match self {
            Self::X => element.x_m,
            Self::Y => element.y_m,
            Self::Z => element.z_m,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewPoint {
    pub x: f64,
    pub y: f64,
    pub lane: usize,
}

#[must_use]
pub fn element_count(lanes: usize) -> usize {
    lanes.max(MIN_ARRAY_LANES as usize)
}

fn sized(geometry: &ArrayGeometry, lanes: usize) -> (ArrayGeometry, usize) {
    match geometry {
        ArrayGeometry::Explicit { positions } => {
            let kept: Vec<ArrayElement> = positions.iter().take(lanes).copied().collect();
            let count = kept.len();
            (ArrayGeometry::Explicit { positions: kept }, count)
        }
        other => (other.clone(), element_count(lanes)),
    }
}

fn xyz(element: &ArrayElement) -> [f64; 3] {
    [element.x_m, element.y_m, element.z_m]
}

#[must_use]
pub fn element_positions(geometry: &ArrayGeometry, lanes: usize) -> Vec<[f64; 3]> {
    match geometry {
        ArrayGeometry::Explicit { positions } => positions.iter().take(lanes).map(xyz).collect(),
        other => other.positions(element_count(lanes)).unwrap_or_default(),
    }
}

#[must_use]
pub fn adjacent_spacing_m(geometry: &ArrayGeometry, lanes: usize) -> Option<f64> {
    let (sized, count) = sized(geometry, lanes);
    sized.adjacent_spacing_m(count)
}

#[must_use]
pub fn unambiguous_hz(geometry: &ArrayGeometry, lanes: usize) -> Option<f64> {
    let (sized, count) = sized(geometry, lanes);
    sized.unambiguous_hz(count)
}

#[must_use]
pub fn aliasing(geometry: &ArrayGeometry, lanes: usize, center_hz: Option<f64>) -> bool {
    match (unambiguous_hz(geometry, lanes), center_hz) {
        (Some(limit), Some(center)) => center > limit,
        _ => false,
    }
}

fn to_millimetre(value: f64) -> f64 {
    (value * STEPS_PER_M).round() / STEPS_PER_M + 0.0
}

#[must_use]
pub fn convert_geometry(to: Shape, from: &ArrayGeometry, lanes: usize) -> ArrayGeometry {
    if Shape::of(from) == to {
        return from.clone();
    }
    let spacing = adjacent_spacing_m(from, lanes)
        .filter(|spacing| *spacing > 0.0)
        .unwrap_or(FALLBACK_SPACING_M);
    match to {
        Shape::Circle => ArrayGeometry::Uca {
            radius_m: spacing / (2.0 * (PI / element_count(lanes) as f64).sin()),
            first_deg: 0.0,
            winding: Winding::Clockwise,
        },
        Shape::Line => ArrayGeometry::Ula {
            spacing_m: spacing,
            axis_deg: LINE_AXIS_DEG,
        },
        Shape::Custom => ArrayGeometry::Explicit {
            positions: element_positions(from, lanes)
                .into_iter()
                .map(|[x, y, z]| ArrayElement {
                    x_m: to_millimetre(x),
                    y_m: to_millimetre(y),
                    z_m: to_millimetre(z),
                })
                .collect(),
        },
    }
}

#[must_use]
pub fn padded_positions(positions: &[ArrayElement], lanes: usize) -> Vec<ArrayElement> {
    (0..lanes.max(positions.len()))
        .map(|row| positions.get(row).copied().unwrap_or_default())
        .collect()
}

#[must_use]
pub fn moved_element(
    positions: &[ArrayElement],
    row: usize,
    axis: Axis,
    value: f64,
) -> Vec<ArrayElement> {
    let mut moved = positions.to_vec();
    if let Some(element) = moved.get_mut(row) {
        match axis {
            Axis::X => element.x_m = value,
            Axis::Y => element.y_m = value,
            Axis::Z => element.z_m = value,
        }
    }
    moved
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1]).hypot(a[2] - b[2])
}

#[must_use]
pub fn overlapping_rows(positions: &[ArrayElement], lanes: usize) -> Vec<usize> {
    let wired: Vec<[f64; 3]> = positions.iter().take(lanes).map(xyz).collect();
    (0..wired.len())
        .filter(|index| {
            wired[..*index]
                .iter()
                .any(|earlier| distance(*earlier, wired[*index]) < OVERLAP_M)
        })
        .collect()
}

#[must_use]
pub fn preview_points(geometry: &ArrayGeometry, lanes: usize, size: f64) -> Vec<PreviewPoint> {
    let positions = element_positions(geometry, lanes);
    let reach = positions
        .iter()
        .map(|[x, y, _]| x.abs().max(y.abs()))
        .fold(0.0, f64::max);
    let scale = if reach > 0.0 {
        PREVIEW_FILL * size / reach
    } else {
        0.0
    };
    let centre = size / 2.0;
    positions
        .into_iter()
        .enumerate()
        .map(|(index, [x, y, _])| PreviewPoint {
            x: centre + x * scale,
            y: centre - y * scale,
            lane: index + 1,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::array::LIGHT_SPEED_M_S;

    use super::*;

    fn circle(radius_m: f64) -> ArrayGeometry {
        ArrayGeometry::Uca {
            radius_m,
            first_deg: 0.0,
            winding: Winding::Clockwise,
        }
    }

    fn at(x_m: f64, y_m: f64) -> ArrayElement {
        ArrayElement { x_m, y_m, z_m: 0.0 }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn a_circle_of_four_spaces_its_elements_by_the_chord() {
        let spacing = adjacent_spacing_m(&circle(0.5), 4).expect("spacing");
        assert!(close(spacing, 2.0 * 0.5 * (PI / 4.0).sin()));
        let limit = unambiguous_hz(&circle(0.5), 4).expect("limit");
        assert!(close(limit, LIGHT_SPEED_M_S / (2.0 * spacing)));
    }

    #[test]
    fn fewer_than_two_lanes_still_lay_out_two_elements() {
        assert_eq!(element_positions(&circle(0.5), 0).len(), 2);
        assert_eq!(element_count(5), 5);
    }

    #[test]
    fn custom_positions_beyond_the_wired_lanes_are_ignored() {
        let custom = ArrayGeometry::Explicit {
            positions: vec![at(0.0, 0.0), at(0.3, 0.0), at(0.0, 0.01)],
        };
        assert!(close(adjacent_spacing_m(&custom, 2).expect("spacing"), 0.3));
        assert!(close(
            adjacent_spacing_m(&custom, 3).expect("spacing"),
            0.01
        ));
    }

    #[test]
    fn aliasing_starts_above_half_a_wavelength() {
        let line = ArrayGeometry::Ula {
            spacing_m: 0.5,
            axis_deg: 90.0,
        };
        let limit = unambiguous_hz(&line, 4).expect("limit");
        assert!(!aliasing(&line, 4, Some(limit - 1.0)));
        assert!(aliasing(&line, 4, Some(limit + 1.0)));
        assert!(!aliasing(&line, 4, None));
    }

    #[test]
    fn switching_shape_keeps_the_element_spacing() {
        let line = ArrayGeometry::Ula {
            spacing_m: 0.25,
            axis_deg: 0.0,
        };
        let round = convert_geometry(Shape::Circle, &line, 4);
        assert!(close(adjacent_spacing_m(&round, 4).expect("spacing"), 0.25));
        let back = convert_geometry(Shape::Line, &round, 4);
        assert_eq!(
            back,
            ArrayGeometry::Ula {
                spacing_m: adjacent_spacing_m(&round, 4).expect("spacing"),
                axis_deg: LINE_AXIS_DEG
            }
        );
        assert_eq!(convert_geometry(Shape::Line, &line, 4), line);
    }

    #[test]
    fn a_custom_layout_starts_from_the_old_shape_rounded_to_millimetres() {
        let line = ArrayGeometry::Ula {
            spacing_m: 0.3333,
            axis_deg: 90.0,
        };
        let ArrayGeometry::Explicit { positions } = convert_geometry(Shape::Custom, &line, 2)
        else {
            panic!("expected custom positions");
        };
        assert_eq!(positions, vec![at(-0.167, 0.0), at(0.167, 0.0)]);
    }

    #[test]
    fn an_unusable_shape_falls_back_to_half_a_metre() {
        let empty = ArrayGeometry::Explicit {
            positions: Vec::new(),
        };
        assert_eq!(
            convert_geometry(Shape::Line, &empty, 2),
            ArrayGeometry::Ula {
                spacing_m: FALLBACK_SPACING_M,
                axis_deg: LINE_AXIS_DEG
            }
        );
    }

    #[test]
    fn custom_rows_pad_to_the_lanes_and_move_one_axis() {
        let padded = padded_positions(&[at(1.0, 2.0)], 3);
        assert_eq!(padded, vec![at(1.0, 2.0), at(0.0, 0.0), at(0.0, 0.0)]);
        let moved = moved_element(&padded, 2, Axis::Z, 0.4);
        assert!(close(moved[2].z_m, 0.4));
        assert_eq!(moved[..2], padded[..2]);
        assert!(close(Axis::Y.of(&moved[0]), 2.0));
    }

    #[test]
    fn overlapping_rows_flag_the_later_duplicate() {
        let positions = [at(0.0, 0.0), at(0.5, 0.0), at(0.001, 0.0), at(0.0, 0.0)];
        assert_eq!(overlapping_rows(&positions, 3), vec![2]);
        assert_eq!(overlapping_rows(&positions, 4), vec![2, 3]);
    }

    #[test]
    fn the_preview_fits_the_layout_in_the_box_with_forward_up() {
        let points = preview_points(&circle(0.5), 4, 96.0);
        assert_eq!(points.len(), 4);
        assert!(close(points[0].x, 48.0));
        assert!(close(points[0].y, 48.0 - 0.36 * 96.0));
        assert!(close(points[1].x, 48.0 + 0.36 * 96.0));
        assert_eq!(points[3].lane, 4);
    }
}
