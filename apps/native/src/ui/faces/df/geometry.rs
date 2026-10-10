use sdrmm_wire::array::{ArrayGeometry, MIN_ARRAY_LANES, Winding};

const SHAPE_TOLERANCE: f64 = 1e-3;

type Xyz = [f64; 3];

#[must_use]
pub fn element_count(lanes: u32) -> u32 {
    lanes.max(MIN_ARRAY_LANES)
}

#[must_use]
pub fn element_positions(geometry: &ArrayGeometry, lanes: u32) -> Vec<Xyz> {
    match geometry {
        ArrayGeometry::Uca {
            radius_m,
            first_deg,
            winding,
        } => {
            let count = element_count(lanes);
            let sense = if *winding == Winding::CounterClockwise {
                -1.0
            } else {
                1.0
            };
            (0..count)
                .map(|lane| {
                    let angle = (first_deg + sense * 360.0 * f64::from(lane) / f64::from(count))
                        .to_radians();
                    [radius_m * angle.sin(), radius_m * angle.cos(), 0.0]
                })
                .collect()
        }
        ArrayGeometry::Ula {
            spacing_m,
            axis_deg,
        } => {
            let count = element_count(lanes);
            let axis = axis_deg.to_radians();
            (0..count)
                .map(|lane| {
                    let offset = (f64::from(lane) - f64::from(count - 1) / 2.0) * spacing_m;
                    [offset * axis.sin(), offset * axis.cos(), 0.0]
                })
                .collect()
        }
        ArrayGeometry::Explicit { positions } => positions
            .iter()
            .take(lanes as usize)
            .map(|element| [element.x_m, element.y_m, element.z_m])
            .collect(),
    }
}

fn distance(a: Xyz, b: Xyz) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1]).hypot(a[2] - b[2])
}

#[derive(Clone, Copy, Debug)]
struct Span {
    from: Xyz,
    unit: Xyz,
    length: f64,
}

impl Span {
    fn between(from: Xyz, to: Xyz) -> Self {
        let length = distance(from, to);
        Self {
            from,
            unit: [
                (to[0] - from[0]) / length,
                (to[1] - from[1]) / length,
                (to[2] - from[2]) / length,
            ],
            length,
        }
    }

    fn along(&self, point: Xyz) -> f64 {
        (0..3)
            .map(|axis| (point[axis] - self.from[axis]) * self.unit[axis])
            .sum()
    }

    fn off_line(&self, point: Xyz) -> f64 {
        let run = self.along(point);
        let foot = [
            self.from[0] + run * self.unit[0],
            self.from[1] + run * self.unit[1],
            self.from[2] + run * self.unit[2],
        ];
        distance(point, foot)
    }

    fn tolerance(&self) -> f64 {
        SHAPE_TOLERANCE * self.length
    }
}

fn widest_span(positions: &[Xyz]) -> Option<Span> {
    let mut widest: Option<Span> = None;
    for (index, from) in positions.iter().enumerate() {
        for to in &positions[index + 1..] {
            if distance(*from, *to) > widest.map_or(0.0, |span| span.length) {
                widest = Some(Span::between(*from, *to));
            }
        }
    }
    widest
}

fn line_span(positions: &[Xyz]) -> Option<Span> {
    let span = widest_span(positions)?;
    positions
        .iter()
        .all(|point| span.off_line(*point) <= span.tolerance())
        .then_some(span)
}

fn level(positions: &[Xyz], tolerance: f64) -> bool {
    let mean = positions.iter().map(|point| point[2]).sum::<f64>() / positions.len() as f64;
    positions
        .iter()
        .all(|point| (point[2] - mean).abs() <= tolerance)
}

fn level_line(positions: &[Xyz]) -> Option<Span> {
    let span = line_span(positions)?;
    level(positions, span.tolerance()).then_some(span)
}

fn evenly_spaced(positions: &[Xyz]) -> bool {
    let Some(span) = level_line(positions) else {
        return false;
    };
    let mut runs: Vec<f64> = positions.iter().map(|point| span.along(*point)).collect();
    runs.sort_by(f64::total_cmp);
    let step = span.length / (runs.len() - 1) as f64;
    runs.windows(2)
        .all(|pair| (pair[1] - pair[0] - step).abs() <= span.tolerance())
}

