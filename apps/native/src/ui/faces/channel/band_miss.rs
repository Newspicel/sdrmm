use sdrmm_wire::channel::BandMiss;

use crate::ui::kit_channel::format_hz;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Said {
    pub label: String,
    pub title: String,
}

#[must_use]
pub fn rounded_up(hz: f64) -> f64 {
    if !hz.is_finite() || hz <= 0.0 {
        return hz;
    }
    let scale = 10f64.powi(hz.log10().floor() as i32 - 2);
    (hz / scale).ceil() * scale
}

fn too_wide_title(needs_hz: f64, top_rate_hz: Option<f64>) -> String {
    match top_rate_hz {
        Some(top) if top < needs_hz => format!(
            "Wider than this radio reaches. Its sample rate tops out at {}",
            format_hz(top)
        ),
        _ => format!(
            "Wider than the radio's window. Raise its sample rate to {} or more",
            format_hz(rounded_up(needs_hz))
        ),
    }
}

#[must_use]
pub fn band_miss_said(miss: BandMiss) -> Said {
    let (label, title) = match miss {
        BandMiss::TooWide {
            needs_hz,
            top_rate_hz,
        } => (
            format!("needs {}", format_hz(rounded_up(needs_hz))),
            too_wide_title(needs_hz, top_rate_hz),
        ),
        BandMiss::OffTuner => (
            String::from("off tuner"),
            String::from("Outside the radio's tuning range"),
        ),
        BandMiss::Crowded => (
            String::from("crowded out"),
            String::from(
                "Auto keeps the radio over the other decoders. Raise the sample rate or move this one to another radio",
            ),
        ),
        BandMiss::TunedAway => (
            String::from("out of band"),
            String::from("The radio is tuned elsewhere"),
        ),
    };
    Said { label, title }
}

#[must_use]
pub fn band_miss_of(reported: Option<Option<BandMiss>>, reaches: bool) -> Option<BandMiss> {
    match reported {
        Some(reported) => reported,
        None => (!reaches).then_some(BandMiss::TunedAway),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_needed_rate_rounds_up_to_three_figures() {
        assert_eq!(rounded_up(2_412_345.0), 2_420_000.0);
        assert_eq!(rounded_up(1_000_000.0), 1_000_000.0);
        assert_eq!(rounded_up(0.0), 0.0);
    }

    #[test]
    fn too_wide_says_what_rate_to_set_or_that_the_radio_cannot() {
        let raise = band_miss_said(BandMiss::TooWide {
            needs_hz: 2_412_345.0,
            top_rate_hz: Some(3_200_000.0),
        });
        assert_eq!(raise.label, "needs 2.42 MHz");
        assert_eq!(
            raise.title,
            "Wider than the radio's window. Raise its sample rate to 2.42 MHz or more"
        );
        let beyond = band_miss_said(BandMiss::TooWide {
            needs_hz: 8_000_000.0,
            top_rate_hz: Some(3_200_000.0),
        });
        assert_eq!(
            beyond.title,
            "Wider than this radio reaches. Its sample rate tops out at 3.2 MHz"
        );
    }

    #[test]
    fn every_reason_has_a_short_label() {
        assert_eq!(band_miss_said(BandMiss::OffTuner).label, "off tuner");
        assert_eq!(band_miss_said(BandMiss::Crowded).label, "crowded out");
        assert_eq!(band_miss_said(BandMiss::TunedAway).label, "out of band");
    }

    #[test]
    fn a_reported_miss_wins_over_the_reach_guess() {
        assert_eq!(band_miss_of(Some(None), false), None);
        assert_eq!(
            band_miss_of(Some(Some(BandMiss::Crowded)), true),
            Some(BandMiss::Crowded)
        );
        assert_eq!(band_miss_of(None, false), Some(BandMiss::TunedAway));
        assert_eq!(band_miss_of(None, true), None);
    }
}
