use crate::registry::Native;
use sdrmm_wire::{Control, KindSpec, SettingField};
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
mod model;
pub use model::*;
mod channel;
pub mod fic;
pub mod fig;
pub mod mode;
pub mod msc;
pub mod ofdm;
pub(crate) mod pacer;
pub(crate) mod packet;
pub(crate) mod pad;
pub mod protection;
pub(crate) mod prs;
pub mod superframe;
pub(crate) mod sync;

pub use channel::{DabChannel, channel_filter, occupied_band};

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
    Native::new::<DabChannel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .audio_channels(|_| 2)
        .home(227_360_000.0)
        .signatures(dab_signatures)
        .form(dab_form)
        .kinds(dab_kinds)
}

pub(crate) fn dab_form() -> Vec<SettingField> {
    vec![
        SettingField::new("mode", "Type", Control::choice::<DabMode>())
            .title("DAB generation")
            .quiet(DabMode::Auto),
        SettingField::new(
            "transmission_mode",
            "Mode",
            Control::choice::<DabTransmissionMode>(),
        )
        .title("DAB transmission mode")
        .quiet(DabTransmissionMode::Auto),
        crate::broadcast_media::service_field("service_id", f64::from(u32::MAX)),
    ]
}

/// The service picker the running decoder fills from its status.
fn dab_kinds() -> Vec<KindSpec> {
    vec![crate::broadcast_media::broadcast_kind()]
}
