use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::nanovna::NanoVnaPoint;

pub const MAX_NANOVNA_NAME_LEN: usize = 80;
pub const MAX_NANOVNA_NOTE_LEN: usize = 2000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NanoVnaAntenna {
    pub id: i64,
    pub name: String,
    pub notes: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NanoVnaAntennaRequest {
    pub name: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NanoVnaMeasurementInfo {
    pub id: i64,
    pub antenna_id: i64,
    pub label: String,
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrument: Option<String>,
    pub created_at: String,
    pub start_hz: u64,
    pub stop_hz: u64,
    pub point_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_hz: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_vswr: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NanoVnaMeasurement {
    #[serde(flatten)]
    pub info: NanoVnaMeasurementInfo,
    pub points: Vec<NanoVnaPoint>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SaveNanoVnaMeasurementRequest {
    pub label: String,
    #[serde(default)]
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrument: Option<String>,
    pub points: Vec<NanoVnaPoint>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UpdateNanoVnaMeasurementRequest {
    pub label: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NanoVnaLibraryResponse {
    pub antennas: Vec<NanoVnaAntenna>,
    pub measurements: Vec<NanoVnaMeasurementInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::nanovna::NanoVnaComplex;

    #[test]
    fn a_measurement_flattens_its_info_beside_the_points() {
        let measurement = NanoVnaMeasurement {
            info: NanoVnaMeasurementInfo {
                id: 4,
                antenna_id: 2,
                label: "After trim".to_owned(),
                note: String::new(),
                instrument: None,
                created_at: "2026-10-12T10:00:00Z".to_owned(),
                start_hz: 7_000_000,
                stop_hz: 7_200_000,
                point_count: 1,
                best_hz: None,
                best_vswr: None,
            },
            points: vec![NanoVnaPoint {
                frequency_hz: 7_100_000,
                s11: NanoVnaComplex { re: 0.1, im: 0.0 },
                s21: NanoVnaComplex { re: 0.0, im: 0.0 },
            }],
        };
        let json = serde_json::to_value(&measurement).expect("serialize");
        assert_eq!(json["label"], "After trim");
        assert_eq!(json["points"][0]["frequency_hz"], 7_100_000);
        assert!(json.get("instrument").is_none());
        let back: NanoVnaMeasurement = serde_json::from_value(json).expect("deserialize");
        assert_eq!(back, measurement);
    }

    #[test]
    fn omitted_notes_default_to_empty() {
        let request: NanoVnaAntennaRequest =
            serde_json::from_str(r#"{"name":"40m dipole"}"#).expect("deserialize");
        assert_eq!(request.notes, "");
    }
}
