use sdrmm_decoder::broadcast::{SYSTEM_READOUT, TRANSMISSION_MODE_READOUT};
use sdrmm_decoder::registry::Native;
use sdrmm_wire::{Control, KindSpec, SettingField};
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
mod model;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
pub use model::*;
mod channel;
mod data;
pub mod fic;
pub mod fig;
pub mod msc;
pub(crate) mod packet;
pub(crate) mod pad;
mod plus;
pub mod protection;
pub(crate) mod prs;
pub mod superframe;
pub(crate) mod sync;

pub use channel::{DabChannel, channel_filter, occupied_band};

pub(crate) const TRANSMISSION_MODES: [DabTransmissionMode; 4] = [
    DabTransmissionMode::I,
    DabTransmissionMode::Ii,
    DabTransmissionMode::Iii,
    DabTransmissionMode::Iv,
];

/// Energy dispersal: x^9 + x^5 + 1 from all ones.
pub(crate) const DISPERSAL: sdrmm_dsp::PrbsSpec = sdrmm_dsp::PrbsSpec {
    stages: 9,
    taps: 0x110,
    seed: 0x1FF,
};
const DAB_BANDS: &[HzRange] = &[HzRange::mhz(174.0, 240.0), HzRange::mhz(1_452.0, 1_492.0)];

fn dab_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "DAB / DAB+",
            &[Modulation::Ofdm],
            HzRange::new(100_000.0, 2_000_000.0),
            "OFDM with a 1 ms useful symbol, the DAB transmission mode I",
        )
        .symbol_length(HzRange::about(1_000.0, 0.03))
        .on(DAB_BANDS),
    ]
}

pub(crate) fn dab_decoder() -> Native<DabParams> {
    let decoder = Native::new::<DabChannel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .audio_channels(|_| 2)
        .home(227_360_000.0)
        .signatures(dab_signatures)
        .form(dab_form)
        .kinds(dab_kinds);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(synth::SIGNALS),
        _ => decoder,
    }
}

pub(crate) fn dab_form() -> Vec<SettingField> {
    vec![
        SettingField::new(
            "mode",
            "Type",
            Control::choice_showing(DabMode::Auto, SYSTEM_READOUT),
        )
        .title("DAB generation")
        .quiet(DabMode::Auto),
        SettingField::new(
            "transmission_mode",
            "Mode",
            Control::choice_showing(DabTransmissionMode::Auto, TRANSMISSION_MODE_READOUT),
        )
        .title("DAB transmission mode")
        .quiet(DabTransmissionMode::Auto),
        sdrmm_decoder::broadcast::service_field("service_id", f64::from(u32::MAX)),
    ]
}

/// The service picker the running decoder fills from its status.
fn dab_kinds() -> Vec<KindSpec> {
    vec![sdrmm_decoder::broadcast::broadcast_kind()]
}

#[cfg(test)]
pub(crate) mod fixture;
