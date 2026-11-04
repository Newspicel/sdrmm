use sdrmm_wire::DecoderSettings;
use serde::{Deserialize, Serialize};

pub const MIN_IDENT_BANDWIDTH_HZ: f64 = 1_000.0;

pub const MAX_IDENT_BANDWIDTH_HZ: f64 = 192_000.0;

pub const MIN_IDENT_INTERVAL_MS: u32 = 250;

pub const MAX_IDENT_INTERVAL_MS: u32 = 10_000;

pub const MIN_IDENT_THRESHOLD_DB: f32 = 3.0;

pub const MAX_IDENT_THRESHOLD_DB: f32 = 40.0;

fn default_ident_bandwidth_hz() -> f64 {
    MAX_IDENT_BANDWIDTH_HZ
}

fn default_ident_interval_ms() -> u32 {
    1_000
}

fn default_ident_threshold_db() -> f32 {
    8.0
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentParams {
    #[serde(default = "default_ident_bandwidth_hz")]
    pub bandwidth_hz: f64,
    #[serde(default = "default_ident_interval_ms")]
    pub interval_ms: u32,
    #[serde(default = "default_ident_threshold_db")]
    pub threshold_db: f32,
}

impl Default for IdentParams {
    fn default() -> Self {
        Self {
            bandwidth_hz: default_ident_bandwidth_hz(),
            interval_ms: default_ident_interval_ms(),
            threshold_db: default_ident_threshold_db(),
        }
    }
}

impl DecoderSettings for IdentParams {
    const TYPE_ID: &'static str = "ident";
}
