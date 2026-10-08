use std::f64::consts::PI;

use sdrmm_wire::{
    HeightReference, OperatorLocationType, RemoteIdLocation, RemoteIdMessage, RemoteIdSystem,
    UaStatus, UaType, UasIdType,
};

pub(crate) const OUIS: [[u8; 3]; 4] = [
    [0x60, 0x60, 0x1F],
    [0x48, 0x1C, 0xB9],
    [0x34, 0xD2, 0x62],
    [0x26, 0x37, 0x12],
];
const FLIGHT_INFO: u8 = 0x10;
const HEADER: usize = 8;
const SERIAL: usize = 16;
const UUID: usize = 20;
const V1_LEN: usize = 74;
const V2_LEN: usize = 86;
const PRIVATE_OPERATOR_ID: u8 = 201;

pub(crate) fn is_drone_id(data: &[u8]) -> bool {
    OUIS.iter().any(|oui| data.starts_with(oui))
        && data.get(6) == Some(&FLIGHT_INFO)
        && matches!(data.get(7), Some(1 | 2))
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn i16(&self, at: usize) -> i16 {
        i16::from_le_bytes([self.0[at], self.0[at + 1]])
    }

    fn i32(&self, at: usize) -> i32 {
        i32::from_le_bytes([self.0[at], self.0[at + 1], self.0[at + 2], self.0[at + 3]])
    }

    fn degrees(&self, at: usize, limit: f64) -> Option<f64> {
        let raw = self.i32(at);
        let degrees = f64::from(raw) / 1e7 * 180.0 / PI;
        (raw != 0 && degrees.abs() <= limit).then_some(degrees)
    }

    fn text(&self, at: usize, len: usize) -> String {
        self.0[at..at + len]
            .iter()
            .filter(|b| b.is_ascii_graphic())
            .map(|&b| char::from(b))
            .collect()
    }
}

pub(crate) fn messages(data: &[u8]) -> Option<Vec<RemoteIdMessage>> {
    let version = *data.get(7)?;
    let body = data.get(HEADER..)?;
    let len = if version == 1 { V1_LEN } else { V2_LEN };
    let fields = Reader(body.get(..len)?);
    let serial = fields.text(4, SERIAL);
    if serial.is_empty() {
        return None;
    }
    let north = f32::from(fields.i16(32)) / 100.0;
    let east = f32::from(fields.i16(34)) / 100.0;
    let (yaw_at, operator_at, home_at, uuid_at) = if version == 1 {
        (42, None, 44, 54)
    } else {
        (38, Some(48), 56, 66)
    };
    let yaw = f32::from(fields.i16(yaw_at)) / 100.0;
    let mut messages = vec![
        RemoteIdMessage::BasicId {
            id_type: UasIdType::SerialNumber,
            ua_type: UaType::Rotorcraft,
            uas_id: serial,
        },
        RemoteIdMessage::Location(RemoteIdLocation {
            status: UaStatus::Undeclared,
            lat: fields.degrees(24, 90.0),
            lon: fields.degrees(20, 180.0),
            track_deg: Some(if north == 0.0 && east == 0.0 {
                yaw.rem_euclid(360.0)
            } else {
                east.atan2(north).to_degrees().rem_euclid(360.0)
            }),
            speed_mps: Some(north.hypot(east)),
            vertical_speed_mps: Some(f32::from(fields.i16(36)) / 100.0),
            pressure_altitude_m: None,
            geodetic_altitude_m: Some(f32::from(fields.i16(28))),
            height_m: Some(f32::from(fields.i16(30)) / 10.0),
            height_reference: HeightReference::Takeoff,
            horizontal_accuracy_m: None,
            vertical_accuracy_m: None,
            pressure_accuracy_m: None,
            speed_accuracy_mps: None,
            seconds_after_hour: None,
            timestamp_accuracy_s: None,
        }),
    ];
    let (location_type, lat_at) = match operator_at {
        Some(at) => (OperatorLocationType::LiveGnss, at),
        None => (OperatorLocationType::Takeoff, home_at + 4),
    };
    let lon_at = operator_at.map_or(home_at, |at| at + 4);
    messages.push(RemoteIdMessage::System(RemoteIdSystem {
        operator_location_type: location_type,
        operator_lat: fields.degrees(lat_at, 90.0),
        operator_lon: fields.degrees(lon_at, 180.0),
        operator_altitude_m: None,
        area_count: 0,
        area_radius_m: 0,
        area_ceiling_m: None,
        area_floor_m: None,
        classification: None,
        timestamp: None,
    }));
    let uuid = fields.text(uuid_at, UUID);
    if !uuid.is_empty() {
        messages.push(RemoteIdMessage::OperatorId {
            id_type: PRIVATE_OPERATOR_ID,
            operator_id: uuid,
        });
    }
    Some(messages)
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{OperatorLocationType, RemoteIdMessage};

    use super::{is_drone_id, messages};
    use crate::remote_id::ble::tests::bytes;

    const ELEMENT: &str = "26371258621310024d06331f455055475430373837475753354949365a66140044b77c005900e400100e480de803b01d68757c1c00000000ec29bc00066cc60159b7ddff489b5cffdc070038343432363336000000000000000000000000";

    #[test]
    fn a_version_two_flight_record_decodes() {
        let data = bytes(ELEMENT);
        assert!(is_drone_id(&data));
        let found = messages(&data).unwrap();
        assert!(matches!(
            &found[0],
            RemoteIdMessage::BasicId { uas_id, .. } if uas_id == "EPUGT0787GWS5II6"
        ));
        let RemoteIdMessage::Location(location) = &found[1] else {
            panic!("{found:?}");
        };
        assert!((location.lon.unwrap() - 7.660_07).abs() < 1e-4);
        assert!((location.lat.unwrap() - 46.830_1).abs() < 1e-4);
        assert_eq!(location.geodetic_altitude_m, Some(89.0));
        assert_eq!(location.height_m, Some(22.8));
        assert_eq!(location.vertical_speed_mps, Some(10.0));
        assert!((location.speed_mps.unwrap() - 49.52).abs() < 0.01);
        let RemoteIdMessage::System(system) = &found[2] else {
            panic!("{found:?}");
        };
        assert_eq!(
            system.operator_location_type,
            OperatorLocationType::LiveGnss
        );
        assert!(matches!(
            &found[3],
            RemoteIdMessage::OperatorId { operator_id, .. } if operator_id == "8442636"
        ));
    }

    #[test]
    fn other_vendor_elements_are_not_drone_id() {
        assert!(!is_drone_id(&bytes("0050f2020101000003a4")));
        assert!(!is_drone_id(&bytes("60601f5862131003")));
        assert!(messages(&bytes("60601f58621310020000")).is_none());
    }
}