#[must_use]
pub fn is_collinear(geometry: &ArrayGeometry, lanes: u32) -> bool {
    matches!(geometry, ArrayGeometry::Ula { .. })
        || line_span(&element_positions(geometry, lanes)).is_some()
}

#[must_use]
pub fn is_level_line(geometry: &ArrayGeometry, lanes: u32) -> bool {
    matches!(geometry, ArrayGeometry::Ula { .. })
        || level_line(&element_positions(geometry, lanes)).is_some()
}

#[must_use]
pub fn allows_structured(geometry: &ArrayGeometry, lanes: u32) -> bool {
    matches!(
        geometry,
        ArrayGeometry::Uca { .. } | ArrayGeometry::Ula { .. }
    ) || evenly_spaced(&element_positions(geometry, lanes))
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::array::ArrayElement;

    use super::*;

    fn explicit(points: &[(f64, f64, f64)]) -> ArrayGeometry {
        ArrayGeometry::Explicit {
            positions: points
                .iter()
                .map(|&(x_m, y_m, z_m)| ArrayElement { x_m, y_m, z_m })
                .collect(),
        }
    }

    fn scattered() -> ArrayGeometry {
        explicit(&[(0.0, 0.0, 0.0), (0.4, 0.1, 0.0), (0.1, 0.5, 0.0)])
    }

    fn even_row() -> ArrayGeometry {
        explicit(&[(0.8, 0.8, 0.0), (0.0, 0.0, 0.0), (0.4, 0.4, 0.0)])
    }

    #[test]
    fn a_circle_puts_the_first_element_ahead_and_winds_clockwise() {
        let circle = ArrayGeometry::Uca {
            radius_m: 1.0,
            first_deg: 0.0,
            winding: Winding::Clockwise,
        };
        let points = element_positions(&circle, 4);
        assert_eq!(points.len(), 4);
        assert!((points[0][1] - 1.0).abs() < 1e-9);
        assert!((points[1][0] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_line_is_centred_on_the_array() {
        let line = ArrayGeometry::Ula {
            spacing_m: 0.5,
            axis_deg: 90.0,
        };
        let points = element_positions(&line, 3);
        assert!((points[0][0] + 0.5).abs() < 1e-9);
        assert!(points[1][0].abs() < 1e-9);
        assert!((points[2][0] - 0.5).abs() < 1e-9);
    }

    #[test]
    fn grid_free_methods_need_a_line_a_circle_or_an_even_row() {
        let line = ArrayGeometry::Ula {
            spacing_m: 0.4,
            axis_deg: 90.0,
        };
        assert!(allows_structured(&line, 3));
        assert!(allows_structured(&ArrayGeometry::default(), 3));
        assert!(allows_structured(&even_row(), 3));
        assert!(!allows_structured(&scattered(), 3));
        let uneven = explicit(&[(0.8, 0.8, 0.0), (0.0, 0.0, 0.0), (0.2, 0.2, 0.0)]);
        assert!(!allows_structured(&uneven, 3));
    }

    #[test]
    fn a_row_is_collinear_and_level_only_when_flat() {
        assert!(is_collinear(&even_row(), 3));
        assert!(is_level_line(&even_row(), 3));
        assert!(!is_collinear(&scattered(), 3));
        assert!(!is_collinear(&ArrayGeometry::default(), 4));
        let tilted = explicit(&[(0.0, 0.0, 0.0), (0.4, 0.0, 0.4), (0.8, 0.0, 0.8)]);
        assert!(is_collinear(&tilted, 3));
        assert!(!is_level_line(&tilted, 3));
    }

    #[test]
    fn a_single_or_stacked_element_is_never_a_line() {
        assert!(!is_collinear(&explicit(&[(0.0, 0.0, 0.0)]), 1));
        assert!(!is_collinear(
            &explicit(&[(0.1, 0.1, 0.0), (0.1, 0.1, 0.0)]),
            2
        ));
    }
}
