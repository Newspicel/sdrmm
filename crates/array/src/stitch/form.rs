use sdrmm_wire::channel::{Control, SettingField};
use sdrmm_wire::labelled;

use super::{StitchBlend, StitchParams};

labelled! {
    StitchBlend {
        Snr => "SNR": "Favour the cleaner lane in overlaps",
        Equal => "Equal": "Average overlaps",
    }
}

impl StitchParams {
    #[must_use]
    pub fn form() -> Vec<SettingField> {
        vec![
            SettingField::new("blend", "Blend", Control::choice::<StitchBlend>()),
            SettingField::new("noise_equalise", "Equalise", Control::Toggle)
                .title("Match noise floors"),
            SettingField::new("flatten", "Flatten", Control::Toggle)
                .title("Undo each lane's filter droop"),
            SettingField::new("spur_reject", "Spurs", Control::Toggle)
                .title("Drop lane spurs in overlaps"),
            SettingField::new("match_phase", "Phase", Control::Toggle)
                .title("Line up phase across seams"),
        ]
    }
}
