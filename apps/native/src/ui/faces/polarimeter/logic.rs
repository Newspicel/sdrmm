use std::{f64::consts::TAU, time::Duration};

use sdrmm_wire::processor::polarimeter::{Hand, PolarimeterParams, PolarimeterReading};

use crate::ui::faces::processor::age_label;

pub const ELLIPSE_POINTS: usize = 64;
pub const PICK_TWO: &str = "Pick two lanes";
pub const LINEAR_DEG: f64 = 2.0;
pub const ARROW_STEP: f64 = 0.35;
pub const ARROW_SIZE: f64 = 5.0;

pub type Point = (f64, f64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    Matched,
    Cross,
}

pub const OUTPUTS: [(Output, &str, &str); 2] = [
    (
        Output::Matched,
        "Matched",
        "Beam output matched to the wave",
    ),
    (Output::Cross, "Cross", "Beam output across the wave"),
];

#[must_use]
pub fn output_of(matched: bool) -> Output {
    if matched {
        Output::Matched
    } else {
        Output::Cross
    }
}

#[must_use]
pub fn output_title(output: Output) -> &'static str {
    OUTPUTS
        .iter()
        .find(|(option, _, _)| *option == output)
        .map_or("", |(_, _, title)| *title)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    H,
    V,
}

#[must_use]
pub fn hand_text(hand: Hand) -> &'static str {
    match hand {
        Hand::Right => "RH",
        Hand::Left => "LH",
        Hand::Linear => "Lin",
    }
}

#[must_use]
pub fn ellipse_point(
    t: f64,
    angle_deg: f64,
    ellipticity_deg: f64,
    radius: f64,
    centre: f64,
) -> Point {
    let (psi, chi) = (angle_deg.to_radians(), ellipticity_deg.to_radians());
    let along = chi.cos() * t.cos();
    let across = chi.sin() * t.sin();
    let x = along * psi.cos() - across * psi.sin();
    let y = along * psi.sin() + across * psi.cos();
    (centre + radius * x, centre - radius * y)
}

#[must_use]
pub fn ellipse_points(
    angle_deg: f64,
    ellipticity_deg: f64,
    radius: f64,
    centre: f64,
    points: usize,
) -> Vec<Point> {
    let steps = points.max(3);
    (0..steps)
        .map(|step| {
            let t = step as f64 / steps as f64 * TAU;
            ellipse_point(t, angle_deg, ellipticity_deg, radius, centre)
        })
        .collect()
}

