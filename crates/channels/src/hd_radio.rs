//! HD Radio (NRSC-5) hybrid FM: the primary main sidebands' OFDM, the PIDS station
//! information and the P1 audio transport with its HDC packets.

use sdrmm_decoder::registry::Native;
use sdrmm_wire::{FilterSpec, HzRange, KindSpec, Modulation, SettingField, SignalSignature};

mod acquire;
mod channel;
mod decode;
mod frame;
mod id3;
mod layout;
mod model;
mod playout;
mod receiver;
mod sis;
mod sync;

pub use channel::HdRadioChannel;
pub use model::HdRadioParams;

/// The outermost primary subcarrier, 546 spacings from the centre.
const OCCUPIED_HZ: f64 = 546.0 * layout::SPACING_HZ;
const FILTER_TAPS: usize = 127;
const FILTER_EDGE_HZ: f64 = 210_000.0;
const FM_BAND: &[HzRange] = &[HzRange::mhz(87.5, 108.0)];

const fn occupied_band() -> (f64, f64) {
    (-OCCUPIED_HZ, OCCUPIED_HZ)
}

fn hd_radio_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "HD Radio",
            &[Modulation::Ofdm],
            HzRange::new(130_000.0, 420_000.0),
            "OFDM sidebands either side of an FM station, 2.75 ms useful symbol",
        )
        .symbol_length(HzRange::about(1.0e6 / layout::SPACING_HZ, 0.03))
        .on(FM_BAND),
    ]
}

pub(crate) fn hd_radio_decoder() -> Native<HdRadioParams> {
    Native::new::<HdRadioChannel>(|_| occupied_band())
        .filter(|_| Ok(FilterSpec::lowpass(FILTER_TAPS, FILTER_EDGE_HZ)))
        .audio_channels(|_| 2)
        .home(98_100_000.0)
        .signatures(hd_radio_signatures)
        .form(hd_radio_form)
        .kinds(hd_radio_kinds)
}

fn hd_radio_form() -> Vec<SettingField> {
    vec![sdrmm_decoder::broadcast::service_field("service", 7.0)]
}

fn hd_radio_kinds() -> Vec<KindSpec> {
    vec![sdrmm_decoder::broadcast::broadcast_kind()]
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
