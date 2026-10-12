use super::{
    EuCategory, EuClassification, RemoteIdFrame, RemoteIdLocation, RemoteIdMessage, RemoteIdSystem,
    UaStatus, UaType,
};
use crate::{
    geo::distance_m,
    present::{Field, Unit, count, fields, lat_lon, number, position, text, text_of},
};

impl RemoteIdFrame {
    pub(super) fn fields(&self) -> Vec<Field> {
        let mut shown = fields(&[
            ("UAS ID", text(self.uas_id.as_deref())),
            (
                "Type",
                self.ua_type().and_then(UaType::label).and_then(text_of),
            ),
        ]);
        shown.extend(self.location().map(location_fields).unwrap_or_default());
        shown.extend(
            self.system()
                .map(|system| self.pilot_fields(system))
                .unwrap_or_default(),
        );
        shown.extend(fields(&[
            ("Operator", text(self.operator_id())),
            ("Description", text(self.self_id())),
            ("Auth pages", count(self.auth_pages())),
            ("Link", text_of(self.transport.label())),
            ("PHY", text_of(self.phy.label())),
            ("Address", text(Some(&self.address))),
            ("Channel", count(self.channel)),
            ("SSID", text(self.ssid.as_deref())),
            ("Counter", count(self.counter)),
            ("Level", number(Some(self.level_dbfs), Unit::Dbfs, 1)),
        ]));
        shown
    }

    fn pilot_fields(&self, system: &RemoteIdSystem) -> Vec<Field> {
        let drone = self.location().and_then(|at| lat_lon(at.lat, at.lon));
        let pilot = lat_lon(system.operator_lat, system.operator_lon);
        let distance = drone
            .zip(pilot)
            .map(|(drone, pilot)| distance_m(drone, pilot));
        fields(&[
            ("Pilot", position(system.operator_lat, system.operator_lon)),
            ("Pilot distance", number(distance, Unit::Metres, 0)),
            (
                "Pilot altitude",
                number(system.operator_altitude_m, Unit::Metres, 0),
            ),
            ("EU class", system.classification.and_then(eu_class)),
        ])
    }

    fn ua_type(&self) -> Option<UaType> {
        self.messages.iter().find_map(|message| match message {
            RemoteIdMessage::BasicId { ua_type, .. } => Some(*ua_type),
            _ => None,
        })
    }

    fn operator_id(&self) -> Option<&str> {
        self.messages.iter().find_map(|message| match message {
            RemoteIdMessage::OperatorId { operator_id, .. } => filled(operator_id),
            _ => None,
        })
    }

    fn self_id(&self) -> Option<&str> {
        self.messages.iter().find_map(|message| match message {
            RemoteIdMessage::SelfId { text, .. } => filled(text),
            _ => None,
        })
    }

    fn auth_pages(&self) -> Option<u32> {
        let pages = self
            .messages
            .iter()
            .filter(|message| matches!(message, RemoteIdMessage::Authentication { .. }))
            .count();
        u32::try_from(pages).ok().filter(|pages| *pages > 0)
    }
}

fn filled(text: &str) -> Option<&str> {
    Some(text).filter(|text| !text.trim().is_empty())
}

fn location_fields(at: &RemoteIdLocation) -> Vec<Field> {
    fields(&[
        ("Status", at.status.label().and_then(text_of)),
        ("Position", position(at.lat, at.lon)),
        ("Height", number(at.height_m, Unit::Metres, 1)),
        ("Altitude", number(at.geodetic_altitude_m, Unit::Metres, 1)),
        (
            "Pressure altitude",
            number(at.pressure_altitude_m, Unit::Metres, 1),
        ),
        ("Speed", number(at.speed_mps, Unit::MetresPerSecond, 2)),
        ("Track", number(at.track_deg, Unit::Degrees, 0)),
        (
            "Climb",
            number(at.vertical_speed_mps, Unit::MetresPerSecond, 1),
        ),
        (
            "Accuracy",
            number(at.horizontal_accuracy_m, Unit::Metres, 0),
        ),
        ("Past hour", number(at.seconds_after_hour, Unit::Seconds, 1)),
    ])
}

fn eu_class(classification: EuClassification) -> Option<crate::present::FieldValue> {
    let category = classification.category.label();
    match classification.class {
        Some(class) => text_of(format!("{category} C{class}")),
        None => text_of(category),
    }
}

impl UaType {
    const fn label(self) -> Option<&'static str> {
        Some(match self {
            Self::None => return None,
            Self::Aeroplane => "Aeroplane",
            Self::Rotorcraft => "Multirotor",
            Self::Gyroplane => "Gyroplane",
            Self::HybridLift => "VTOL",
            Self::Ornithopter => "Ornithopter",
            Self::Glider => "Glider",
            Self::Kite => "Kite",
            Self::FreeBalloon => "Balloon",
            Self::CaptiveBalloon => "Captive balloon",
            Self::Airship => "Airship",
            Self::Parachute => "Parachute",
            Self::Rocket => "Rocket",
            Self::TetheredAircraft => "Tethered",
            Self::GroundObstacle => "Obstacle",
            Self::Other => "Other",
        })
    }
}

