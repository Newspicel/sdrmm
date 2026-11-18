use sdrmm_wire::channel::{Control, SettingField};
use sdrmm_wire::form::{full_width, milliseconds, offset, powers_of_two, within};
use sdrmm_wire::labelled;

use super::{LIMITS, SpatialMethod, SpatialSpectrumParams};

const FULL_CIRCLE_DEG: u32 = 360;

labelled! {
    SpatialMethod {
        Bartlett => "Bartlett": "Robust, wide peaks",
        Capon => "Capon": "Sharper peaks",
        Music => "MUSIC": "Sharpest, needs clean signals",
    }
}

impl SpatialSpectrumParams {
    #[must_use]
    pub fn form() -> Vec<SettingField> {
        let limits = LIMITS;
        let steps: Vec<f64> = (1..=FULL_CIRCLE_DEG)
            .filter(|deg| FULL_CIRCLE_DEG.is_multiple_of(*deg))
            .map(f64::from)
            .filter(|deg| limits.azimuth_step_deg.contains(*deg))
            .collect();
        vec![
            SettingField::new("method", "Method", Control::choice::<SpatialMethod>()),
            SettingField::new("bins", "FFT", powers_of_two(limits.bins)).title("Frequency bins"),
            SettingField::new("columns", "Columns", powers_of_two(limits.columns))
                .title("Frequency columns shown")
                .at_most("bins"),
            milliseconds("average_ms", "Average", limits.average_ms, 10.0),
            milliseconds("report_ms", "Rate", limits.report_ms, 10.0)
                .title("New picture this often"),
            SettingField::new(
                "azimuth_step_deg",
                "Step",
                Control::values(&steps, |deg| format!("{deg}°")),
            )
            .title("Bearing resolution"),
            SettingField::new("span_db", "Range", within(limits.span_db, 1.0).unit("dB"))
                .title("Colour range below the peak"),
            offset(limits.band),
            full_width(limits.band),
        ]
    }
}
