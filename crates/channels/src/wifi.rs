use sdrmm_decoder::registry::Native;
use sdrmm_wire::{KindSpec, SettingField, airtime_form};
mod model;
pub use model::*;
mod occupancy;

pub use self::occupancy::WifiOccupancyChannel;
pub(crate) use self::occupancy::{channel_filter, input_rate, occupied_band};

pub(crate) fn wifi_occupancy_decoder() -> Native<WifiOccupancyParams> {
    Native::new::<WifiOccupancyChannel>(|p| occupied_band(*p))
        .filter(|_| Ok(channel_filter()))
        .input_rate(|p| input_rate(*p))
        .home(2_437_000_000.0)
        .form(wifi_occupancy_form)
        .kinds(wifi_occupancy_kinds)
}

pub(crate) fn wifi_occupancy_form() -> Vec<SettingField> {
    airtime_form(DEFAULT_WIFI_MARGIN_DB)
}

fn wifi_occupancy_kinds() -> Vec<KindSpec> {
    vec![wifi_occupancy_kind()]
}

#[cfg(test)]
pub(crate) mod fixture;