#[must_use]
pub fn sense_step(v: f64, ellipticity_deg: f64) -> f64 {
    let clockwise = v > 0.0;
    let forward_turns_left = ellipticity_deg > 0.0;
    if clockwise == forward_turns_left {
        -ARROW_STEP
    } else {
        ARROW_STEP
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SenseArrow {
    pub arc: [Point; 2],
    pub head: [Point; 3],
}

#[must_use]
pub fn sense_arrow(
    angle_deg: f64,
    ellipticity_deg: f64,
    v: f64,
    radius: f64,
    centre: f64,
) -> Option<SenseArrow> {
    if ellipticity_deg.abs() <= LINEAR_DEG {
        return None;
    }
    let step = sense_step(v, ellipticity_deg);
    let at = |t: f64| ellipse_point(t, angle_deg, ellipticity_deg, radius, centre);
    let (start, tip, back) = (at(0.0), at(step), at(step * 0.8));
    let length = (tip.0 - back.0).hypot(tip.1 - back.1);
    if length == 0.0 {
        return None;
    }
    let (dx, dy) = ((tip.0 - back.0) / length, (tip.1 - back.1) / length);
    let base = (tip.0 - dx * ARROW_SIZE, tip.1 - dy * ARROW_SIZE);
    let half = ARROW_SIZE * 0.6;
    let left = (base.0 - dy * half, base.1 + dx * half);
    let right = (base.0 + dy * half, base.1 - dx * half);
    Some(SenseArrow {
        arc: [start, back],
        head: [left, tip, right],
    })
}

#[must_use]
pub fn lane_edit(params: &PolarimeterParams, which: Which, lane: u32) -> Option<PolarimeterParams> {
    let other = match which {
        Which::H => params.v_lane,
        Which::V => params.h_lane,
    };
    if lane == other {
        return None;
    }
    let mut next = *params;
    match which {
        Which::H => next.h_lane = lane,
        Which::V => next.v_lane = lane,
    }
    Some(next)
}

#[must_use]
pub fn stokes_text(value: f32) -> String {
    format!("{value:.2}")
}

#[must_use]
pub fn percent_text(fraction: f32) -> String {
    format!("{}%", (fraction.clamp(0.0, 1.0) * 100.0).round() as u32)
}

fn value(
    reading: Option<&PolarimeterReading>,
    text: impl Fn(&PolarimeterReading) -> String,
) -> String {
    reading.map_or_else(|| "-".to_owned(), text)
}

#[must_use]
pub fn left_rows(reading: Option<&PolarimeterReading>) -> Vec<(String, String)> {
    vec![
        (
            "I".to_owned(),
            value(reading, |r| format!("{:.1} dB", r.i_db)),
        ),
        ("Q".to_owned(), value(reading, |r| stokes_text(r.q))),
        ("U".to_owned(), value(reading, |r| stokes_text(r.u))),
        ("V".to_owned(), value(reading, |r| stokes_text(r.v))),
        (
            "SNR".to_owned(),
            value(reading, |r| {
                r.snr_db
                    .map_or_else(|| "-".to_owned(), |snr| format!("{snr:.0} dB"))
            }),
        ),
    ]
}

#[must_use]
pub fn right_rows(
    reading: Option<&PolarimeterReading>,
    age: Option<Duration>,
) -> Vec<(String, String)> {
    vec![
        ("Pol".to_owned(), value(reading, |r| percent_text(r.degree))),
        (
            "Tilt".to_owned(),
            value(reading, |r| format!("{}°", r.angle_deg.round() as i32)),
        ),
        (
            "Ellip".to_owned(),
            value(reading, |r| {
                format!("{}°", r.ellipticity_deg.round() as i32)
            }),
        ),
        (
            "Hand".to_owned(),
            value(reading, |r| hand_text(r.hand).to_owned()),
        ),
        ("Age".to_owned(), age_label(age)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(angle: f64, chi: f64, v: f64) -> f64 {
        let start = ellipse_point(0.0, angle, chi, 40.0, 48.0);
        let next = ellipse_point(sense_step(v, chi), angle, chi, 40.0, 48.0);
        (start.0 - 48.0) * (next.1 - start.1) - (start.1 - 48.0) * (next.0 - start.0)
    }

    #[test]
    fn the_ellipse_has_one_point_per_step() {
        assert_eq!(
            ellipse_points(30.0, 10.0, 40.0, 48.0, ELLIPSE_POINTS).len(),
            64
        );
        assert_eq!(ellipse_points(30.0, 10.0, 40.0, 48.0, 1).len(), 3);
    }

    #[test]
    fn forty_five_degrees_of_ellipticity_draws_a_circle() {
        let radius = 40.0 * std::f64::consts::FRAC_PI_4.cos();
        for (x, y) in ellipse_points(17.0, 45.0, 40.0, 48.0, ELLIPSE_POINTS) {
            assert!(((x - 48.0).hypot(y - 48.0) - radius).abs() < 1e-9);
        }
    }

    #[test]
    fn linear_polarisation_is_a_flat_line_tilted_by_the_angle() {
        let start = ellipse_point(0.0, 90.0, 0.0, 40.0, 48.0);
        assert!((start.0 - 48.0).abs() < 1e-9);
        assert!((start.1 - 8.0).abs() < 1e-9);
        let flat = ellipse_point(std::f64::consts::FRAC_PI_2, 0.0, 0.0, 40.0, 48.0);
        assert!((flat.0 - 48.0).abs() < 1e-9);
        assert!((flat.1 - 48.0).abs() < 1e-9);
    }

    #[test]
    fn positive_v_turns_clockwise_on_screen_and_negative_v_back() {
        for angle in [0.0, 35.0, 120.0] {
            assert!(turn(angle, 20.0, 0.4) > 0.0);
            assert!(turn(angle, -20.0, -0.4) < 0.0);
            assert!(turn(angle, -20.0, 0.4) > 0.0);
            assert!(turn(angle, 20.0, -0.4) < 0.0);
        }
    }

    #[test]
    fn only_elliptical_waves_get_an_arrowhead() {
        assert_eq!(sense_arrow(30.0, 1.0, 0.1, 40.0, 48.0), None);
        let arrow = sense_arrow(30.0, 20.0, 0.4, 40.0, 48.0).expect("an arrow");
        assert_eq!(arrow.arc[0], ellipse_point(0.0, 30.0, 20.0, 40.0, 48.0));
        let tip = arrow.head[1];
        let side = (arrow.head[0].0 - tip.0).hypot(arrow.head[0].1 - tip.1);
        assert!(side > ARROW_SIZE * 0.9 && side < ARROW_SIZE * 1.3);
    }

    #[test]
    fn hand_and_degree_read_short() {
        assert_eq!(hand_text(Hand::Right), "RH");
        assert_eq!(hand_text(Hand::Left), "LH");
        assert_eq!(hand_text(Hand::default()), "Lin");
        assert_eq!(percent_text(0.874), "87%");
        assert_eq!(percent_text(1.4), "100%");
        assert_eq!(stokes_text(-0.4567), "-0.46");
    }

    #[test]
    fn h_and_v_never_share_a_lane() {
        let params = PolarimeterParams::default();
        assert_eq!(lane_edit(&params, Which::H, params.v_lane), None);
        let next = lane_edit(&params, Which::V, 3).expect("a free lane");
        assert_eq!(next.v_lane, 3);
        assert_eq!(next.h_lane, params.h_lane);
    }

    #[test]
    fn the_output_follows_the_matched_flag() {
        assert_eq!(output_of(true), Output::Matched);
        assert_eq!(output_of(false), Output::Cross);
        assert_eq!(output_title(Output::Cross), "Beam output across the wave");
    }

    #[test]
    fn the_readouts_fill_two_columns() {
        let reading = PolarimeterReading {
            i_db: -31.24,
            q: 0.5,
            degree: 0.9,
            angle_deg: 44.6,
            ellipticity_deg: -12.2,
            hand: Hand::Left,
            snr_db: Some(21.3),
            ..PolarimeterReading::default()
        };
        let left = left_rows(Some(&reading));
        assert_eq!(left[0].1, "-31.2 dB");
        assert_eq!(left[1].1, "0.50");
        assert_eq!(left[4].1, "21 dB");
        let right = right_rows(Some(&reading), None);
        assert_eq!(
            right
                .iter()
                .map(|(_, value)| value.as_str())
                .collect::<Vec<_>>(),
            ["90%", "45°", "-12°", "LH", "-"]
        );
        assert!(left_rows(None).iter().all(|(_, value)| value == "-"));
    }
}
