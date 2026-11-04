use crate::default_true;
use sdrmm_wire::{Capability, EventContract, Hue, KindSpec, Symbol};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::FieldValue;
use sdrmm_wire::present::Place;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::bearing;
use sdrmm_wire::present::count;
use sdrmm_wire::present::feet_to_metres;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::flag;
use sdrmm_wire::present::joined;
use sdrmm_wire::present::lat_lon;
use sdrmm_wire::present::number;
use sdrmm_wire::present::position;
use sdrmm_wire::present::text;
use sdrmm_wire::present::trimmed;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdsbParams {
    #[serde(default = "default_true")]
    pub crc_fix: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_lon: Option<f64>,
}

impl Default for AdsbParams {
    fn default() -> Self {
        Self {
            crc_fix: true,
            ref_lat: None,
            ref_lon: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AdsbMessage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_12mhz: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_level: Option<u8>,
    pub icao: String,
    pub df: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_code: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callsign: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub altitude_ft: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground_speed_kt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_deg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_rate_fpm: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub squawk: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_ground: Option<bool>,
    pub raw: String,
}

impl EventPayload for AdsbMessage {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let icao = self.icao.to_ascii_uppercase();
        Presentation {
            label: trimmed(self.callsign.as_deref()).or_else(|| Some(icao.clone())),
            place: lat_lon(self.lat, self.lon).map(|at| {
                Place::at(at)
                    .heading(bearing(self.track_deg))
                    .altitude(self.altitude_ft.map(|feet| feet_to_metres(feet.into())))
            }),
            fields: adsb_fields(self, &icao),
            track: Some(track(self, &icao)),
            entity: Some(icao),
            ..Presentation::of(adsb_summary(self))
        }
    }
}

const KNOTS_TO_MPS: f64 = 0.514_444;
const FEET_PER_MINUTE_TO_MPS: f64 = 0.005_08;
const FEET_TO_M: f64 = 0.3048;

fn track(a: &AdsbMessage, icao: &str) -> sdrmm_wire::present::TrackFix {
    sdrmm_wire::present::TrackFix {
        id: icao.to_owned(),
        label: trimmed(a.callsign.as_deref()),
        lat: a.lat,
        lon: a.lon,
        altitude_m: a.altitude_ft.map(|feet| f64::from(feet) * FEET_TO_M),
        speed_mps: a.ground_speed_kt.map(|knots| knots * KNOTS_TO_MPS),
        course_deg: a.track_deg,
        climb_mps: a
            .vertical_rate_fpm
            .map(|rate| f64::from(rate) * FEET_PER_MINUTE_TO_MPS),
    }
}

fn adsb_fields(a: &AdsbMessage, icao: &str) -> Vec<Field> {
    fields(&[
        ("ICAO", Some(FieldValue::Text(icao.to_owned()))),
        ("Callsign", text(a.callsign.as_deref())),
        ("Format", count(Some(a.df))),
        ("Type code", count(a.type_code)),
        ("Altitude", number(a.altitude_ft, Unit::Feet, 0)),
        ("On ground", flag(a.on_ground)),
        ("Position", position(a.lat, a.lon)),
        ("Speed", number(a.ground_speed_kt, Unit::Knots, 1)),
        ("Track", number(a.track_deg, Unit::Degrees, 0)),
        ("Climb", number(a.vertical_rate_fpm, Unit::FeetPerMinute, 0)),
        ("Squawk", text(a.squawk.as_deref())),
        ("Raw", text(Some(&a.raw))),
    ])
}

fn adsb_summary(a: &AdsbMessage) -> String {
    let mut parts = vec![a.icao.clone()];
    if let Some(cs) = &a.callsign {
        parts.push(cs.trim().to_owned());
    }
    if let Some(alt) = a.altitude_ft {
        parts.push(format!("{alt} ft"));
    }
    if let (Some(lat), Some(lon)) = (a.lat, a.lon) {
        parts.push(format!("{lat:.4}, {lon:.4}"));
    }
    joined(parts)
}

pub(crate) fn adsb_kind() -> KindSpec {
    KindSpec::of::<AdsbMessage>("ADS-B", "Aircraft", Hue::Teal)
        .caps(&[
            Capability::Log,
            Capability::Station,
            Capability::Geo,
            Capability::Track,
        ])
        .symbol(Symbol::Plane)
        .sighting()
        .build()
}

impl EventContract for AdsbMessage {
    const KIND: EventKind = EventKind::named("adsb");
}

impl sdrmm_wire::DecoderSettings for AdsbParams {
    const TYPE_ID: &'static str = "adsb";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use crate::rtty::RttyText;
    use sdrmm_wire::DecodedRecord;
    use sdrmm_wire::DecoderEvent;
    use sdrmm_wire::LatLon;

    #[test]
    fn position_is_reported_only_when_both_coordinates_are_known() {
        let mut a = AdsbMessage {
            icao: "3C6444".to_owned(),
            lat: Some(52.5),
            ..AdsbMessage::default()
        };
        assert_eq!(
            presented(&DecoderEvent::of(&a.clone()))
                .place
                .map(|place| (place.at.lat, place.at.lon)),
            None
        );
        a.lon = Some(13.4);
        assert_eq!(
            presented(&DecoderEvent::of(&a))
                .place
                .map(|place| (place.at.lat, place.at.lon)),
            Some((52.5, 13.4))
        );
        assert_eq!(
            presented(&DecoderEvent::of(&RttyText {
                text: "CQ".to_owned()
            }))
            .place,
            None
        );
    }

