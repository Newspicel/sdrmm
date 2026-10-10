use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::ble::{BLUETOOTH_BAND_RATE_HZ, BLUETOOTH_CHANNEL_WIDTH_HZ, BLUETOOTH_RATE_HZ};

pub const WIDE_RATE_HZ: f64 = BLUETOOTH_BAND_RATE_HZ;
pub const WIFI_CHANNEL_WIDTH_HZ: f64 = 20_000_000.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RemoteIdLink {
    #[default]
    Bluetooth,
    BluetoothBand,
    Wifi,
}

impl RemoteIdLink {
    #[must_use]
    pub fn input_rate_hz(self) -> f64 {
        match self {
            Self::Bluetooth => BLUETOOTH_RATE_HZ,
            Self::BluetoothBand | Self::Wifi => WIDE_RATE_HZ,
        }
    }

    #[must_use]
    pub fn bandwidth_hz(self) -> f64 {
        match self {
            Self::Bluetooth => BLUETOOTH_CHANNEL_WIDTH_HZ,
            Self::BluetoothBand => WIDE_RATE_HZ - BLUETOOTH_CHANNEL_WIDTH_HZ,
            Self::Wifi => WIFI_CHANNEL_WIDTH_HZ,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RemoteIdParams {
    #[serde(default)]
    pub link: RemoteIdLink,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RemoteIdTransport {
    BluetoothLegacy,
    BluetoothExtended,
    WifiBeacon,
    WifiBeaconFrench,
    WifiBeaconDji,
    WifiNan,
}

impl RemoteIdTransport {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::BluetoothLegacy => "BT4",
            Self::BluetoothExtended => "BT5",
            Self::WifiBeacon => "Wi-Fi beacon",
            Self::WifiBeaconFrench => "Wi-Fi FR",
            Self::WifiBeaconDji => "DJI",
            Self::WifiNan => "Wi-Fi NAN",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RemoteIdPhy {
    Le1m,
    LeCodedS8,
    LeCodedS2,
    Dsss1m,
    Dsss2m,
    Cck5m5,
    Cck11m,
    Ofdm,
}

impl RemoteIdPhy {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Le1m => "LE 1M",
            Self::LeCodedS8 => "LE Coded S8",
            Self::LeCodedS2 => "LE Coded S2",
            Self::Dsss1m => "DSSS 1 Mb/s",
            Self::Dsss2m => "DSSS 2 Mb/s",
            Self::Cck5m5 => "CCK 5.5 Mb/s",
            Self::Cck11m => "CCK 11 Mb/s",
            Self::Ofdm => "OFDM",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UasIdType {
    None,
    SerialNumber,
    CaaRegistration,
    UtmAssigned,
    SpecificSession,
    FrenchIdentifier,
    Reserved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UaType {
    None,
    Aeroplane,
    Rotorcraft,
    Gyroplane,
    HybridLift,
    Ornithopter,
    Glider,
    Kite,
    FreeBalloon,
    CaptiveBalloon,
    Airship,
    Parachute,
    Rocket,
    TetheredAircraft,
    GroundObstacle,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UaStatus {
    Undeclared,
    Ground,
    Airborne,
    Emergency,
    SystemFailure,
    Reserved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HeightReference {
    Takeoff,
    Ground,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    None,
    UasIdSignature,
    OperatorIdSignature,
    MessageSetSignature,
    NetworkRemoteId,
    SpecificMethod,
    Reserved,
    Private,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperatorLocationType {
    Takeoff,
    LiveGnss,
    Fixed,
    Reserved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EuCategory {
    Undeclared,
    Open,
    Specific,
    Certified,
    Reserved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct EuClassification {
    pub category: EuCategory,
    pub class: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RemoteIdLocation {
    pub status: UaStatus,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub track_deg: Option<f32>,
    pub speed_mps: Option<f32>,
    pub vertical_speed_mps: Option<f32>,
    pub pressure_altitude_m: Option<f32>,
    pub geodetic_altitude_m: Option<f32>,
    pub height_m: Option<f32>,
    pub height_reference: HeightReference,
    pub horizontal_accuracy_m: Option<f32>,
    pub vertical_accuracy_m: Option<f32>,
    pub pressure_accuracy_m: Option<f32>,
    pub speed_accuracy_mps: Option<f32>,
    pub seconds_after_hour: Option<f32>,
    pub timestamp_accuracy_s: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RemoteIdSystem {
    pub operator_location_type: OperatorLocationType,
    pub operator_lat: Option<f64>,
    pub operator_lon: Option<f64>,
    pub operator_altitude_m: Option<f32>,
    pub area_count: u16,
    pub area_radius_m: u16,
    pub area_ceiling_m: Option<f32>,
    pub area_floor_m: Option<f32>,
    pub classification: Option<EuClassification>,
    pub timestamp: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RemoteIdMessage {
    BasicId {
        id_type: UasIdType,
        ua_type: UaType,
        uas_id: String,
    },
    Location(RemoteIdLocation),
    Authentication {
        auth_type: AuthType,
        page: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_page: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        length: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timestamp: Option<i64>,
        data: String,
    },
    SelfId {
        description_type: u8,
        text: String,
    },
    System(RemoteIdSystem),
    OperatorId {
        id_type: u8,
        operator_id: String,
    },
    Unknown {
        message_type: u8,
        data: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RemoteIdFrame {
    pub transport: RemoteIdTransport,
    pub phy: RemoteIdPhy,
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counter: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uas_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,
    pub level_dbfs: f32,
    pub messages: Vec<RemoteIdMessage>,
    #[serde(default)]
    pub rejected: u32,
}

impl RemoteIdFrame {
    pub fn location(&self) -> Option<&RemoteIdLocation> {
        self.messages.iter().find_map(|message| match message {
            RemoteIdMessage::Location(location) => Some(location),
            _ => None,
        })
    }

    pub fn system(&self) -> Option<&RemoteIdSystem> {
        self.messages.iter().find_map(|message| match message {
            RemoteIdMessage::System(system) => Some(system),
            _ => None,
        })
    }

    #[must_use]
    pub fn position(&self) -> Option<(f64, f64)> {
        let location = self.location()?;
        location.lat.zip(location.lon)
    }

    #[must_use]
    pub fn station(&self) -> String {
        self.uas_id.clone().unwrap_or_else(|| self.address.clone())
    }

    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts = vec![self.station(), self.transport.label().to_owned()];
        if let Some((lat, lon)) = self.position() {
            parts.push(format!("{lat:.5}, {lon:.5}"));
        }
        if let Some(height) = self.location().and_then(|location| location.height_m) {
            parts.push(format!("{height:.0} m"));
        }
        if let Some(system) = self.system()
            && let Some((lat, lon)) = system.operator_lat.zip(system.operator_lon)
        {
            parts.push(format!("pilot {lat:.5}, {lon:.5}"));
        }
        parts.extend(self.messages.iter().find_map(|message| match message {
            RemoteIdMessage::SelfId { text, .. } if !text.is_empty() => Some(text.clone()),
            RemoteIdMessage::OperatorId { operator_id, .. } => Some(operator_id.clone()),
            _ => None,
        }));
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(lat: f64, lon: f64) -> RemoteIdLocation {
        RemoteIdLocation {
            status: UaStatus::Airborne,
            lat: Some(lat),
            lon: Some(lon),
            track_deg: Some(90.0),
            speed_mps: Some(4.5),
            vertical_speed_mps: Some(0.5),
            pressure_altitude_m: None,
            geodetic_altitude_m: Some(510.0),
            height_m: Some(42.0),
            height_reference: HeightReference::Takeoff,
            horizontal_accuracy_m: Some(3.0),
            vertical_accuracy_m: Some(10.0),
            pressure_accuracy_m: None,
            speed_accuracy_mps: Some(1.0),
            seconds_after_hour: Some(1_234.5),
            timestamp_accuracy_s: Some(0.1),
        }
    }

    #[test]
    fn a_frame_round_trips_and_names_the_drone() {
        let frame = RemoteIdFrame {
            transport: RemoteIdTransport::BluetoothLegacy,
            phy: RemoteIdPhy::Le1m,
            address: "D2:11:22:33:44:55".to_owned(),
            channel: Some(37),
            counter: Some(7),
            uas_id: Some("1596F1234567890".to_owned()),
            ssid: None,
            level_dbfs: -41.0,
            rejected: 0,
            messages: vec![
                RemoteIdMessage::Location(location(47.397_6, 8.545_6)),
                RemoteIdMessage::SelfId {
                    description_type: 0,
                    text: "Survey".to_owned(),
                },
            ],
        };
        let json = serde_json::to_value(&frame).unwrap();
        assert_eq!(json["messages"][0]["type"], "location");
        assert_eq!(json["transport"], "bluetooth_legacy");
        let back: RemoteIdFrame = serde_json::from_value(json).unwrap();
        assert_eq!(back, frame);
        assert_eq!(frame.position(), Some((47.397_6, 8.545_6)));
        assert_eq!(
            frame.summary(),
            "1596F1234567890 · BT4 · 47.39760, 8.54560 · 42 m · Survey"
        );
    }

    #[test]
    fn without_an_id_the_address_names_the_drone() {
        let frame = RemoteIdFrame {
            transport: RemoteIdTransport::WifiBeacon,
            phy: RemoteIdPhy::Dsss1m,
            address: "60:60:1F:00:00:01".to_owned(),
            channel: Some(6),
            counter: None,
            uas_id: None,
            ssid: Some("RID-1".to_owned()),
            level_dbfs: -60.0,
            messages: vec![],
            rejected: 2,
        };
        assert_eq!(frame.station(), "60:60:1F:00:00:01");
        assert_eq!(frame.position(), None);
        assert_eq!(frame.summary(), "60:60:1F:00:00:01 · Wi-Fi beacon");
    }

    #[test]
    fn links_pick_their_rates() {
        assert_eq!(RemoteIdLink::Bluetooth.input_rate_hz(), 4e6);
        assert_eq!(RemoteIdLink::BluetoothBand.input_rate_hz(), 20e6);
        assert_eq!(RemoteIdLink::Wifi.bandwidth_hz(), 20e6);
    }
}
