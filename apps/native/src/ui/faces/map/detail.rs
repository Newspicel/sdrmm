use sdrmm_wire::{
    decode::DecoderEvent,
    remote_id::{RemoteIdFrame, UaStatus},
    weather::RadiosondeFrame,
};

use super::targets::{label, position};
use crate::{
    decoders::views::{lora_protocol, operator_of, pilot_of, ua_type_label, ua_type_of},
    ui::{
        kit_maps::feed::Station,
        map::{Geo, geo::great_circle_km},
    },
};

#[derive(Clone, Debug, PartialEq)]
pub struct Detail {
    pub kind: &'static str,
    pub id: String,
    pub label: String,
    pub freq_hz: f64,
    pub last_seen: i64,
    pub rows: Vec<(&'static str, String)>,
}

fn hemisphere(deg: f64, positive: &str, negative: &str) -> String {
    format!(
        "{:.4}° {}",
        deg.abs(),
        if deg < 0.0 { negative } else { positive }
    )
}

#[must_use]
pub fn format_position(at: Geo) -> String {
    format!(
        "{} {}",
        hemisphere(at.lat, "N", "S"),
        hemisphere(at.lon, "E", "W")
    )
}

pub fn scalar(value: Option<f64>, digits: usize, unit: &str) -> Option<String> {
    value
        .filter(|value| value.is_finite())
        .map(|value| format!("{value:.digits$}{unit}"))
}

pub fn trimmed(value: Option<&str>) -> Option<String> {
    let text = value.unwrap_or("").trim();
    (!text.is_empty()).then(|| text.to_owned())
}

#[must_use]
pub fn heading_of(deg: Option<u16>) -> Option<f64> {
    deg.filter(|deg| *deg != 511).map(f64::from)
}

#[must_use]
pub fn course_of(deg: Option<f64>) -> Option<f64> {
    deg.filter(|deg| *deg != 360.0)
}

fn kept(entries: Vec<(&'static str, Option<String>)>) -> Vec<(&'static str, String)> {
    entries
        .into_iter()
        .filter_map(|(name, value)| value.map(|value| (name, value)))
        .collect()
}

#[must_use]
pub const fn ua_status_label(status: UaStatus) -> &'static str {
    match status {
        UaStatus::Undeclared => "-",
        UaStatus::Ground => "Ground",
        UaStatus::Airborne => "Airborne",
        UaStatus::Emergency => "Emergency",
        UaStatus::SystemFailure => "RID failure",
        UaStatus::Reserved => "Reserved",
    }
}

fn remote_id_rows(
    station: &Station,
    frame: &RemoteIdFrame,
    fix: Option<String>,
) -> Vec<(&'static str, Option<String>)> {
    let location = frame.location();
    let drone = position(station);
    let pilot = pilot_of(frame).and_then(|(lat, lon)| Geo::checked(Some(lat), Some(lon)));
    let metres = |value: Option<f32>| scalar(value.map(f64::from), 0, " m");
    vec![
        ("ID", Some(station.id.clone())),
        (
            "Type",
            ua_type_of(frame).map(|kind| ua_type_label(kind).to_owned()),
        ),
        (
            "Status",
            location.map(|found| ua_status_label(found.status).to_owned()),
        ),
        ("Position", fix),
        ("Height", metres(location.and_then(|found| found.height_m))),
        (
            "Altitude",
            metres(location.and_then(|found| found.geodetic_altitude_m)),
        ),
        (
            "Speed",
            scalar(
                location.and_then(|found| found.speed_mps).map(f64::from),
                1,
                " m/s",
            ),
        ),
        ("Pilot", pilot.map(format_position)),
        (
            "Pilot distance",
            drone.zip(pilot).and_then(|(drone, pilot)| {
                scalar(Some(great_circle_km(drone, pilot) * 1e3), 0, " m")
            }),
        ),
        ("Operator", operator_of(frame)),
        ("Link", Some(frame.transport.label().to_owned())),
    ]
}

fn radiosonde_rows(
    d: &RadiosondeFrame,
    fix: Option<String>,
) -> Vec<(&'static str, Option<String>)> {
    vec![
        ("Serial", Some(d.serial.clone())),
        ("Position", fix),
        ("Altitude", scalar(d.altitude_m, 0, " m")),
        ("Climb", scalar(d.climb_ms, 1, " m/s")),
        ("Speed", scalar(d.speed_ms, 1, " m/s")),
        ("Temp", scalar(d.temperature_c.map(f64::from), 1, " °C")),
        ("Humidity", scalar(d.humidity_pct.map(f64::from), 0, "%")),
        ("Pressure", scalar(d.pressure_hpa.map(f64::from), 1, " hPa")),
    ]
}

fn rows(station: &Station) -> Vec<(&'static str, String)> {
    let fix = position(station).map(format_position);
    kept(match &station.event {
        DecoderEvent::Adsb(d) => vec![
            ("ICAO", Some(d.icao.to_uppercase())),
            ("Position", fix),
            ("Altitude", scalar(d.altitude_ft.map(f64::from), 0, " ft")),
            ("Speed", scalar(d.ground_speed_kt, 0, " kt")),
            ("Track", scalar(d.track_deg, 0, "°")),
            ("V/S", scalar(d.vertical_rate_fpm.map(f64::from), 0, " fpm")),
            ("Squawk", trimmed(d.squawk.as_deref())),
            (
                "State",
                (d.on_ground == Some(true)).then(|| "on ground".to_owned()),
            ),
        ],
        DecoderEvent::Ais(d) => vec![
            ("MMSI", Some(d.mmsi.to_string())),
            ("Position", fix),
            ("SOG", scalar(d.sog_kt, 1, " kt")),
            ("COG", scalar(course_of(d.cog_deg), 0, "°")),
            ("Heading", scalar(heading_of(d.heading_deg), 0, "°")),
            ("Call sign", trimmed(d.call_sign.as_deref())),
            ("Destination", trimmed(d.destination.as_deref())),
        ],
        DecoderEvent::Aprs(d) => vec![
            ("Source", Some(d.source.clone())),
            ("Position", fix),
            ("Speed", scalar(d.speed_kt, 0, " kt")),
            ("Course", scalar(d.course_deg, 0, "°")),
            ("Altitude", scalar(d.altitude_ft.map(f64::from), 0, " ft")),
            ("Message", trimmed(d.mic_e_message.as_deref())),
            ("Comment", trimmed(d.comment.as_deref())),
        ],
        DecoderEvent::Radiosonde(d) => radiosonde_rows(d, fix),
        DecoderEvent::Lora(d) => vec![
            ("Node", Some(station.id.clone())),
            ("Protocol", Some(lora_protocol(d).to_owned())),
            ("Position", fix),
            ("SNR", scalar(Some(f64::from(d.snr_db)), 1, " dB")),
        ],
        DecoderEvent::RemoteId(d) => remote_id_rows(station, d, fix),
        _ => Vec::new(),
    })
}

#[must_use]
pub fn detail(station: &Station) -> Detail {
    let mut rows = rows(station);
    rows.push(("Frames", station.frames.to_string()));
    Detail {
        kind: station.kind,
        id: station.id.clone(),
        label: label(station),
        freq_hz: station.freq_hz,
        last_seen: station.last_seen,
        rows,
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{
        decode::{AdsbMessage, AisMessage},
        remote_id::{RemoteIdMessage, UaType},
    };

    use super::*;
    use crate::ui::kit_maps::fixtures::{drone, location, pilot};

    fn station(event: DecoderEvent) -> Station {
        Station {
            kind: event.kind(),
            id: "target".to_owned(),
            event,
            last_seen: 1,
            freq_hz: 1_090_000_000.0,
            frames: 12,
        }
    }

    #[test]
    fn a_detail_lists_only_what_the_target_reported() {
        let target = station(DecoderEvent::Adsb(AdsbMessage {
            icao: "3c6444".to_owned(),
            lat: Some(-33.8688),
            lon: Some(151.2093),
            altitude_ft: Some(37_000),
            track_deg: Some(89.4),
            ..AdsbMessage::default()
        }));
        let shown = detail(&target);
        let rows: Vec<(&str, &str)> = shown
            .rows
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                ("ICAO", "3C6444"),
                ("Position", "33.8688° S 151.2093° E"),
                ("Altitude", "37000 ft"),
                ("Track", "89°"),
                ("Frames", "12"),
            ]
        );
        let ship = detail(&Station {
            id: "211234560".to_owned(),
            ..station(DecoderEvent::Ais(AisMessage {
                mmsi: 211_234_560,
                name: Some("NORDIC".to_owned()),
                ..AisMessage::default()
            }))
        });
        assert_eq!(
            (ship.kind, ship.id.as_str(), ship.label.as_str()),
            ("ais", "211234560", "NORDIC")
        );
        assert_eq!(ship.freq_hz, 1_090_000_000.0);
        assert_eq!(ship.last_seen, 1);
    }

    #[test]
    fn a_drone_card_names_its_pilot_and_how_far_away_they_stand() {
        let frame = drone(
            Some("1596F"),
            vec![
                RemoteIdMessage::Location(location(47.0, 8.0)),
                RemoteIdMessage::System(pilot(47.001, 8.0)),
                RemoteIdMessage::BasicId {
                    id_type: sdrmm_wire::remote_id::UasIdType::SerialNumber,
                    ua_type: UaType::Rotorcraft,
                    uas_id: "1596F".to_owned(),
                },
            ],
        );
        let shown = detail(&Station {
            id: "1596F".to_owned(),
            ..station(DecoderEvent::RemoteId(frame))
        });
        let row = |name: &str| {
            shown
                .rows
                .iter()
                .find(|(held, _)| *held == name)
                .map(|(_, value)| value.clone())
        };
        assert_eq!(row("Type").as_deref(), Some("Multirotor"));
        assert_eq!(row("Status").as_deref(), Some("Airborne"));
        assert_eq!(row("Pilot distance").as_deref(), Some("111 m"));
        assert_eq!(row("Link").as_deref(), Some("BT4"));
        assert_eq!(row("Height").as_deref(), Some("42 m"));
    }
}