    #[test]
    fn decoded_record_roundtrips() {
        let rec = DecodedRecord {
            origin: None,
            sinks: Vec::new(),
            callsign: None,
            device_set: 1,
            channel: 2,
            at: "2026-08-09T12:00:00Z".to_owned(),
            freq_hz: 1_090_000_000.0,
            event: DecoderEvent::of(&AdsbMessage {
                icao: "3C6444".to_owned(),
                df: 17,
                type_code: Some(4),
                callsign: Some("DLH123".to_owned()),
                raw: "8D3C6444".to_owned(),
                ..AdsbMessage::default()
            }),
            view: None,
        };
        let json = serde_json::to_value(&rec).unwrap();
        assert_eq!(json["event"]["kind"], "adsb");
        assert_eq!(json["freq_hz"], 1_090_000_000.0);
        let back: DecodedRecord = serde_json::from_value(json).unwrap();
        assert_eq!(back, rec);
    }

    fn presented(event: &DecoderEvent) -> Presentation {
        sdrmm_wire::EventCatalog::new([adsb_kind(), crate::rtty::rtty_kind()]).present(event)
    }

    #[test]
    fn an_aircraft_is_named_placed_and_turned() {
        let view = serde_json::from_str::<AdsbMessage>(
            r#"{"icao":"3c6444","df":17,"callsign":"DLH400  ","altitude_ft":37000,"lat":52.5,"lon":13.4,"track_deg":-90.0,"raw":"8d3c6444"}"#,
        )
        .expect("aircraft fixture")
        .present(&EventKind::named("adsb"));
        assert_eq!(view.entity.as_deref(), Some("3C6444"));
        assert_eq!(view.label.as_deref(), Some("DLH400"));
        let place = view.place.expect("placed");
        assert_eq!(
            place.at,
            sdrmm_wire::LatLon {
                lat: 52.5,
                lon: 13.4
            }
        );
        assert_eq!(place.heading_deg, Some(270.0));
        assert_eq!(place.altitude_m.map(f32::round), Some(11_278.0));
    }

    fn adsb_event(data: serde_json::Value) -> DecoderEvent {
        let mut message = serde_json::json!({ "df": 17, "raw": "8d4840d6" });
        if let (Some(base), serde_json::Value::Object(extra)) = (message.as_object_mut(), data) {
            base.extend(extra);
        }
        DecoderEvent::of(&serde_json::from_value::<AdsbMessage>(message).expect("adsb"))
    }

    #[test]
    fn partial_aircraft_frames_build_one_aircraft() {
        let mut aircraft = adsb_event(serde_json::json!({
            "icao": "abc123", "callsign": "DLH400", "type_code": 4
        }));
        let position = adsb_event(serde_json::json!({
            "icao": "abc123", "lat": 52.5, "lon": 13.4, "altitude_ft": 37_000, "type_code": 11
        }));
        assert!(sdrmm_wire::EventCatalog::new([adsb_kind()]).merge(&mut aircraft, &position));
        let Some(merged) = aircraft.read::<AdsbMessage>() else {
            panic!("still an aircraft");
        };
        assert_eq!(merged.callsign.as_deref(), Some("DLH400"));
        assert_eq!((merged.lat, merged.lon), (Some(52.5), Some(13.4)));
        assert_eq!(merged.altitude_ft, Some(37_000));
        assert_eq!(merged.type_code, Some(11));
    }

    fn adsb(json: serde_json::Value) -> Presentation {
        serde_json::from_value::<AdsbMessage>(json)
            .unwrap()
            .present(&EventKind::named("adsb"))
    }

    #[test]
    fn adsb_lists_only_what_the_frame_carried() {
        let view = adsb(serde_json::json!({ "icao": "3c6444", "df": 11, "raw": "5d" }));
        assert_eq!(view.labels(), ["ICAO", "Format", "Raw"]);
        assert_eq!(view.field("ICAO"), text(Some("3C6444")).as_ref());
        assert_eq!(view.field("Format"), count(Some(11)).as_ref());
    }

    #[test]
    fn adsb_types_every_reported_value() {
        let view = adsb(serde_json::json!({
            "icao": "3c6444", "df": 17, "raw": "8d3c6444", "callsign": " DLH123 ",
            "altitude_ft": 37000, "ground_speed_kt": 451.4, "track_deg": 271.6,
            "vertical_rate_fpm": -1088, "lat": 52.52, "lon": 13.405, "on_ground": false,
        }));
        assert_eq!(view.field("Callsign"), text(Some("DLH123")).as_ref());
        assert_eq!(
            view.field("Altitude"),
            number(Some(37000), Unit::Feet, 0).as_ref()
        );
        assert_eq!(
            view.field("Speed"),
            number(Some(451.4), Unit::Knots, 1).as_ref()
        );
        assert_eq!(
            view.field("Track"),
            number(Some(271.6), Unit::Degrees, 0).as_ref()
        );
        assert_eq!(
            view.field("Climb"),
            number(Some(-1088), Unit::FeetPerMinute, 0).as_ref()
        );
        assert_eq!(
            view.field("Position"),
            Some(&FieldValue::Position(LatLon {
                lat: 52.52,
                lon: 13.405
            }))
        );
        assert_eq!(view.field("On ground"), Some(&FieldValue::Flag(false)));
    }
}
