use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

use sdrmm_wire::LatLon;
use sdrmm_wire::geo::wrap_180;
use sdrmm_wire::present::Shape;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InmarsatStdcParams {}

const NAVAREA_LIMITS: &str = include_str!("../../data/navareas.txt");

const METRES_PER_NM: f32 = 1_852.0;

const ROMAN_TENS: [&str; 4] = ["", "X", "XX", "XXX"];

const ROMAN_ONES: [&str; 10] = ["", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX"];

/// The sea area an Inmarsat EGC call is addressed to: its C3 address field, read as geometry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SeaArea {
    Rectangle {
        south_west: LatLon,
        north_deg: u8,
        east_deg: u8,
    },
    Circle {
        center: LatLon,
        radius_nm: u16,
    },
    Navarea {
        number: u8,
    },
    Coastal {
        navarea: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        letter: Option<String>,
    },
}

impl SeaArea {
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Rectangle { south_west, .. } => format!("Area {}", corner(*south_west)),
            Self::Circle { center, radius_nm } => {
                format!("Circle {} {radius_nm} NM", corner(*center))
            }
            Self::Navarea { number } => format!("NAVAREA {}", roman(*number)),
            Self::Coastal { navarea, letter } => match letter {
                Some(letter) => format!("NAVAREA {} {letter}", roman(*navarea)),
                None => format!("NAVAREA {}", roman(*navarea)),
            },
        }
    }

    #[must_use]
    pub fn center(&self) -> Option<LatLon> {
        match self {
            Self::Rectangle {
                south_west,
                north_deg,
                east_deg,
            } => Some(LatLon {
                lat: south_west.lat + f64::from(*north_deg) / 2.0,
                lon: wrap_180(south_west.lon + f64::from(*east_deg) / 2.0),
            }),
            Self::Circle { center, .. } => Some(*center),
            Self::Navarea { number }
            | Self::Coastal {
                navarea: number, ..
            } => navarea_limits(*number).and_then(middle),
        }
    }

    #[must_use]
    pub fn shapes(&self) -> Vec<Shape> {
        match self {
            Self::Rectangle {
                south_west,
                north_deg,
                east_deg,
            } => vec![rectangle(*south_west, *north_deg, *east_deg)],
            Self::Circle { center, radius_nm } => vec![Shape::Circle {
                center: *center,
                radius_m: f32::from(*radius_nm) * METRES_PER_NM,
            }],
            Self::Navarea { number }
            | Self::Coastal {
                navarea: number, ..
            } => navarea_limits(*number)
                .map(|ring| Shape::Polygon {
                    points: ring.to_vec(),
                })
                .into_iter()
                .collect(),
        }
    }
}

/// NAVAREA I to XXI limits, simplified from the USCG layer; longitudes may pass 180 so a
/// ring that crosses the antimeridian stays one continuous polygon.
#[must_use]
pub fn navarea_limits(number: u8) -> Option<&'static [LatLon]> {
    static LIMITS: LazyLock<Vec<(u8, Vec<LatLon>)>> =
        LazyLock::new(|| NAVAREA_LIMITS.lines().filter_map(limit_line).collect());
    LIMITS
        .iter()
        .find(|(area, _)| *area == number)
        .map(|(_, ring)| ring.as_slice())
}

fn limit_line(line: &str) -> Option<(u8, Vec<LatLon>)> {
    let mut words = line.split_whitespace();
    let number = words.next()?.parse().ok()?;
    let ring = words.map(vertex).collect::<Option<Vec<_>>>()?;
    Some((number, ring))
}

fn vertex(word: &str) -> Option<LatLon> {
    let (lat, lon) = word.split_once(',')?;
    Some(LatLon {
        lat: lat.parse().ok()?,
        lon: lon.parse().ok()?,
    })
}

fn middle(ring: &[LatLon]) -> Option<LatLon> {
    let (south, north, west, east) = ring.iter().fold(
        (f64::MAX, f64::MIN, f64::MAX, f64::MIN),
        |(south, north, west, east), at| {
            (
                south.min(at.lat),
                north.max(at.lat),
                west.min(at.lon),
                east.max(at.lon),
            )
        },
    );
    (!ring.is_empty()).then(|| LatLon {
        lat: f64::midpoint(south, north),
        lon: wrap_180(f64::midpoint(west, east)),
    })
}

fn rectangle(south_west: LatLon, north_deg: u8, east_deg: u8) -> Shape {
    let north = (south_west.lat + f64::from(north_deg)).min(90.0);
    let east = south_west.lon + f64::from(east_deg);
    let at = |lat, lon| LatLon { lat, lon };
    Shape::Polygon {
        points: vec![
            at(south_west.lat, south_west.lon),
            at(south_west.lat, east),
            at(north, east),
            at(north, south_west.lon),
        ],
    }
}

fn corner(at: LatLon) -> String {
    let lat = if at.lat < 0.0 { 'S' } else { 'N' };
    let lon = if at.lon < 0.0 { 'W' } else { 'E' };
    format!("{:.0}{lat} {:.0}{lon}", at.lat.abs(), at.lon.abs())
}

#[must_use]
pub fn roman(number: u8) -> String {
    if number == 0 || number >= 40 {
        return number.to_string();
    }
    format!(
        "{}{}",
        ROMAN_TENS[usize::from(number / 10)],
        ROMAN_ONES[usize::from(number % 10)]
    )
}

impl sdrmm_wire::DecoderSettings for InmarsatStdcParams {
    const TYPE_ID: &'static str = "inmarsat_stdc";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::EventKind;
    use sdrmm_wire::present::EventPayload;
    use sdrmm_wire::present::text;

    #[test]
    fn an_addressed_sea_area_is_the_entity_place_and_shape() {
        let mut link = crate::datalink::sample(serde_json::json!({}));
        link.station = None;
        link.lat = None;
        link.lon = None;
        link.area = Some(SeaArea::Navarea { number: 12 });
        let view = link.present(&EventKind::named("inmarsat_stdc"));
        assert_eq!(view.entity.as_deref(), Some("NAVAREA XII"));
        assert_eq!(view.field("Area"), text(Some("NAVAREA XII")).as_ref());
        assert!(view.place.is_some());
        assert!(matches!(view.shapes.as_slice(), [Shape::Polygon { .. }]));
    }

    #[test]
    fn a_station_and_its_fix_win_over_the_area() {
        let mut link = crate::datalink::sample(serde_json::json!({}));
        link.area = Some(SeaArea::Navarea { number: 12 });
        let view = link.present(&EventKind::named("inmarsat_stdc"));
        assert_eq!(view.entity.as_deref(), Some(".D-AIBC"));
        assert_eq!(view.place.map(|place| place.at.lat), Some(52.5));
    }
}

#[cfg(test)]
mod tests;
