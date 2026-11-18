use sdrmm_wire::channel::{Control, SettingField};
use sdrmm_wire::form::{full_width, offset, powers_of_two, within};

use super::{CorrelatorParams, LIMITS};

impl CorrelatorParams {
    #[must_use]
    pub fn form() -> Vec<SettingField> {
        let limits = LIMITS;
        vec![
            SettingField::new("bins", "FFT", powers_of_two(limits.bins)).title("Frequency bins"),
            SettingField::new("channels", "Channels", powers_of_two(limits.channels))
                .title("Frequency points sent per baseline")
                .at_most("bins"),
            SettingField::new(
                "integrate_s",
                "Integrate",
                within(limits.integrate_s, 0.05).unit("s"),
            )
            .title("Averaging time per result"),
            SettingField::new("overlap", "Overlap", Control::Toggle)
                .title("Half-overlapping FFTs, twice the work"),
            offset(limits.band),
            full_width(limits.band),
        ]
    }
}
