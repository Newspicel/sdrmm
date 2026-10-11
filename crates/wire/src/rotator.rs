use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    MAX_NMEA_BAUD, MAX_POSITION_ENDPOINT_LEN, MIN_NMEA_BAUD, patch::valid_host_port,
    satellite::SkyPoint,
};

pub const DEFAULT_ROTCTLD_ADDRESS: &str = "127.0.0.1:4533";
pub const DEFAULT_GS232_BAUD: u32 = 9_600;
pub const DEFAULT_ROTATOR_TOLERANCE_DEG: f64 = 2.0;
pub const MIN_ROTATOR_TOLERANCE_DEG: f64 = 0.5;
pub const MAX_ROTATOR_TOLERANCE_DEG: f64 = 20.0;
pub const MIN_ROTATOR_AZIMUTH_DEG: f64 = 360.0;
pub const MAX_ROTATOR_AZIMUTH_DEG: f64 = 540.0;
pub const LEVEL_ELEVATION_DEG: f64 = 90.0;
pub const FLIP_ELEVATION_DEG: f64 = 180.0;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RotatorLink {
    Gs232 { device: String, baud: u32 },
    Rotctld { address: String },
}

impl RotatorLink {
    #[must_use]
    pub fn valid(&self) -> bool {
        match self {
            Self::Gs232 { device, baud } => {
                !device.is_empty()
                    && device.len() <= MAX_POSITION_ENDPOINT_LEN
                    && (MIN_NMEA_BAUD..=MAX_NMEA_BAUD).contains(baud)
            }
            Self::Rotctld { address } => {
                address.len() <= MAX_POSITION_ENDPOINT_LEN && valid_host_port(address)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RotatorAxes {
    #[default]
    AzimuthElevation,
    Azimuth,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RotatorMode {
    #[default]
    Track,
    Manual,
    Park,
    Stop,
}

const fn default_tolerance() -> f64 {
    DEFAULT_ROTATOR_TOLERANCE_DEG
}

const fn default_azimuth_max() -> f64 {
    MIN_ROTATOR_AZIMUTH_DEG
}

const HOME: SkyPoint = SkyPoint {
    azimuth_deg: 0.0,
    elevation_deg: 0.0,
};

const fn home() -> SkyPoint {
    HOME
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RotatorNode {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<RotatorLink>,
    #[serde(default = "default_tolerance")]
    pub tolerance_deg: f64,
    #[serde(default)]
    pub axes: RotatorAxes,
    #[serde(default = "default_azimuth_max")]
    pub azimuth_max_deg: f64,
    #[serde(default)]
    pub flip: bool,
    #[serde(default)]
    pub mode: RotatorMode,
    #[serde(default = "home")]
    pub manual: SkyPoint,
    #[serde(default = "home")]
    pub park: SkyPoint,
}

impl Default for RotatorNode {
    fn default() -> Self {
        Self {
            link: None,
            tolerance_deg: DEFAULT_ROTATOR_TOLERANCE_DEG,
            axes: RotatorAxes::default(),
            azimuth_max_deg: MIN_ROTATOR_AZIMUTH_DEG,
            flip: false,
            mode: RotatorMode::default(),
            manual: HOME,
            park: HOME,
        }
    }
}

impl RotatorNode {
    #[must_use]
    pub fn valid(&self) -> bool {
        (MIN_ROTATOR_TOLERANCE_DEG..=MAX_ROTATOR_TOLERANCE_DEG).contains(&self.tolerance_deg)
            && (MIN_ROTATOR_AZIMUTH_DEG..=MAX_ROTATOR_AZIMUTH_DEG).contains(&self.azimuth_max_deg)
            && self.reachable(self.manual)
            && self.reachable(self.park)
            && self.link.as_ref().is_none_or(RotatorLink::valid)
    }

    #[must_use]
    pub fn flips(&self) -> bool {
        self.flip && self.axes == RotatorAxes::AzimuthElevation
    }

    #[must_use]
    pub fn elevation_max_deg(&self) -> f64 {
        match (self.axes, self.flips()) {
            (RotatorAxes::Azimuth, _) => 0.0,
            (_, true) => FLIP_ELEVATION_DEG,
            (_, false) => LEVEL_ELEVATION_DEG,
        }
    }

    #[must_use]
    pub fn reachable(&self, point: SkyPoint) -> bool {
        (0.0..=self.azimuth_max_deg).contains(&point.azimuth_deg)
            && (0.0..=FLIP_ELEVATION_DEG).contains(&point.elevation_deg)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RotatorStatus {
    pub node: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<SkyPoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<SkyPoint>,
    #[serde(default)]
    pub awaiting_pass: bool,
    #[serde(default)]
    pub flipped: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_node_is_valid_and_waits_for_a_link() {
        assert!(RotatorNode::default().valid());
    }

    #[test]
    fn nonsense_settings_are_refused() {
        let unreachable = SkyPoint {
            azimuth_deg: 400.0,
            elevation_deg: 0.0,
        };
        let bad = [
            RotatorNode {
                tolerance_deg: 0.0,
                ..RotatorNode::default()
            },
            RotatorNode {
                tolerance_deg: f64::NAN,
                ..RotatorNode::default()
            },
            RotatorNode {
                azimuth_max_deg: 720.0,
                ..RotatorNode::default()
            },
            RotatorNode {
                manual: unreachable,
                ..RotatorNode::default()
            },
            RotatorNode {
                park: unreachable,
                ..RotatorNode::default()
            },
            RotatorNode {
                link: Some(RotatorLink::Gs232 {
                    device: String::new(),
                    baud: DEFAULT_GS232_BAUD,
                }),
                ..RotatorNode::default()
            },
            RotatorNode {
                link: Some(RotatorLink::Gs232 {
                    device: "/dev/ttyUSB0".to_owned(),
                    baud: 1,
                }),
                ..RotatorNode::default()
            },
            RotatorNode {
                link: Some(RotatorLink::Rotctld {
                    address: "localhost".to_owned(),
                }),
                ..RotatorNode::default()
            },
        ];
        for node in bad {
            assert!(!node.valid(), "{node:?}");
        }
    }

    #[test]
    fn an_overlap_rotator_reaches_past_north() {
        let node = RotatorNode {
            azimuth_max_deg: 450.0,
            manual: SkyPoint {
                azimuth_deg: 400.0,
                elevation_deg: 0.0,
            },
            ..RotatorNode::default()
        };
        assert!(node.valid());
    }

    #[test]
    fn both_links_take_their_usual_settings() {
        for link in [
            RotatorLink::Gs232 {
                device: "/dev/ttyUSB0".to_owned(),
                baud: DEFAULT_GS232_BAUD,
            },
            RotatorLink::Rotctld {
                address: DEFAULT_ROTCTLD_ADDRESS.to_owned(),
            },
        ] {
            assert!(link.valid(), "{link:?}");
        }
    }

    #[test]
    fn only_an_elevation_axis_can_flip() {
        let flipping = RotatorNode {
            flip: true,
            ..RotatorNode::default()
        };
        assert!(flipping.flips());
        assert!((flipping.elevation_max_deg() - FLIP_ELEVATION_DEG).abs() < f64::EPSILON);
        let turntable = RotatorNode {
            axes: RotatorAxes::Azimuth,
            ..flipping
        };
        assert!(!turntable.flips());
        assert!(turntable.elevation_max_deg().abs() < f64::EPSILON);
    }
}
