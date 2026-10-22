use super::{
    EventPayload, Field, Place, Presentation, Unit, bearing, feet_to_metres, fields,
    knots_to_kilometres_per_hour, lat_lon, number, position, text, trimmed,
};
use crate::{
    decode::{AprsPacket, EventKind},
    weather::AprsWeather,
};

impl EventPayload for AprsPacket {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            place: lat_lon(self.lat, self.lon).map(|at| {
                Place::at(at)
                    .heading(bearing(self.course_deg))
                    .altitude(self.altitude_ft.map(|feet| feet_to_metres(feet.into())))
            }),
            entity: trimmed(Some(&self.source)),
            callsign: trimmed(Some(&self.source)),
            fields: aprs_fields(self),
            ..Presentation::of(aprs_summary(self))
        }
    }
}

fn aprs_fields(p: &AprsPacket) -> Vec<Field> {
    let mut shown = fields(&[
        ("Source", text(Some(&p.source))),
        ("Destination", text(Some(&p.destination))),
        ("Path", text(Some(&p.path.join(" → ")))),
        ("Symbol", text(p.symbol.as_deref())),
        ("Position", position(p.lat, p.lon)),
        ("Course", number(p.course_deg, Unit::Degrees, 0)),
        (
            "Speed",
            number(
                p.speed_kt.map(knots_to_kilometres_per_hour),
                Unit::KilometresPerHour,
                1,
            ),
        ),
        (
            "Altitude",
            number(
                p.altitude_ft.map(|feet| feet_to_metres(feet.into())),
                Unit::Metres,
                0,
            ),
        ),
        ("Message", text(p.mic_e_message.as_deref())),
        ("Comment", text(p.comment.as_deref())),
    ]);
    shown.extend(p.weather.as_ref().map(weather_fields).unwrap_or_default());
    shown.extend(fields(&[("Packet", text(Some(&p.tnc2)))]));
    shown
}

fn weather_fields(w: &AprsWeather) -> Vec<Field> {
    fields(&[
        ("Wind", number(w.wind_speed_ms, Unit::MetresPerSecond, 1)),
        ("Wind from", number(w.wind_dir_deg, Unit::Degrees, 0)),
        ("Gust", number(w.wind_gust_ms, Unit::MetresPerSecond, 1)),
        ("Temperature", number(w.temperature_c, Unit::Celsius, 1)),
        ("Humidity", number(w.humidity_pct, Unit::Percent, 0)),
        ("Pressure", number(w.pressure_hpa, Unit::Hectopascal, 1)),
        ("Rain 1 h", number(w.rain_1h_mm, Unit::Millimetres, 1)),
        ("Rain 24 h", number(w.rain_24h_mm, Unit::Millimetres, 1)),
        (
            "Rain today",
            number(w.rain_midnight_mm, Unit::Millimetres, 1),
        ),
        ("Snow 24 h", number(w.snow_24h_mm, Unit::Millimetres, 1)),
        (
            "Light",
            number(w.luminosity_wm2, Unit::WattsPerSquareMetre, 0),
        ),
    ])
}

fn aprs_summary(p: &AprsPacket) -> String {
    let weather = p.weather.as_ref().map(AprsWeather::summary);
    match (&p.mic_e_message, weather) {
        (Some(message), _) => format!("{} · {message}", p.tnc2),
        (None, Some(weather)) if !weather.is_empty() => format!("{} · {weather}", p.source),
        _ => p.tnc2.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LatLon, present::FieldValue};

    fn aprs(json: serde_json::Value) -> Presentation {
        serde_json::from_value::<AprsPacket>(json)
            .unwrap()
            .present(EventKind::Aprs)
    }

    #[test]
    fn aprs_carries_what_the_monitor_line_packs_away() {
        let view = aprs(serde_json::json!({
            "source": "DL1ABC-9", "destination": "S32U6T", "path": ["WIDE1-1", "WIDE2-1"],
            "info": "`(_fn\"Oj/", "tnc2": "DL1ABC-9>S32U6T:`(_fn\"Oj/",
            "lat": 52.52, "lon": 13.405, "course_deg": 251.0, "speed_kt": 20.0,
            "altitude_ft": 1500, "mic_e_message": "En Route",
        }));
        assert_eq!(view.field("Source"), text(Some("DL1ABC-9")).as_ref());
        assert_eq!(view.field("Path"), text(Some("WIDE1-1 → WIDE2-1")).as_ref());
        assert_eq!(
            view.field("Position"),
            Some(&FieldValue::Position(LatLon {
                lat: 52.52,
                lon: 13.405
            }))
        );
        assert_eq!(
            view.field("Course"),
            number(Some(251.0), Unit::Degrees, 0).as_ref()
        );
        assert_eq!(
            view.field("Speed"),
            number(
                Some(knots_to_kilometres_per_hour(20.0)),
                Unit::KilometresPerHour,
                1
            )
            .as_ref()
        );
        assert_eq!(
            view.field("Altitude"),
            number(Some(feet_to_metres(1500.0)), Unit::Metres, 0).as_ref()
        );
        assert_eq!(view.field("Message"), text(Some("En Route")).as_ref());
        assert_eq!(
            view.field("Packet"),
            text(Some("DL1ABC-9>S32U6T:`(_fn\"Oj/")).as_ref()
        );
    }

    #[test]
    fn aprs_weather_comes_with_units() {
        let view = aprs(serde_json::json!({
            "source": "DL1WX", "destination": "APRS", "info": "_", "tnc2": "DL1WX>APRS:_",
            "weather": {
                "wind_dir_deg": 220, "wind_speed_ms": 4.5, "wind_gust_ms": 9.0,
                "temperature_c": 12.25, "humidity_pct": 81, "pressure_hpa": 1013.2,
                "rain_1h_mm": 0.5, "luminosity_wm2": 300,
            },
        }));
        assert_eq!(
            view.labels(),
            [
                "Source",
                "Destination",
                "Wind",
                "Wind from",
                "Gust",
                "Temperature",
                "Humidity",
                "Pressure",
                "Rain 1 h",
                "Light",
                "Packet"
            ]
        );
        assert_eq!(
            view.field("Wind"),
            number(Some(4.5), Unit::MetresPerSecond, 1).as_ref()
        );
        assert_eq!(
            view.field("Wind from"),
            number(Some(220), Unit::Degrees, 0).as_ref()
        );
        assert_eq!(
            view.field("Rain 1 h"),
            number(Some(0.5), Unit::Millimetres, 1).as_ref()
        );
        assert_eq!(
            view.field("Light"),
            number(Some(300), Unit::WattsPerSquareMetre, 0).as_ref()
        );
    }
}
