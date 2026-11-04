use crate::datv::{DvbtBandwidth, DvbtParams, DvbtStandard};
use crate::registry::Native;
use sdrmm_wire::{Control, KindSpec, NumberSpec, SettingField};
mod acquire;
mod channel;
mod en300744;
mod estimate;
mod frontend;
pub mod mapping;
pub mod receiver;
pub mod t2;
pub mod tps;
mod wiener;

pub use channel::{DvbtChannel, channel_filter, occupied_band};

pub(crate) fn dvbt_decoder() -> Native<DvbtParams> {
    Native::new::<DvbtChannel>(occupied_band)
        .filter(|p| Ok(channel_filter(p)))
        .input_rate(|p| p.sample_rate_hz())
        .audio_channels(|_| 2)
        .form(dvbt_form)
        .kinds(dvbt_kinds)
}

pub(crate) fn dvbt_form() -> Vec<SettingField> {
    vec![
        SettingField::new("standard", "Standard", Control::choice::<DvbtStandard>())
            .title("Terrestrial standard"),
        SettingField::new("bandwidth", "BW", Control::choice::<DvbtBandwidth>())
            .title("DVB-T bandwidth"),
        SettingField::new(
            "plp",
            "PLP",
            NumberSpec::new(0.0, 255.0, 1.0).optional("auto"),
        )
        .title("PLP ID, empty for automatic selection")
        .when("standard", [DvbtStandard::DvbT2]),
        SettingField::new("low_priority", "Low priority", Control::Toggle)
            .title("Decode the low priority transport stream of a hierarchical DVB-T multiplex")
            .when("standard", [DvbtStandard::DvbT]),
        crate::broadcast_media::service_field("program", f64::from(u16::MAX)),
    ]
}

fn dvbt_kinds() -> Vec<KindSpec> {
    vec![crate::broadcast_media::broadcast_kind()]
}

#[cfg(test)]
mod tests;