impl UaStatus {
    const fn label(self) -> Option<&'static str> {
        Some(match self {
            Self::Undeclared => return None,
            Self::Ground => "Ground",
            Self::Airborne => "Airborne",
            Self::Emergency => "Emergency",
            Self::SystemFailure => "RID failure",
            Self::Reserved => "Reserved",
        })
    }
}

impl EuCategory {
    const fn label(self) -> &'static str {
        match self {
            Self::Undeclared => "undeclared",
            Self::Open => "open",
            Self::Specific => "specific",
            Self::Certified => "certified",
            Self::Reserved => "reserved",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        decode::EventKind,
        present::EventPayload,
        remote_id::{HeightReference, OperatorLocationType, RemoteIdPhy, RemoteIdTransport},
    };

    fn location() -> RemoteIdLocation {
        RemoteIdLocation {
            status: UaStatus::Ground,
            lat: Some(47.198_710_3),
            lon: Some(8.730_237_1),
            track_deg: Some(84.0),
            speed_mps: None,
            vertical_speed_mps: None,
            pressure_altitude_m: None,
            geodetic_altitude_m: None,
            height_m: Some(0.0),
            height_reference: HeightReference::Takeoff,
            horizontal_accuracy_m: None,
            vertical_accuracy_m: None,
            pressure_accuracy_m: None,
            speed_accuracy_mps: None,
            seconds_after_hour: None,
            timestamp_accuracy_s: None,
        }
    }

    fn system() -> RemoteIdSystem {
        RemoteIdSystem {
            operator_location_type: OperatorLocationType::Takeoff,
            operator_lat: Some(47.198_710_3),
            operator_lon: Some(8.731_237_1),
            operator_altitude_m: None,
            area_count: 1,
            area_radius_m: 0,
            area_ceiling_m: None,
            area_floor_m: None,
            classification: Some(EuClassification {
                category: EuCategory::Open,
                class: Some(1),
            }),
            timestamp: None,
        }
    }

    fn frame(messages: Vec<RemoteIdMessage>) -> RemoteIdFrame {
        RemoteIdFrame {
            transport: RemoteIdTransport::WifiBeacon,
            phy: RemoteIdPhy::Ofdm,
            address: "90:3A:E6:5B:C8:A8".to_owned(),
            channel: None,
            counter: None,
            uas_id: Some("1588E040445AC004418".to_owned()),
            ssid: None,
            level_dbfs: -50.0,
            messages,
        }
    }

    #[test]
    fn a_drone_lists_its_place_link_and_pilot() {
        let view = frame(vec![
            RemoteIdMessage::BasicId {
                id_type: crate::remote_id::UasIdType::SerialNumber,
                ua_type: UaType::Rotorcraft,
                uas_id: "1588E040445AC004418".to_owned(),
            },
            RemoteIdMessage::Location(location()),
            RemoteIdMessage::System(system()),
            RemoteIdMessage::OperatorId {
                id_type: 0,
                operator_id: "FIN87astrdge12k8".to_owned(),
            },
        ])
        .present(EventKind::RemoteId);
        assert_eq!(
            view.labels(),
            [
                "UAS ID",
                "Type",
                "Status",
                "Position",
                "Height",
                "Track",
                "Pilot",
                "Pilot distance",
                "EU class",
                "Operator",
                "Link",
                "PHY",
                "Address",
                "Level"
            ]
        );
        assert_eq!(view.field("Type"), text(Some("Multirotor")).as_ref());
        assert_eq!(view.field("Status"), text(Some("Ground")).as_ref());
        assert_eq!(view.field("Link"), text(Some("Wi-Fi beacon")).as_ref());
        assert_eq!(view.field("EU class"), text(Some("open C1")).as_ref());
        let Some(crate::present::FieldValue::Number { value, .. }) = view.field("Pilot distance")
        else {
            panic!("pilot distance");
        };
        assert!((value - 75.6).abs() < 1.0, "{value}");
    }

    #[test]
    fn undeclared_type_and_status_stay_hidden() {
        let view = frame(vec![
            RemoteIdMessage::BasicId {
                id_type: crate::remote_id::UasIdType::None,
                ua_type: UaType::None,
                uas_id: String::new(),
            },
            RemoteIdMessage::Location(RemoteIdLocation {
                status: UaStatus::Undeclared,
                ..location()
            }),
        ])
        .present(EventKind::RemoteId);
        assert_eq!(view.field("Type"), None);
        assert_eq!(view.field("Status"), None);
        assert_eq!(view.field("Pilot"), None);
    }

    #[test]
    fn an_empty_operator_id_gives_way_to_a_later_one() {
        let view = frame(vec![
            RemoteIdMessage::OperatorId {
                id_type: 0,
                operator_id: String::new(),
            },
            RemoteIdMessage::OperatorId {
                id_type: 0,
                operator_id: "FIN87astrdge12k8".to_owned(),
            },
        ])
        .present(EventKind::RemoteId);
        assert_eq!(
            view.field("Operator"),
            text(Some("FIN87astrdge12k8")).as_ref()
        );
    }
}
