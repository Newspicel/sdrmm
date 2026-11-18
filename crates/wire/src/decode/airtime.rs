//! What a spectrum airtime survey is set with, whatever band it watches.

use serde::{Deserialize, Serialize};

use crate::{Control, NumberSpec, SettingField, labelled};

pub const AIRTIME_REPORT_MS: u32 = 1_000;

pub const MIN_AIRTIME_MARGIN_DB: f32 = 3.0;

pub const MAX_AIRTIME_MARGIN_DB: f32 = 30.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AirtimeSpan {
    #[default]
    Mhz20,
    Mhz40,
    Mhz80,
}

impl AirtimeSpan {
    #[must_use]
    pub fn sample_rate_hz(self) -> f64 {
        match self {
            Self::Mhz20 => 20_000_000.0,
            Self::Mhz40 => 40_000_000.0,
            Self::Mhz80 => 80_000_000.0,
        }
    }
}

labelled! {
    AirtimeSpan {
        Mhz20 => "20": "20 MS/s",
        Mhz40 => "40": "40 MS/s",
        Mhz80 => "80": "80 MS/s, all of 2.4 GHz",
    }
}

/// Refuses a busy margin outside what a survey offers.
pub fn check_airtime_margin(margin_db: f32) -> Result<(), String> {
    if margin_db.is_finite() && (MIN_AIRTIME_MARGIN_DB..=MAX_AIRTIME_MARGIN_DB).contains(&margin_db)
    {
        Ok(())
    } else {
        Err(format!(
            "margin must be {MIN_AIRTIME_MARGIN_DB}..={MAX_AIRTIME_MARGIN_DB} dB, got {margin_db}"
        ))
    }
}

/// The span and margin every airtime survey offers; only the default margin differs.
#[must_use]
pub fn airtime_form(default_margin_db: f32) -> Vec<SettingField> {
    vec![
        SettingField::new("span", "Span", Control::choice::<AirtimeSpan>())
            .title("Width of the window measured, in MHz")
            .fallback(AirtimeSpan::Mhz20),
        SettingField::new(
            "margin_db",
            "Margin",
            NumberSpec::new(
                f64::from(MIN_AIRTIME_MARGIN_DB),
                f64::from(MAX_AIRTIME_MARGIN_DB),
                0.5,
            )
            .unit("dB"),
        )
        .title("How far over the noise floor counts as busy")
        .fallback(default_margin_db),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_margin_outside_the_offer_is_refused() {
        assert!(check_airtime_margin(6.0).is_ok());
        assert!(check_airtime_margin(MAX_AIRTIME_MARGIN_DB + 1.0).is_err());
        assert!(check_airtime_margin(f32::NAN).is_err());
    }
}
