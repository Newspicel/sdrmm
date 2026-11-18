//! What a paging message carries, whichever paging protocol sent it.

use serde::{Deserialize, Serialize};

#[must_use]
pub const fn default_pager_bandwidth_hz() -> f64 {
    12_500.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PagerPayload {
    Tone,
    Numeric,
    Alpha,
    Binary,
}

impl PagerPayload {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Tone => "tone",
            Self::Numeric => "numeric",
            Self::Alpha => "alpha",
            Self::Binary => "binary",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_payload_reads_as_its_label() {
        for payload in [
            PagerPayload::Tone,
            PagerPayload::Numeric,
            PagerPayload::Alpha,
            PagerPayload::Binary,
        ] {
            let json = serde_json::to_value(payload).expect("payload serializes");
            assert_eq!(json, payload.label());
        }
    }
}
