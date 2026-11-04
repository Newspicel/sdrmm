use crate::registry::Native;
use sdrmm_wire::{Control, KindSpec, NumberSpec, SettingField};
mod model;
pub use model::*;
mod acquire;
mod channel;
pub mod dvbs;
pub mod dvbs2;
pub mod dvbt;
pub mod ts;

pub use channel::{DatvChannel, channel_filter, input_rate_hz, occupied_band};

pub(crate) fn datv_decoder() -> Native<DatvParams> {
    Native::new::<DatvChannel>(occupied_band)
        .filter(channel_filter)
        .input_rate(input_rate_hz)
        .audio_channels(|_| 2)
        .form(datv_form)
        .kinds(datv_kinds)
}

pub(crate) fn datv_form() -> Vec<SettingField> {
    vec![
        SettingField::new("standard", "Standard", Control::choice::<DatvStandard>())
            .title("DATV standard"),
        SettingField::new(
            "symbol_rate",
            "Rate",
            NumberSpec::new(MIN_DATV_SYMBOL_RATE, MAX_DATV_SYMBOL_RATE, 1_000.0).unit("Bd"),
        )
        .title("DATV symbol rate"),
        crate::broadcast_media::service_field("program", f64::from(u16::MAX)),
        SettingField::new("roll_off", "Roll-off", Control::choice::<DatvRollOff>())
            .title(
                "Match the transmitter's filter shape; DVB-S2 signals it in the base band header",
            )
            .when("standard", [DatvStandard::DvbS2]),
        SettingField::new("superframes", "Superframes", Control::Toggle)
            .title("Receive DVB-S2X Annex E superframes, formats 0 to 7")
            .when("standard", [DatvStandard::DvbS2]),
        SettingField::new(
            "superframe_reference",
            "Ref code",
            NumberSpec::new(0.0, f64::from(MAX_SUPERFRAME_CODE), 1.0),
        )
        .title("Superframe reference scrambling code n_Ref, 0 by default")
        .when("standard", [DatvStandard::DvbS2])
        .when("superframes", [true]),
        SettingField::new(
            "superframe_payload",
            "Data code",
            NumberSpec::new(0.0, f64::from(MAX_SUPERFRAME_CODE), 1.0),
        )
        .title("Superframe payload scrambling code n_Pay, 0 by default")
        .when("standard", [DatvStandard::DvbS2])
        .when("superframes", [true]),
        SettingField::new("superframe_search", "Code search", Control::Toggle)
            .title("Find unknown scrambling codes from the signal; needs a clean carrier")
            .when("standard", [DatvStandard::DvbS2])
            .when("superframes", [true]),
        SettingField::new(
            "input_stream",
            "Stream",
            NumberSpec::new(0.0, 255.0, 1.0).optional("Auto"),
        )
        .title("Choose an input stream identifier on a multistream carrier")
        .when("standard", [DatvStandard::DvbS2]),
        SettingField::new(
            "code_rate",
            "FEC",
            Control::choice_showing(
                DatvCodeRate::Auto,
                crate::broadcast_media::CODE_RATE_READOUT,
            ),
        )
        .title("DVB-S code rate")
        .quiet(DatvCodeRate::Auto)
        .when("standard", [DatvStandard::DvbS]),
    ]
}

/// The service picker the running decoder fills from its status.
fn datv_kinds() -> Vec<KindSpec> {
    vec![crate::broadcast_media::broadcast_kind()]
}
