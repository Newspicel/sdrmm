use crate::registry::Native;
use sdrmm_wire::{Control, KindSpec, SettingField};
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
mod model;
pub use model::*;
pub(crate) mod aac;
pub(crate) mod audio;
pub(crate) mod bits;
pub(crate) mod cells;
mod channel;
pub(crate) mod coding;
mod estimate;
pub(crate) mod fac;
mod framing;
pub(crate) mod mlc;
pub(crate) mod mode;
pub(crate) mod msc;
mod ofdm;
mod receiver;
pub(crate) mod sdc;
pub(crate) mod text;

use sdrmm_dsp::{FirC, design_lowpass};
use sdrmm_wire::ChannelSettings;

pub use channel::DrmChannel;

use crate::{ChannelError, ChannelFilter};

const INPUT_RATE_HZ: f64 = 192_000.0;
const MIN_DRM30_BANDWIDTH_HZ: f64 = 4_500.0;
const MAX_DRM30_BANDWIDTH_HZ: f64 = 20_000.0;
const DRM_PLUS_BANDWIDTH_HZ: f64 = 100_000.0;
const DRM30_DEFAULT_HZ: f64 = 10_000.0;
const DRM30_BANDWIDTHS_HZ: [f64; 6] = [4_500.0, 5_000.0, 9_000.0, 10_000.0, 18_000.0, 20_000.0];
const FILTER_TAPS: usize = 127;
const FILTER_MARGIN_HZ: f64 = 1_000.0;

fn params(settings: &ChannelSettings) -> Result<DrmParams, ChannelError> {
    checked(
        settings
            .params
            .parse()
            .map_err(ChannelError::InvalidSettings)?,
    )
}

fn checked(p: DrmParams) -> Result<DrmParams, ChannelError> {
    let valid = p.bandwidth_hz.is_finite()
        && match p.mode {
            DrmMode::Drm30 => {
                (MIN_DRM30_BANDWIDTH_HZ..=MAX_DRM30_BANDWIDTH_HZ).contains(&p.bandwidth_hz)
            }
            DrmMode::DrmPlus | DrmMode::Auto => {
                (p.bandwidth_hz - DRM_PLUS_BANDWIDTH_HZ).abs() < 1.0
            }
        }
        && p.service.is_none_or(|service| service < 4);
    if valid {
        Ok(p)
    } else {
        Err(ChannelError::InvalidSettings(format!(
            "DRM bandwidth {} Hz or service {:?} is invalid for {:?}",
            p.bandwidth_hz, p.service, p.mode
        )))
    }
}

pub(crate) fn occupied_band(p: &DrmParams) -> (f64, f64) {
    match p.mode {
        DrmMode::Drm30 if p.bandwidth_hz < 6_000.0 => (0.0, p.bandwidth_hz),
        DrmMode::Drm30 if p.bandwidth_hz < 15_000.0 => {
            (-p.bandwidth_hz / 2.0, p.bandwidth_hz / 2.0)
        }
        DrmMode::Drm30 => (-p.bandwidth_hz / 4.0, 3.0 * p.bandwidth_hz / 4.0),
        DrmMode::Auto | DrmMode::DrmPlus => {
            (-DRM_PLUS_BANDWIDTH_HZ / 2.0, DRM_PLUS_BANDWIDTH_HZ / 2.0)
        }
    }
}

pub(crate) fn channel_filter(p: &DrmParams) -> Result<ChannelFilter, ChannelError> {
    let p = checked(*p)?;
    let (low, high) = occupied_band(&p);
    let low = (low - FILTER_MARGIN_HZ).max(-INPUT_RATE_HZ * 0.45);
    let high = (high + FILTER_MARGIN_HZ).min(INPUT_RATE_HZ * 0.45);
    let half_width = (high - low) / 2.0 / INPUT_RATE_HZ;
    let centre = (high + low) / 2.0 / INPUT_RATE_HZ;
    Ok(ChannelFilter::Sideband(FirC::from_lowpass(
        &design_lowpass(FILTER_TAPS, half_width),
        centre,
    )))
}

const HF: &[HzRange] = &[HzRange::khz(150.0, 30_000.0)];

const DRM_VHF: &[HzRange] = &[HzRange::mhz(47.0, 108.0), HzRange::mhz(174.0, 240.0)];

fn drm_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "DRM30",
            &[Modulation::Ofdm],
            HzRange::new(4_000.0, 21_000.0),
            "OFDM with a 9 to 24 ms useful symbol in a broadcast HF channel",
        )
        .symbol_length(HzRange::new(9_000.0, 24_500.0))
        .on(HF),
        SignalSignature::new(
            "DRM+",
            &[Modulation::Ofdm],
            HzRange::new(80_000.0, 120_000.0),
            "OFDM with a 2.25 ms useful symbol in a 100 kHz channel",
        )
        .symbol_length(HzRange::about(2_250.0, 0.03))
        .on(DRM_VHF),
    ]
}

pub(crate) fn drm_decoder() -> Native<DrmParams> {
    Native::new::<DrmChannel>(occupied_band)
        .filter(channel_filter)
        .signatures(drm_signatures)
        .form(drm_form)
        .kinds(drm_kinds)
        .settle(settle)
}

/// DRM30 takes one of its standard widths; DRM+ and auto take the full 100 kHz.
fn settle(p: &mut DrmParams) {
    let drm30 = p.mode == DrmMode::Drm30;
    if !drm30 {
        p.bandwidth_hz = default_drm_bandwidth_hz();
    } else if !DRM30_BANDWIDTHS_HZ.contains(&p.bandwidth_hz) {
        p.bandwidth_hz = DRM30_DEFAULT_HZ;
    }
}

pub(crate) fn drm_form() -> Vec<SettingField> {
    vec![
        SettingField::new(
            "mode",
            "Mode",
            Control::choice_showing(DrmMode::Auto, crate::broadcast_media::SYSTEM_READOUT),
        )
        .title("DRM mode")
        .quiet(DrmMode::Auto),
        SettingField::bandwidth(&DRM30_BANDWIDTHS_HZ).when("mode", [DrmMode::Drm30]),
        crate::broadcast_media::service_field("service", 3.0),
    ]
}

/// The service picker the running decoder fills from its status.
fn drm_kinds() -> Vec<KindSpec> {
    vec![crate::broadcast_media::broadcast_kind()]
}
