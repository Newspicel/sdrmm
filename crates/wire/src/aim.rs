use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    channel::{Control, SettingField},
    present::{Field, Unit, fields, number, text_of},
    satellite::SatelliteLook,
};

pub const MAX_AIM_ENTITY_LEN: usize = 64;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct AimParams {
    /// The entity to follow, such as a sonde serial or a callsign. Empty follows the newest.
    pub entity: String,
}

impl AimParams {
    #[must_use]
    pub fn valid(&self) -> bool {
        self.entity.len() <= MAX_AIM_ENTITY_LEN
    }

    #[must_use]
    pub fn follows(&self, entity: Option<&str>) -> bool {
        let wanted = self.entity.trim();
        wanted.is_empty() || entity.is_some_and(|entity| entity.eq_ignore_ascii_case(wanted))
    }

    #[must_use]
    pub fn form() -> Vec<SettingField> {
        vec![
            SettingField::new(
                "entity",
                "Follow",
                Control::Text {
                    placeholder: Some("newest".to_owned()),
                    optional: true,
                    secret: false,
                    max_len: Some(MAX_AIM_ENTITY_LEN),
                },
            )
            .title("Serial or callsign to follow, empty for the newest"),
        ]
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AimNode {
    #[serde(default)]
    pub settings: AimParams,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AimStatus {
    pub node: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look: Option<SatelliteLook>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub altitude_m: Option<f64>,
    /// Seconds since the followed position was decoded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub driving: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// What the node waits for, never a fault: a wire, a setting or data still to come.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readouts: Vec<Field>,
}

impl AimStatus {
    #[must_use]
    pub fn with_readouts(mut self) -> Self {
        let look = self.look;
        self.readouts = fields(&[
            (
                "entity",
                "Following",
                self.entity.as_deref().and_then(text_of),
            ),
            (
                "azimuth",
                "Az",
                number(look.map(|look| look.azimuth_deg), Unit::Degrees, 0),
            ),
            (
                "elevation",
                "El",
                number(look.map(|look| look.elevation_deg), Unit::Degrees, 1),
            ),
            (
                "range",
                "Range",
                number(look.map(|look| look.range_km), Unit::Kilometres, 1),
            ),
            ("altitude", "Alt", number(self.altitude_m, Unit::Metres, 0)),
            ("age", "Age", number(self.age_s, Unit::Seconds, 0)),
        ]);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_entity_follows_anything() {
        let params = AimParams::default();
        assert!(params.follows(None));
        assert!(params.follows(Some("S1234567")));
    }

    #[test]
    fn a_named_entity_follows_only_itself() {
        let params = AimParams {
            entity: " s1234567 ".to_owned(),
        };
        assert!(params.follows(Some("S1234567")));
        assert!(!params.follows(Some("T7654321")));
        assert!(!params.follows(None));
    }

    #[test]
    fn an_overlong_entity_is_refused() {
        let params = AimParams {
            entity: "x".repeat(MAX_AIM_ENTITY_LEN + 1),
        };
        assert!(!params.valid());
    }

    #[test]
    fn readouts_show_where_it_points() {
        let status = AimStatus {
            entity: Some("S1".to_owned()),
            look: Some(SatelliteLook {
                azimuth_deg: 90.0,
                elevation_deg: 12.5,
                range_km: 40.0,
                range_rate_km_s: 0.0,
            }),
            ..AimStatus::default()
        }
        .with_readouts();
        let keys: Vec<&str> = status.readouts.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, ["entity", "azimuth", "elevation", "range"]);
    }
}
