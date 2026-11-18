use sdrmm_wire::channel::{Control, LanePick, ParamOption, SettingField};
use sdrmm_wire::form::{choice, milliseconds, offset, width};

use super::{LIMITS, PolarimeterParams};

impl PolarimeterParams {
    #[must_use]
    pub fn form() -> Vec<SettingField> {
        let limits = LIMITS;
        vec![
            SettingField::new(
                "h_lane",
                "H",
                Control::lanes_except(LanePick::One, "v_lane"),
            )
            .title("Lane of the horizontal antenna"),
            SettingField::new(
                "v_lane",
                "V",
                Control::lanes_except(LanePick::One, "h_lane"),
            )
            .title("Lane of the vertical antenna"),
            SettingField::new(
                "matched",
                "Output",
                choice(vec![
                    ParamOption::new(true, "Matched").title("Beam output matched to the wave"),
                    ParamOption::new(false, "Cross").title("Beam output across the wave"),
                ]),
            )
            .title("What the beam output carries"),
            offset(limits.band),
            width(limits.band, "Band around the offset"),
            milliseconds("report_ms", "Report", limits.report_ms, 10.0),
            milliseconds("average_ms", "Average", limits.average_ms, 10.0),
            milliseconds("crossfade_ms", "Fade", limits.crossfade_ms, 1.0)
                .title("Blend old and new weights"),
            SettingField::new("flip_hand", "Flip", Control::Toggle)
                .title("Swap right and left hand"),
        ]
    }
}
