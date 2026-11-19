use sdrmm_decoder::default_true;
use serde::{Deserialize, Serialize};

pub use sdrmm_decoder::aircraft::AdsbMessage;
pub(crate) use sdrmm_decoder::aircraft::adsb_kind;

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

impl sdrmm_wire::DecoderSettings for AdsbParams {
    const TYPE_ID: &'static str = "adsb";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::{DecoderEvent, EventContract};

    #[test]
    fn the_aircraft_table_names_fields_a_message_shows() {
        let sample = DecoderEvent::tagged(
            AdsbMessage::KIND,
            &serde_json::json!({
                "icao": "3c6444", "df": 17, "raw": "8D", "callsign": "DLH123",
                "altitude_ft": 37000, "ground_speed_kt": 451.4, "track_deg": 271.6,
                "vertical_rate_fpm": -1088, "squawk": "1000", "lat": 52.52, "lon": 13.405,
            }),
        );
        sdrmm_decoder_test_support::assert_views_hold(&adsb_kind(), &[sample]);
    }
}
