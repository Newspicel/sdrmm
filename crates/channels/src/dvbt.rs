mod model;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
pub use model::*;
use sdrmm_decoder::registry::{LiveProgram, Native};
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

pub use channel::{DvbtChannel, channel_filter, occupied_band};

fn program_switch(before: &DvbtParams, after: &DvbtParams) -> Option<LiveProgram> {
    let comparable = DvbtParams {
        program: after.program,
        ..*before
    };
    (before.program != after.program && comparable == *after).then_some(LiveProgram {
        program: after.program,
    })
}

pub(crate) fn dvbt_decoder() -> Native<DvbtParams> {
    let decoder = Native::new::<DvbtChannel>(occupied_band)
        .program_switch(program_switch)
        .filter(|p| Ok(channel_filter(p)))
        .input_rate(|p| p.sample_rate_hz())
        .audio_channels(|_| 2)
        .form(dvbt_form)
        .kinds(dvbt_kinds);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(synth::SIGNALS),
        _ => decoder,
    }
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
        sdrmm_decoder::broadcast::service_field("program", f64::from(u16::MAX)),
    ]
}

fn dvbt_kinds() -> Vec<KindSpec> {
    vec![sdrmm_decoder::broadcast::broadcast_kind()]
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
