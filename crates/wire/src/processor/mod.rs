use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub mod beamformer;
pub mod correlator;
pub mod df;
pub mod polarimeter;
pub mod spatial;
pub mod stitch;

use crate::limits::{BandLimits, Bounds};

pub const MAX_AT_LEN: usize = 40;
pub const MIN_BAND_HZ: f64 = 1_000.0;
pub const DEFAULT_BAND_HZ: f64 = 200_000.0;

pub(crate) fn reserved_at() -> String {
    String::with_capacity(MAX_AT_LEN)
}

pub(crate) fn power_of_two_in(value: u32, bounds: Bounds<u32>) -> bool {
    value.is_power_of_two() && bounds.contains(value)
}

pub(crate) fn band_holds(band: BandLimits, bandwidth_hz: Option<f64>) -> bool {
    bandwidth_hz.is_none_or(|hz| band.bandwidth_hz.contains(hz))
}

macro_rules! processors {
    ($callback:ident) => {
        $callback! {
            processors: [
                (
                    Df,
                    "df",
                    $crate::processor::df::DfParams,
                    $crate::processor::df::DfReading,
                    DfNode,
                    "Direction finder",
                    "Bearings from an array",
                    $crate::patch::Face::fit(400),
                    [
                        array_in,
                        events_out("True bearings", Df),
                        bearing_out("Live bearing to steer a beam")
                    ]
                ),
                (
                    Beamformer,
                    "beamformer",
                    $crate::processor::beamformer::BeamformerParams,
                    $crate::processor::beamformer::BeamformerReading,
                    BeamformerNode,
                    "Beamformer",
                    "Steer, null or combine an array",
                    $crate::patch::Face::fit(400),
                    [
                        array_in,
                        steer_in("A direction finder to steer at"),
                        beam_out("The combined lane")
                    ]
                ),
                (
                    PassiveRadar,
                    "passive_radar",
                    $crate::radar::PassiveRadarParams,
                    $crate::radar::RadarUpdate,
                    PassiveRadarNode,
                    "Passive radar",
                    "Aircraft echoes of FM, DAB or DVB-T",
                    $crate::patch::Face::sized(560, 540).min(420, 380),
                    [
                        array_in,
                        tx_in("GPS node fixed at the transmitter"),
                        adsb_in("ADS-B decoder used as truth"),
                        events_out("Track events", Radar, RadarSites)
                    ]
                ),
                (
                    Stitch,
                    "stitch",
                    $crate::processor::stitch::StitchParams,
                    $crate::processor::stitch::StitchReading,
                    StitchNode,
                    "Stitch",
                    "Spread lanes joined into one wide band",
                    $crate::patch::Face::fit(360),
                    [array_in, wide_out("Every lane joined")]
                ),
                (
                    SpatialSpectrum,
                    "spatial_spectrum",
                    $crate::processor::spatial::SpatialSpectrumParams,
                    $crate::processor::spatial::SpatialReading,
                    SpatialSpectrumNode,
                    "Spatial spectrum",
                    "Bearing over frequency",
                    $crate::patch::Face::sized(560, 420).min(400, 300),
                    [array_in]
                ),
                (
                    Correlator,
                    "correlator",
                    $crate::processor::correlator::CorrelatorParams,
                    $crate::processor::correlator::CorrelatorReading,
                    CorrelatorNode,
                    "Correlator",
                    "Baseline visibilities",
                    $crate::patch::Face::sized(520, 460).min(380, 340),
                    [array_in]
                ),
                (
                    Polarimeter,
                    "polarimeter",
                    $crate::processor::polarimeter::PolarimeterParams,
                    $crate::processor::polarimeter::PolarimeterReading,
                    PolarimeterNode,
                    "Polarimeter",
                    "Polarisation of two crossed antennas",
                    $crate::patch::Face::fit(360),
                    [array_in, beam_out("Matched to the wave")]
                ),
            ],
            probe: (Probe, "probe", $crate::processor::ProbeParams),
        }
    };
}

pub(crate) use processors;

macro_rules! define_processor_enums {
    (
        processors: [$((
            $variant:ident,
            $type_id:literal,
            $params:ty,
            $reading:ty,
            $node:ident,
            $name:literal,
            $summary:literal,
            $face:expr,
            [$($port:tt)*]
        )),* $(,)?],
        probe: ($probe:ident, $probe_id:literal, $probe_params:ty) $(,)?
    ) => {
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(tag = "type", content = "settings")]
        pub enum ProcessorParams {
            $(
                #[serde(rename = $type_id)]
                $variant($params),
            )*
            #[cfg(feature = "probe")]
            #[serde(rename = $probe_id)]
            $probe($probe_params),
        }

        impl ProcessorParams {
            #[must_use]
            pub const fn type_id(&self) -> &'static str {
                match self {
                    $(Self::$variant(_) => $type_id,)*
                    #[cfg(feature = "probe")]
                    Self::$probe(_) => $probe_id,
                }
            }

            #[must_use]
            pub fn problem(&self) -> Option<&'static str> {
                match self {
                    $(Self::$variant(params) => params.problem(),)*
                    #[cfg(feature = "probe")]
                    Self::$probe(params) => params.problem(),
                }
            }

            #[must_use]
            pub fn valid(&self) -> bool {
                self.problem().is_none()
            }
        }

        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
        #[serde(tag = "type", content = "reading", rename_all = "snake_case")]
        pub enum ProcessorReading {
            $(
                #[serde(rename = $type_id)]
                $variant($reading),
            )*
        }

        impl ProcessorReading {
            #[must_use]
            pub const fn type_id(&self) -> &'static str {
                match self {
                    $(Self::$variant(_) => $type_id,)*
                }
            }

            #[must_use]
            pub fn empty(type_id: &str) -> Option<Self> {
                match type_id {
                    $($type_id => Some(Self::$variant(<$reading>::reserved())),)*
                    _ => None,
                }
            }
        }

        pub const PROCESSOR_TYPE_IDS: &[&str] = &[$($type_id),*];
    };
}

processors!(define_processor_enums);

#[cfg(feature = "probe")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProbeParams {
    pub time: bool,
    pub phase: bool,
    pub gain: bool,
    pub spread: bool,
    pub lane_ports: u8,
    pub rebuild: u32,
    pub block_drop_ms: u32,
}

#[cfg(feature = "probe")]
impl ProbeParams {
    #[must_use]
    pub const fn problem(&self) -> Option<&'static str> {
        None
    }
}

#[cfg(test)]
mod tests;
