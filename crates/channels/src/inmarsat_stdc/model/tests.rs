use super::*;
use sdrmm_wire::LatLon;
use sdrmm_wire::present::Shape;

fn at(lat: f64, lon: f64) -> LatLon {
    LatLon { lat, lon }
}

fn polygon(shapes: Vec<Shape>) -> Vec<LatLon> {
    match <[Shape; 1]>::try_from(shapes) {
        Ok([Shape::Polygon { points }]) => points,
        other => panic!("not one polygon: {other:?}"),
    }
}

#[test]
fn every_navarea_has_limits() {
    for number in 1..=21 {
        let ring = navarea_limits(number).unwrap_or_else(|| panic!("NAVAREA {number}"));
        assert!(ring.len() >= 4, "{number}");
        assert!(
            ring.iter()
                .all(|at| (-90.0..=90.0).contains(&at.lat) && (-360.0..=360.0).contains(&at.lon)),
            "{number}"
        );
    }
    assert_eq!(navarea_limits(0), None);
    assert_eq!(navarea_limits(22), None);
}

#[test]
fn a_navarea_draws_its_limits_and_sits_inside_them() {
    let area = SeaArea::Navarea { number: 1 };
    assert_eq!(area.label(), "NAVAREA I");
    let ring = polygon(area.shapes());
    assert!(ring.contains(&at(48.45, -35.0)));
    let middle = area.center().expect("centre");
    assert!((40.0..80.0).contains(&middle.lat) && (-35.0..35.0).contains(&middle.lon));
}

#[test]
fn an_antimeridian_navarea_stays_one_ring_with_a_wrapped_centre() {
    let area = SeaArea::Navarea { number: 14 };
    let ring = polygon(area.shapes());
    assert!(ring.iter().any(|at| at.lon > 180.0));
    let middle = area.center().expect("centre");
    assert!((-180.0..=180.0).contains(&middle.lon));
    assert!((middle.lon - -160.0).abs() < 1e-9);
}

#[test]
fn a_coastal_area_draws_its_navarea() {
    let area = SeaArea::Coastal {
        navarea: 2,
        letter: Some("C".to_owned()),
    };
    assert_eq!(area.label(), "NAVAREA II C");
    assert_eq!(area.shapes(), SeaArea::Navarea { number: 2 }.shapes());
}

#[test]
fn a_rectangle_spans_its_extents() {
    let area = SeaArea::Rectangle {
        south_west: at(60.0, -10.0),
        north_deg: 30,
        east_deg: 25,
    };
    assert_eq!(area.label(), "Area 60N 10W");
    assert_eq!(
        polygon(area.shapes()),
        [
            at(60.0, -10.0),
            at(60.0, 15.0),
            at(90.0, 15.0),
            at(90.0, -10.0)
        ]
    );
    assert_eq!(area.center(), Some(at(75.0, 2.5)));
}

#[test]
fn a_rectangle_over_the_antimeridian_stays_continuous() {
    let area = SeaArea::Rectangle {
        south_west: at(-20.0, 170.0),
        north_deg: 10,
        east_deg: 30,
    };
    assert_eq!(polygon(area.shapes())[1], at(-20.0, 200.0));
    assert_eq!(area.center(), Some(at(-15.0, -175.0)));
}

#[test]
fn a_circle_is_drawn_in_metres() {
    let area = SeaArea::Circle {
        center: at(14.0, -66.0),
        radius_nm: 300,
    };
    assert_eq!(area.label(), "Circle 14N 66W 300 NM");
    assert_eq!(
        area.shapes(),
        [Shape::Circle {
            center: at(14.0, -66.0),
            radius_m: 555_600.0
        }]
    );
}

#[test]
fn roman_numerals_cover_the_navareas() {
    assert_eq!(roman(12), "XII");
    assert_eq!(roman(17), "XVII");
    assert_eq!(roman(21), "XXI");
    assert_eq!(roman(0), "0");
}
