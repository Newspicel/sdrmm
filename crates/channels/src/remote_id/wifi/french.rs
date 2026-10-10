use sdrmm_wire::{
    HeightReference, OperatorLocationType, RemoteIdLocation, RemoteIdMessage, RemoteIdSystem,
    UaStatus, UaType, UasIdType,
};

pub(crate) const OUI: [u8; 3] = [0x6A, 0x5C, 0x35];
pub(crate) const TYPE: u8 = 0x01;
const VERSION: u8 = 1;
const IDENTIFIER: u8 = 2;
const SERIAL: u8 = 3;
const LAT: u8 = 4;
const LON: u8 = 5;
const ALTITUDE: u8 = 6;
const HEIGHT: u8 = 7;
const TAKEOFF_LAT: u8 = 8;
const TAKEOFF_LON: u8 = 9;
const SPEED: u8 = 10;
const COURSE: u8 = 11;
const SCALE: f64 = 1e5;

#[derive(Default)]
struct Fields {
    identifier: Option<String>,
    serial: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
    altitude: Option<f32>,
    height: Option<f32>,
    takeoff_lat: Option<f64>,
    takeoff_lon: Option<f64>,
    speed: Option<f32>,
    course: Option<f32>,
}

pub(crate) fn messages(tlvs: &[u8]) -> Option<Vec<RemoteIdMessage>> {
    let fields = fields(tlvs)?;
    let mut messages = Vec::new();
    for (id_type, id) in [
        (UasIdType::SerialNumber, &fields.serial),
        (UasIdType::FrenchIdentifier, &fields.identifier),
    ] {
        if let Some(id) = id {
            messages.push(RemoteIdMessage::BasicId {
                id_type,
                ua_type: UaType::None,
                uas_id: id.clone(),
            });
        }
    }
    if fields.lat.is_some() || fields.height.is_some() {
        messages.push(RemoteIdMessage::Location(location(&fields)));
    }
    if fields.takeoff_lat.is_some() {
        messages.push(RemoteIdMessage::System(RemoteIdSystem {
            operator_location_type: OperatorLocationType::Takeoff,
            operator_lat: fields.takeoff_lat,
            operator_lon: fields.takeoff_lon,
            operator_altitude_m: None,
            area_count: 0,
            area_radius_m: 0,
            area_ceiling_m: None,
            area_floor_m: None,
            classification: None,
            timestamp: None,
        }));
    }
    Some(messages)
}

fn location(fields: &Fields) -> RemoteIdLocation {
    RemoteIdLocation {
        status: UaStatus::Undeclared,
        lat: fields.lat,
        lon: fields.lon,
        track_deg: fields.course,
        speed_mps: fields.speed,
        vertical_speed_mps: None,
        pressure_altitude_m: None,
        geodetic_altitude_m: fields.altitude,
        height_m: fields.height,
        height_reference: HeightReference::Takeoff,
        horizontal_accuracy_m: None,
        vertical_accuracy_m: None,
        pressure_accuracy_m: None,
        speed_accuracy_mps: None,
        seconds_after_hour: None,
        timestamp_accuracy_s: None,
    }
}

fn fields(tlvs: &[u8]) -> Option<Fields> {
    let mut fields = Fields::default();
    let mut rest = tlvs;
    let mut versioned = false;
    while let [kind, length, tail @ ..] = rest {
        let (value, next) = tail.split_at_checked(usize::from(*length))?;
        match *kind {
            VERSION => versioned = value == [VERSION],
            IDENTIFIER => fields.identifier = text(value),
            SERIAL => fields.serial = text(value),
            LAT => fields.lat = degrees(value, 90.0),
            LON => fields.lon = degrees(value, 180.0),
            ALTITUDE => fields.altitude = metres(value),
            HEIGHT => fields.height = metres(value),
            TAKEOFF_LAT => fields.takeoff_lat = degrees(value, 90.0),
            TAKEOFF_LON => fields.takeoff_lon = degrees(value, 180.0),
            SPEED => fields.speed = value.first().map(|&speed| f32::from(speed as i8)),
            COURSE => fields.course = metres(value).filter(|course| (0.0..360.0).contains(course)),
            _ => {}
        }
        rest = next;
    }
    versioned.then_some(fields)
}

fn degrees(value: &[u8], limit: f64) -> Option<f64> {
    let raw = i32::from_be_bytes(value.try_into().ok()?);
    let degrees = f64::from(raw) / SCALE;
    (degrees.abs() <= limit).then_some(degrees)
}

fn metres(value: &[u8]) -> Option<f32> {
    Some(f32::from(i16::from_be_bytes(value.try_into().ok()?)))
}

fn text(value: &[u8]) -> Option<String> {
    let end = value.iter().position(|&b| b == 0).unwrap_or(value.len());
    let text: String = value[..end]
        .iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                char::from(b)
            } else {
                '?'
            }
        })
        .collect();
    let text = text.trim().to_owned();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{RemoteIdMessage, UasIdType};

    use super::messages;

    fn tlv(kind: u8, value: &[u8]) -> Vec<u8> {
        let mut out = vec![kind, value.len() as u8];
        out.extend_from_slice(value);
        out
    }

    #[test]
    fn a_french_beacon_gives_id_position_and_takeoff() {
        let mut identifier = [0u8; 30];
        identifier[..16].copy_from_slice(b"ILLDRONE00000001");
        let tlvs = [
            tlv(1, &[1]),
            tlv(2, &identifier),
            tlv(3, b"1581F4XFC2342001"),
            tlv(4, &4_884_221i32.to_be_bytes()),
            tlv(5, &233_550i32.to_be_bytes()),
            tlv(6, &154i16.to_be_bytes()),
            tlv(7, &40i16.to_be_bytes()),
            tlv(8, &4_884_100i32.to_be_bytes()),
            tlv(9, &233_400i32.to_be_bytes()),
            tlv(10, &[7]),
            tlv(11, &275i16.to_be_bytes()),
        ]
        .concat();
        let found = messages(&tlvs).unwrap();
        assert_eq!(found.len(), 4);
        assert!(matches!(
            &found[0],
            RemoteIdMessage::BasicId { id_type: UasIdType::SerialNumber, uas_id, .. } if uas_id == "1581F4XFC2342001"
        ));
        assert!(matches!(
            &found[1],
            RemoteIdMessage::BasicId { id_type: UasIdType::FrenchIdentifier, uas_id, .. } if uas_id == "ILLDRONE00000001"
        ));
        let RemoteIdMessage::Location(location) = &found[2] else {
            panic!("{found:?}");
        };
        assert!((location.lat.unwrap() - 48.842_21).abs() < 1e-9);
        assert!((location.lon.unwrap() - 2.335_5).abs() < 1e-9);
        assert_eq!(location.geodetic_altitude_m, Some(154.0));
        assert_eq!(location.height_m, Some(40.0));
        assert_eq!(location.speed_mps, Some(7.0));
        assert_eq!(location.track_deg, Some(275.0));
        let RemoteIdMessage::System(system) = &found[3] else {
            panic!("{found:?}");
        };
        assert!((system.operator_lat.unwrap() - 48.841).abs() < 1e-9);
    }

    #[test]
    fn an_unknown_version_is_refused() {
        assert!(messages(&tlv(1, &[2])).is_none());
        assert!(messages(&[1, 5, 1]).is_none());
    }
}
