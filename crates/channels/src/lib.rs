mod acars;
mod adsb;
mod airtime;
mod ais;
mod am;
mod aprs;
mod apt;
pub mod array_processor;
mod atv;
pub mod audio_chain;
pub mod band;
pub mod beamformer;
mod ble;
mod broadcast_audio;
mod broadcast_media;
mod builtin;
pub mod correlator;
mod crypto;
mod cw_skimmer;
mod dab;
mod datalink;
mod datv;
mod dect;
pub mod df;
mod drm;
mod dsc;
mod dv;
mod eot;
mod ermes;
mod flex;
mod front;
mod gnss;
mod gsm;
mod hfdl;
pub mod hunt_sweep;
mod ident;
pub mod if_stage;
mod ils;
mod inmarsat_aero;
mod inmarsat_stdc;
mod iridium;
mod ism;
mod ism_survey;
mod lora;
mod lrpt;
pub mod monitor;
mod morse;
mod navtex;
pub mod neural;
pub mod neural_denoise;
mod nfm;
mod noise_figure;
pub mod passive_radar;
mod per;
mod pocsag;
pub mod polarimeter;
pub mod pose_clock;
mod psk;
mod radio_clock;
mod radiosonde;
mod rds;
pub mod registry;
mod remote_id;
mod rtty;
mod same;
mod sarsat;
mod selcall;
mod sid;
pub mod spatial_spectrum;
mod ssb;
mod sstv;
mod stanag;
pub mod stitch;
pub mod symbols;
mod tetra;
mod tetrapol;
pub mod tone_squelch;
mod vdl2;
mod voice_inversion;
mod vor;
mod weak_signal;
mod wefax;
mod wfm;
mod wifi;
mod wmbus;
#[cfg(test)]
mod xng_adapter;

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod testutil;

#[cfg(any(test, feature = "fixtures"))]
pub mod reference_data;
#[cfg(any(test, feature = "synth"))]
pub mod synth;

pub use acars::AcarsChannel;
pub use adsb::AdsbChannel;
pub use ais::AisChannelRx;
pub use am::{AmChannel, AmTx};
pub use aprs::{AprsChannel, AprsTx, MicE, MicEBit};
pub use apt::AptChannel;
pub use atv::AtvChannel;
pub use audio_chain::{AudioChain, ClickProfile, click_profile};
pub use ble::BleChannel;
#[cfg(feature = "fuzzing")]
pub use broadcast_audio::decode_layer_two;
#[cfg(feature = "fuzzing")]
pub use broadcast_media::decode_bitstream;
pub use cw_skimmer::CwSkimmerChannel;
pub use dab::DabChannel;
#[cfg(any(test, feature = "synth"))]
pub use datv::dvbs2::{frame::Modulation as Dvbs2Modulation, ldpc::Rate as Dvbs2Rate};
pub use datv::{
    DatvChannel,
    dvbt::{DvbtChannel, t2 as dvbt2},
};
pub use dect::DectChannel;
pub use drm::DrmChannel;
pub use dsc::DscChannel;
pub use dv::chip;
pub use dv::{
    DmrChannel, DpmrChannel, DstarChannel, FreeDvChannel, M17Channel, NxdnChannel, P25Channel,
    YsfChannel,
};
pub use eot::EotChannel;
pub use ermes::ErmesChannel;
pub use flex::FlexChannel;
pub use front::ChannelFront;
pub use gnss::GnssChannel;
pub use gsm::GsmChannel;
pub use hfdl::HfdlChannel;
pub use ident::IdentChannel;
pub use ils::IlsChannel;
pub use inmarsat_aero::InmarsatAeroChannel;
pub use inmarsat_stdc::InmarsatStdcChannel;
pub use iridium::IridiumChannel;
pub use ism::IsmChannel;
pub use ism_survey::IsmSurveyChannel;
pub use lora::LoraChannel;
pub use lrpt::LrptChannel;
pub use morse::MorseChannel;
pub use navtex::NavtexChannel;
pub use nfm::{NfmChannel, NfmTx};
pub use noise_figure::NoiseFigureChannel;
use num_complex::Complex;
pub use per::PerChannel;
pub use pocsag::PocsagChannel;
pub use psk::PskChannel;
pub use radio_clock::RadioClockChannel;
pub use radiosonde::RadiosondeChannel;
pub use remote_id::RemoteIdChannel;
pub use rtty::RttyChannel;
pub use same::SameChannel;
pub use sarsat::SarsatChannel;
use sdrmm_dsp::{Decimator, FirC};
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, ChannelStatus, DecoderEvent, PositionFix};
pub use selcall::SelcallChannel;
pub use sid::SidChannel;
pub use ssb::{SsbChannel, SsbTx};
pub use sstv::SstvChannel;
pub use stanag::StanagChannel;
pub use symbols::SymbolTap;
pub use tetra::TetraChannel;
pub use tetrapol::TetrapolChannel;
pub use vdl2::Vdl2Channel;
pub use vor::VorChannel;
pub use weak_signal::{Ft4Channel, Ft8Channel, WsprChannel};
pub use wefax::WefaxChannel;
pub use wfm::WfmChannel;
pub use wifi::WifiOccupancyChannel;
pub use wmbus::WmbusChannel;

pub use acars::{AcarsMessage, AcarsParams};
pub use adsb::{AdsbMessage, AdsbParams};
pub use airtime::{
    AIRTIME_REPORT_MS, AirtimeSpan, IsmKind, IsmKindLoad, IsmSurveyParams, IsmSurveyReport,
    MAX_AIRTIME_MARGIN_DB, MIN_AIRTIME_MARGIN_DB, SURVEY_CENTRES, WifiBand, WifiChannelLoad,
    WifiOccupancyParams, WifiOccupancyReport,
};
pub use ais::{AisChannel, AisMessage, AisParams};
pub use am::AmParams;
pub use aprs::{AprsMode, AprsPacket, AprsParams, AprsWeather};
pub use apt::{AptImage, AptParams, AvhrrChannel};
pub use atv::{AtvColor, AtvModulation, AtvParams, AtvStandard};
pub use ble::{
    BLUETOOTH_BAND_RATE_HZ, BLUETOOTH_CHANNEL_WIDTH_HZ, BLUETOOTH_RATE_HZ, BleAddress,
    BleAddressKind, BleAdi, BleAdvert, BleAuxPointer, BleBeacon, BleFlags, BleLink,
    BleManufacturer, BleParams, BlePdu, BlePhy, BleService,
};
pub use broadcast_media::{
    BroadcastService, BroadcastServiceKind, BroadcastStatus, BroadcastSystem,
};
pub use cw_skimmer::{CwSkimmerParams, CwSkimmerSpot};
pub use dab::{DabMode, DabParams, DabTransmissionMode};
pub use datalink::DataLinkMessage;
pub use datv::{
    DatvCodeRate, DatvParams, DatvRollOff, DatvStandard, DvbtBandwidth, DvbtParams, DvbtStandard,
    MAX_DATV_SYMBOL_RATE, MAX_SUPERFRAME_CODE, MIN_DATV_SYMBOL_RATE, SuperframeStatus,
};
pub use dect::{
    DECT_CARRIER_SPACING_HZ, DectArc, DectBand, DectCapability, DectCipherState, DectFrame,
    DectIdentity, DectParams, DectSecurity, DectSide, DectSides, DectSpan, DectUpdate, DectVoice,
};
pub use drm::{DrmMode, DrmParams};
pub use dsc::DscParams;
pub use dv::{
    DmrParams, DmrSlots, DpmrParams, DstarParams, FreeDvMode, FreeDvParams, M17Params,
    NxdnBandwidth, NxdnParams, P25Params, VoiceKey, YsfParams,
};
pub use eot::{
    EotArming, EotBattery, EotMessage, EotParams, EotReport, EotStatus, HotCommand, HotRequest,
};
pub use ermes::{ErmesMessage, ErmesParams};
pub use flex::{FlexMessage, FlexParams, PagerPayload};
pub use gnss::{GnssFrame, GnssParams};
pub use gsm::{
    GSM_CHANNEL_SPACING_HZ, GSM_SYMBOL_RATE_HZ, GsmAssignment, GsmBand, GsmCcch, GsmCell,
    GsmChannelType, GsmDedicated, GsmFrame, GsmHopping, GsmIdentity, GsmIdentityKind,
    GsmLapdmFrame, GsmLogicalChannel, GsmMessage, GsmParams, GsmProtocol, GsmSi, cipher_label,
};
pub use hfdl::HfdlParams;
pub use ident::{
    IdentParams, MAX_IDENT_BANDWIDTH_HZ, MAX_IDENT_INTERVAL_MS, MAX_IDENT_THRESHOLD_DB,
    MIN_IDENT_BANDWIDTH_HZ, MIN_IDENT_INTERVAL_MS, MIN_IDENT_THRESHOLD_DB,
};
pub use ils::{IlsComponent, IlsParams, IlsReading};
pub use inmarsat_aero::{AeroChannel, InmarsatAeroParams};
pub use inmarsat_stdc::{InmarsatStdcParams, SeaArea, navarea_limits, roman};
pub use iridium::{IridiumParams, IridiumSpan};
pub use ism::{
    ISM_BANDWIDTH_HZ, ISM_SAMPLE_RATE_HZ, IsmCoding, IsmIntegrity, IsmMessage, IsmModulation,
    IsmParams, UNKNOWN_MODEL,
};
pub use lora::{
    LoraBandwidth, LoraCodingRate, LoraFrame, LoraImplicitHeader, LoraIntegrity, LoraIq, LoraKey,
    LoraKeyField, LoraParams, LoraPayload, LoraProtocol, LoraSpreadingFactor, LorawanFrame,
    LorawanMessageType, MAX_LORA_KEY_NAME_LEN, MAX_LORA_KEY_TEXT_LEN, MAX_LORA_KEYS,
    MeshcoreContent, MeshcoreNodeType, MeshcorePacket, MeshcorePayloadType, MeshcoreRoute,
    MeshtasticContent, MeshtasticEncryption, MeshtasticMetric, MeshtasticNeighbor,
    MeshtasticPacket,
};
pub use lrpt::{LrptImage, LrptMode, LrptParams};
pub use morse::{MorseParams, MorseText};
pub use navtex::{NavtexMessage, NavtexParams};
pub use nfm::{
    CTCSS_TONES_HZ, DCS_CODES, NfmParams, NfmScramblerMode, NfmToneMode, ScramblerStatus,
    ToneSquelchStatus,
};
pub use noise_figure::{
    MAX_ENR_DB, MEASUREMENT_REPORT_MS, MIN_ENR_DB, NoiseFigureParams, NoiseFigureReport,
};
pub use per::{MAX_PER_PAYLOAD_BYTES, MIN_PER_PAYLOAD_BYTES, PerBaud, PerParams, PerReport};
pub use pocsag::{PocsagBaud, PocsagMessage, PocsagParams, PocsagPayload};
pub use psk::{PskBaud, PskParams, PskText};
pub use radio_clock::{RadioClockFrame, RadioClockParams, RadioClockStandard};
pub use radiosonde::{
    DEFAULT_SONDE_BURST_M, RadiosondeFrame, RadiosondeParams, SondeForecast, SondeType,
};
pub use rds::RdsUpdate;
pub use registry::{
    ChannelLayout, Decoder, Native, Registry, Trunking, Tuned, audio_channels, channel_filter,
    check, compose, create, create_tx, default_settings, descriptor, descriptors, input_rate,
    install_plugins, layout, normalize, occupied_band, registry, trunking, tune,
};
pub use remote_id::{
    AuthType, EuCategory, EuClassification, HeightReference, OperatorLocationType, RemoteIdFrame,
    RemoteIdLink, RemoteIdLocation, RemoteIdMessage, RemoteIdParams, RemoteIdPhy, RemoteIdSystem,
    RemoteIdTransport, UaStatus, UaType, UasIdType, WIDE_RATE_HZ, WIFI_CHANNEL_WIDTH_HZ,
};
pub use rtty::{RTTY_BAUDS, RTTY_SHIFTS_HZ, RttyParams, RttyStopBits, RttyText};
pub use same::{
    MAX_SAME_BANDWIDTH_HZ, MAX_SAME_LOCATIONS, MIN_SAME_BANDWIDTH_HZ, SAME_BANDWIDTH_HZ, SAME_BAUD,
    SAME_BURSTS, SAME_MARK_HZ, SAME_SPACE_HZ, SameAlert, SameBody, SameLocation, SameMessage,
    SameParams,
};
pub use sarsat::{
    MAX_SARSAT_BANDWIDTH_HZ, MIN_SARSAT_BANDWIDTH_HZ, SARSAT_BANDWIDTH_HZ, SarsatBeacon,
    SarsatMessage, SarsatParams, SarsatProtocol,
};
pub use selcall::{SelcallParams, SelcallSequence, SelcallSystem};
pub use sid::{
    MAX_SID_REPORT_S, MAX_SID_THRESHOLD_DB, MIN_SID_REPORT_S, MIN_SID_THRESHOLD_DB, SidParams,
    SidReport, SidStationLevel,
};
pub use ssb::SsbParams;
pub use sstv::{SstvMode, SstvModulation, SstvParams, SstvPicture};
pub use stanag::{
    MAX_STANAG_WINDOW_S, MIN_STANAG_WINDOW_S, STANAG_WINDOW_S, StanagInterleave, StanagModem,
    StanagParams, StanagRate, StanagReport,
};
pub use tetra::TetraParams;
pub use tetrapol::{
    MAX_TETRAPOL_SCRAMBLING, TETRAPOL_BANDWIDTH_HZ, TetrapolAddress, TetrapolAddressKind,
    TetrapolBand, TetrapolCallType, TetrapolCell, TetrapolGroupCall, TetrapolKey,
    TetrapolLogicalChannel, TetrapolMessage, TetrapolParams, TetrapolPayload, TetrapolSystemInfo,
};
pub use vdl2::Vdl2Params;
pub use vor::{MAX_NAVAID_REPORT_MS, MIN_NAVAID_REPORT_MS, VorParams, VorReading};
pub use weak_signal::{WsjtMessage, WsjtParams, WsprParams, WsprSpot};
pub use wefax::{WefaxIoc, WefaxLpm, WefaxParams, WefaxPicture};
pub use wfm::{DEEMPHASIS_US, WfmParams};
pub use wmbus::{
    KEY_DIGITS, MAX_WMBUS_KEYS, METER_DIGITS, WMBUS_BANDWIDTH_HZ, WMBUS_S_HZ, WMBUS_SAMPLE_RATE_HZ,
    WMBUS_T_C_HZ, WmbusFormat, WmbusFunction, WmbusKey, WmbusMessage, WmbusMode, WmbusParams,
    WmbusQuantity, WmbusRecord, WmbusSecurity, control_label, device_label,
};

pub const AUDIO_RATE: u32 = sdrmm_wire::audio::AUDIO_RATE_HZ;

/// A channel's settings as its decoder's struct, refusing another decoder's.
pub(crate) fn settings_of<P: sdrmm_wire::DecoderSettings>(
    settings: &ChannelSettings,
) -> Result<P, ChannelError> {
    settings
        .params
        .decode()
        .map_err(ChannelError::InvalidSettings)
}

pub(crate) const fn default_true() -> bool {
    true
}

pub(crate) fn voice_leveller() -> sdrmm_dsp::Agc {
    sdrmm_dsp::Agc::new(f64::from(AUDIO_RATE), 0.25, 0.005, 0.5, 100.0)
}

pub(crate) fn clamp_full_scale(pcm: &mut [f32]) {
    for s in pcm {
        *s = s.clamp(-1.0, 1.0);
    }
}

pub enum ChannelFilter {
    Symmetric(Decimator),
    Sideband(FirC),
    Passthrough,
}

impl ChannelFilter {
    /// How far the filter brings the rate down. A filter that decimates takes over the last
    /// step of the downconversion: the converter feeding it runs at the decoder's rate times
    /// this and guards only the decoder's band.
    #[must_use]
    pub fn decimation(&self) -> usize {
        match self {
            Self::Symmetric(filter) => filter.factor(),
            Self::Sideband(_) | Self::Passthrough => 1,
        }
    }

    pub fn reset(&mut self) {
        match self {
            Self::Symmetric(filter) => filter.reset(),
            Self::Sideband(filter) => filter.reset(),
            Self::Passthrough => {}
        }
    }

    pub fn process(&mut self, input: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        match self {
            Self::Symmetric(f) => f.process(input, out),
            Self::Sideband(f) => f.process(input, out),
            Self::Passthrough => {
                out.clear();
                out.extend_from_slice(input);
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error("unknown channel type: {0}")]
    UnknownType(String),
    #[error("invalid settings: {0}")]
    InvalidSettings(String),
    #[error("{0} has no modulator")]
    NoTransmitter(String),
    #[error("invalid payload: {0}")]
    InvalidPayload(String),
    #[error("{0}")]
    Refused(&'static str),
    #[error("{0}")]
    Unsupported(String),
}

impl From<sdrmm_modem::keying::QueueFull> for ChannelError {
    fn from(full: sdrmm_modem::keying::QueueFull) -> Self {
        Self::InvalidPayload(full.to_string())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ChannelCtx {
    pub input_rate: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VideoPicture {
    pub width: u16,
    pub height: u16,
    pub luma: Vec<u8>,
    pub rgb: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedImage {
    pub source: &'static str,
    pub mode: String,
    pub complete: bool,
    pub lines: u16,
    pub picture: VideoPicture,
}

#[derive(Default)]
pub struct ChannelOutputs {
    pub audio_pcm: Vec<f32>,
    pub audio_rate: u32,
    pub events: Vec<DecoderEvent>,
    pub video: Vec<VideoPicture>,
    pub images: Vec<DecodedImage>,
    pub symbols: SymbolTap,
    pub status: Option<ChannelStatus>,
}

impl ChannelOutputs {
    pub fn reset(&mut self) {
        self.audio_pcm.clear();
        self.audio_rate = 0;
        self.events.clear();
        self.video.clear();
        self.images.clear();
        self.symbols.clear();
        self.status = None;
    }
}

/// A decoder compiled into this crate: its static descriptor and how to open it.
pub trait NativeChannel: ChannelRx + Sized {
    fn descriptor() -> &'static ChannelDescriptor;

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError>;
}

/// A running decoder channel, built in or loaded as a plugin.
pub trait ChannelRx: Send {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError>;

    fn retuned(&mut self) {}

    fn position_changed(&mut self, _fix: Option<&PositionFix>) {}

    /// When the first sample of the next `process` call was captured, by the wall clock. Called
    /// only while the source's clock is known.
    fn captured_at(&mut self, _time: std::time::SystemTime) {}

    /// Where the receiver's own LO artifact falls in this channel's baseband, if it falls in it.
    ///
    /// A zero-IF front end's DC term is a carrier like any other and cannot be told apart from a
    /// real one by its shape, so anything measuring the passband has to be told to disregard it.
    fn lo_artifact_at(&mut self, _offset_hz: Option<f64>) {}

    fn needs_gated_input(&self) -> bool {
        true
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs);

    fn rejected(&self) -> u32 {
        0
    }

    fn skipped(&self) -> u32 {
        0
    }
}

pub enum TxPayload {
    Audio(Vec<f32>),
    Frame(Vec<u8>),
}

/// A modulator compiled into this crate: its static descriptor and how to open it.
pub trait NativeTx: ChannelTx + Sized {
    fn descriptor() -> &'static ChannelDescriptor;

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError>;
}

/// A running modulator, built in or loaded as a plugin.
pub trait ChannelTx: Send {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError>;

    fn submit(&mut self, payload: TxPayload) -> Result<(), ChannelError>;

    fn generate(&mut self, out: &mut [Complex<f32>]) -> usize;
}

pub(crate) fn check_input_rate(
    ctx: ChannelCtx,
    descriptor: &ChannelDescriptor,
) -> Result<(), ChannelError> {
    check_rate(ctx, descriptor, descriptor.input_rate_hz)
}

pub(crate) fn check_rate(
    ctx: ChannelCtx,
    descriptor: &ChannelDescriptor,
    expected: f64,
) -> Result<(), ChannelError> {
    if ctx.input_rate == expected {
        Ok(())
    } else {
        Err(ChannelError::InvalidSettings(format!(
            "{} expects {expected} Hz input, engine supplied {} Hz",
            descriptor.type_id, ctx.input_rate
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use sdrmm_wire::{CallGate, Capability, ChannelParams, EventKind, KindView, Sideband};

    use crate::am::AmParams;

    use crate::atv::AtvColor;

    use crate::atv::AtvParams;

    use crate::datv::DatvParams;

    use crate::nfm::NfmParams;

    use crate::ssb::SsbParams;

    use crate::wfm::WfmParams;

    use super::*;
    use crate::testutil::settings;

    fn default_params(type_id: &str) -> ChannelParams {
        ChannelParams::default_for(type_id)
            .unwrap_or_else(|| panic!("unexpected type id {type_id}"))
    }

    #[test]
    fn a_datv_input_rate_follows_its_symbol_rate() {
        let narrow = ChannelParams::new("datv", &DatvParams::default()).expect("datv settings");
        let wide = ChannelParams::new(
            "datv",
            &DatvParams {
                symbol_rate: 2_330_000.0,
                ..DatvParams::default()
            },
        )
        .expect("datv settings");
        assert_eq!(input_rate(&narrow), 2_000_000.0);
        assert_eq!(input_rate(&wide), 4_000_000.0);
        assert_eq!(
            input_rate(&ChannelParams::new("nfm", &NfmParams::default()).expect("nfm settings")),
            NfmChannel::descriptor().input_rate_hz
        );
    }

    #[test]
    fn descriptor_lookup_matches_catalogue() {
        let catalogue = descriptors();
        for expected in &catalogue {
            assert_eq!(descriptor(&expected.type_id).as_ref(), Some(expected));
        }
        assert!(descriptor("unknown").is_none());
    }

    #[test]
    fn only_protocols_the_identifier_recognizes_are_identifiable() {
        let identifiable = |kind: &str| descriptor(kind).is_some_and(|d| d.identifiable);
        for kind in [
            "nfm", "am", "wfm", "ssb", "dmr", "tetra", "pocsag", "adsb", "ft8",
        ] {
            assert!(identifiable(kind), "{kind}");
        }
        assert!(!identifiable("ident"));
        for descriptor in descriptors() {
            assert_eq!(
                descriptor.identifiable,
                !descriptor.signatures.is_empty(),
                "{}",
                descriptor.type_id
            );
        }
    }

    #[test]
    fn every_descriptor_has_a_short_summary() {
        for d in descriptors() {
            assert!(!d.summary.is_empty(), "{} has no summary", d.type_id);
            assert!(d.summary.len() <= 60, "{} summary is too long", d.type_id);
        }
    }

    #[test]
    fn descriptors_are_unique_and_complete() {
        let all = descriptors();
        let ids: HashSet<&str> = all.iter().map(|d| d.type_id.as_str()).collect();
        assert_eq!(ids.len(), all.len());
        for d in &all {
            let (bandwidth, rate) = match d.type_id.as_str() {
                "nfm" => (12_500.0, 48_000.0),
                "selcall" => (12_500.0, 48_000.0),
                "am" => (10_000.0, 48_000.0),
                "ssb" => (3_000.0, 48_000.0),
                "wfm" => (200_000.0, 240_000.0),
                "pocsag" => (12_500.0, 24_000.0),
                "flex" | "ermes" | "eot" => (12_500.0, 48_000.0),
                "sarsat" => (20_000.0, 24_000.0),
                "tetrapol" => (12_500.0, 48_000.0),
                "adsb" => (2_000_000.0, 2_400_000.0),
                "ais" => (25_000.0, 48_000.0),
                "aprs" => (12_500.0, 48_000.0),
                "rtty" => (1_000.0, 8_000.0),
                "morse" => (400.0, 8_000.0),
                "cw_skimmer" => (24_000.0, 48_000.0),
                "navtex" => (600.0, 8_000.0),
                "acars" => (12_500.0, 24_000.0),
                "atv" => (1_500_000.0, 20_000_000.0),
                "sstv" => (1_600.0, 16_000.0),
                "dab" => (1_536_000.0, 2_048_000.0),
                "datv" => (449_550.0, 2_000_000.0),
                "dvbt" => (8_000_000.0, 64_000_000.0 / 7.0),
                "drm" => (100_000.0, 192_000.0),
                "dmr" | "ysf" | "p25" => (12_500.0, 48_000.0),
                "dstar" | "nxdn" | "dpmr" => (6_250.0, 48_000.0),
                "m17" => (9_000.0, 48_000.0),
                "tetra" => (25_000.0, 72_000.0),
                "ft8" | "ft4" | "wspr" => (3_200.0, 12_000.0),
                "psk" => (650.0, 8_000.0),
                "freedv" => (1_400.0, 8_000.0),
                "ident" => (192_000.0, 240_000.0),
                "radio_clock" => (200.0, 2_000.0),
                "gnss" => (2_046_000.0, 2_048_000.0),
                "vor" => (24_000.0, 48_000.0),
                "ils" => (20_000.0, 48_000.0),
                "dsc" => (500.0, 8_000.0),
                "inmarsat_stdc" => (4_000.0, 12_000.0),
                "inmarsat_aero" => (13_000.0, 48_000.0),
                "vdl2" => (17_000.0, 100_000.0),
                "hfdl" => (6_000.0, 12_000.0),
                "iridium" => (50_000.0, 250_000.0),
                "dect" => (1_728_000.0, 2_304_000.0),
                "gsm" => (200_000.0, 1_625_000.0 / 3.0),
                "apt" => (40_000.0, 60_000.0),
                "lrpt" => (140_000.0, 288_000.0),
                "wefax" => (1_600.0, 12_000.0),
                "same" => (12_500.0, 50_000.0),
                "radiosonde" => (20_000.0, 48_000.0),
                "lora" => (125_000.0, 250_000.0),
                "remote_id" | "ble" => (2_000_000.0, 4_000_000.0),
                "wifi_occupancy" | "ism_survey" => (17_000_000.0, 20_000_000.0),
                "ism" => (240_000.0, 250_000.0),
                "wmbus" => (300_000.0, 400_000.0),
                "noise_figure" => (400_000.0, 500_000.0),
                "per" => (28_800.0, 96_000.0),
                "sid" => (42_000.0, 48_000.0),
                "stanag" => (3_400.0, 9_600.0),
                other => panic!("unexpected type id {other}"),
            };
            assert_eq!(d.bandwidth_hz, bandwidth, "{}", d.type_id);
            assert_eq!(d.input_rate_hz, rate, "{}", d.type_id);
            assert!(!d.name.is_empty(), "{}", d.type_id);
            assert!(
                d.has_audio || !d.emits.is_empty() || d.has_video,
                "{} produces neither audio, decoder events nor video",
                d.type_id
            );
            assert_eq!(
                d.has_audio,
                matches!(
                    d.type_id.as_str(),
                    "nfm"
                        | "am"
                        | "ssb"
                        | "wfm"
                        | "atv"
                        | "dab"
                        | "datv"
                        | "drm"
                        | "dvbt"
                        | "dmr"
                        | "dstar"
                        | "ysf"
                        | "nxdn"
                        | "p25"
                        | "dpmr"
                        | "m17"
                        | "freedv"
                        | "dect"
                ),
                "{} audio flag does not match its mode class",
                d.type_id
            );
            assert_eq!(
                d.has_video,
                matches!(
                    d.type_id.as_str(),
                    "atv" | "sstv" | "datv" | "dvbt" | "apt" | "lrpt" | "wefax"
                ),
                "{} video flag does not match its mode class",
                d.type_id
            );
        }
    }

    fn may_be_unset(field: &sdrmm_wire::SettingField) -> bool {
        use sdrmm_wire::Control;
        field.fallback.is_some()
            || !field.when.is_empty()
            || match &field.control {
                Control::Number(spec) => spec.optional.is_some(),
                Control::Text { optional, .. } => *optional,
                Control::Choice { options } => options.iter().any(|option| option.value.is_null()),
                Control::Live { .. }
                | Control::List { .. }
                | Control::Preset { .. }
                | Control::SerialPort { .. } => true,
                Control::Toggle | Control::Lanes { .. } | Control::Numbers { .. } => false,
            }
    }

    #[test]
    fn every_form_field_names_a_setting_its_defaults_carry() {
        for descriptor in descriptors() {
            let Some(defaults) = &descriptor.defaults else {
                panic!("{} has no defaults", descriptor.type_id);
            };
            let settings = &defaults.params.settings;
            for field in descriptor
                .settings
                .iter()
                .filter(|field| !may_be_unset(field))
            {
                assert!(
                    sdrmm_wire::channel::lookup(settings, &field.key).is_some(),
                    "{} form names {} which its settings lack",
                    descriptor.type_id,
                    field.key
                );
            }
            assert!(
                defaults.check_limits(&descriptor.settings).is_ok(),
                "{} defaults break their own form",
                descriptor.type_id
            );
        }
    }

    #[test]
    fn every_emitted_kind_is_in_the_catalog() {
        registry::init();
        let catalog = sdrmm_wire::catalog();
        for descriptor in descriptors() {
            for kind in &descriptor.emits {
                assert!(
                    catalog.entry(kind).is_some(),
                    "{} emits unknown {kind}",
                    descriptor.type_id
                );
            }
        }
    }

    #[test]
    fn every_view_hint_is_one_the_frontend_can_draw() {
        registry::init();
        for entry in sdrmm_wire::catalog().entries() {
            for view in &entry.views {
                let fields = view.fields();
                match view {
                    KindView::Table { columns, sort, .. } => {
                        let unique: std::collections::BTreeSet<_> = columns.iter().collect();
                        assert_eq!(unique.len(), columns.len(), "{} repeats", entry.kind);
                        assert!(
                            sort.as_ref()
                                .is_none_or(|sort| columns.contains(&sort.field)),
                            "{} sorts by a hidden column",
                            entry.kind
                        );
                    }
                    KindView::Meter { min, max, .. } => {
                        assert!(min < max, "{} meter range", entry.kind);
                    }
                    KindView::Trend { window_s, .. } => {
                        assert!(*window_s > 0, "{} trend window", entry.kind);
                    }
                    KindView::Picture => {
                        assert!(
                            entry.has(Capability::Picture),
                            "{} has no picture",
                            entry.kind
                        );
                    }
                }
                assert!(
                    matches!(view, KindView::Picture) || !fields.is_empty(),
                    "{} view names no field",
                    entry.kind
                );
            }
        }
    }

    #[test]
    fn calls_follow_the_mode_class() {
        for d in descriptors() {
            let expected = match d.type_id.as_str() {
                "dmr" | "dstar" | "ysf" | "nxdn" | "p25" | "dpmr" | "m17" | "freedv" | "tetra" => {
                    Some(CallGate::Frames)
                }
                "nfm" | "am" | "ssb" | "dect" => Some(CallGate::Squelch),
                _ => None,
            };
            assert_eq!(d.calls, expected, "{}", d.type_id);
            assert_eq!(
                d.calls == Some(CallGate::Frames),
                d.emits.contains(&EventKind::DV),
                "{} gates calls by frames exactly when it emits digital voice",
                d.type_id
            );
            assert!(
                !d.emits.contains(&EventKind::CALL),
                "{} declares calls, which only the call tracker makes",
                d.type_id
            );
            if d.calls == Some(CallGate::Squelch) {
                assert!(
                    d.has_audio,
                    "{} gates calls by squelch without audio",
                    d.type_id
                );
            }
        }
    }

    #[test]
    fn audio_channels_matches_the_frames_each_mode_produces() {
        const LEN: usize = 96_000;
        for d in descriptors().into_iter().filter(|d| d.has_audio) {
            let params = default_params(&d.type_id);
            let channels = usize::from(audio_channels(&params));
            let ctx = ChannelCtx {
                input_rate: d.input_rate_hz,
            };
            let mut chan = create(ctx, &settings(params)).expect("builds");
            let audio = crate::testutil::run_ragged(
                chan.as_mut(),
                &crate::testutil::complex_noise(7, 0.5, LEN),
            );
            assert_eq!(
                audio.len() % channels,
                0,
                "{} emitted a partial sample frame",
                d.type_id
            );
            if d.calls == Some(CallGate::Frames) || d.emits.contains(&EventKind::named("broadcast"))
            {
                continue;
            }
            if d.type_id == "atv" && audio.is_empty() {
                continue;
            }
            let frames = audio.len() / channels;
            let expected = (LEN as f64 * f64::from(AUDIO_RATE) / d.input_rate_hz) as usize;
            assert!(
                frames <= expected && expected - frames < 200,
                "{} produced {frames} frames of {channels}-channel audio, expected ~{expected}",
                d.type_id
            );
        }
    }

    fn home_settings(type_id: &str) -> ChannelSettings {
        ChannelSettings {
            frequency_hz: default_settings(type_id).map_or(0.0, |defaults| defaults.frequency_hz),
            ..settings(default_params(type_id))
        }
    }

    #[test]
    fn create_builds_every_registered_type() {
        for d in descriptors() {
            let ctx = ChannelCtx {
                input_rate: d.input_rate_hz,
            };
            let built = create(ctx, &home_settings(&d.type_id));
            assert!(built.is_ok(), "{}: {:?}", d.type_id, built.err());
        }
    }

    #[test]
    fn every_transmitter_is_fed_by_audio_or_by_its_message_form() {
        for d in descriptors() {
            assert_eq!(
                d.can_transmit,
                d.transmits_audio != !d.message.is_empty(),
                "{}: audio {} message fields {}",
                d.type_id,
                d.transmits_audio,
                d.message.len()
            );
            let composed = compose(&d.type_id, &serde_json::json!({}));
            if d.message.is_empty() {
                assert!(
                    matches!(composed, Err(ChannelError::InvalidPayload(_))),
                    "{} sends no messages",
                    d.type_id
                );
            }
        }
        assert_eq!(descriptor("aprs").expect("aprs").message.len(), 4);
        assert!(descriptor("nfm").expect("nfm").transmits_audio);
    }

    #[test]
    fn can_transmit_matches_what_create_tx_will_build() {
        for d in descriptors() {
            let ctx = ChannelCtx {
                input_rate: d.input_rate_hz,
            };
            let built = create_tx(ctx, &home_settings(&d.type_id));
            assert_eq!(
                built.is_ok(),
                d.can_transmit,
                "{}: can_transmit {} but create_tx said {:?}",
                d.type_id,
                d.can_transmit,
                built.err()
            );
            if !d.can_transmit {
                assert!(
                    matches!(built, Err(ChannelError::NoTransmitter(_))),
                    "{} must refuse by naming the missing modulator",
                    d.type_id
                );
            }
        }
    }

    #[test]
    fn only_the_modes_with_a_modulator_transmit() {
        let transmitting: Vec<String> = descriptors()
            .into_iter()
            .filter(|d| d.can_transmit)
            .map(|d| d.type_id)
            .collect();
        assert_eq!(transmitting, ["nfm", "am", "ssb", "aprs"]);
    }

    #[test]
    fn create_rejects_mismatched_input_rate() {
        let ctx = ChannelCtx {
            input_rate: 96_000.0,
        };
        let err = create(
            ctx,
            &settings(ChannelParams::new("nfm", &NfmParams::default()).expect("nfm settings")),
        );
        assert!(matches!(err, Err(ChannelError::InvalidSettings(_))));
    }

    #[test]
    fn occupied_band_tracks_params_and_sideband() {
        assert_eq!(
            occupied_band(
                &ChannelParams::new(
                    "nfm",
                    &NfmParams {
                        bandwidth_hz: 25_000.0,
                        ..NfmParams::default()
                    }
                )
                .expect("nfm settings")
            ),
            (-12_500.0, 12_500.0)
        );
        assert_eq!(
            occupied_band(
                &ChannelParams::new(
                    "am",
                    &AmParams {
                        bandwidth_hz: 8_000.0,
                        ..AmParams::default()
                    }
                )
                .expect("am settings")
            ),
            (-4_000.0, 4_000.0)
        );
        assert_eq!(
            occupied_band(
                &ChannelParams::new(
                    "ssb",
                    &SsbParams {
                        sideband: Sideband::Usb,
                        bandwidth_hz: 10_000.0,
                    }
                )
                .expect("ssb settings")
            ),
            (100.0, 10_000.0)
        );
        assert_eq!(
            occupied_band(
                &ChannelParams::new(
                    "ssb",
                    &SsbParams {
                        sideband: Sideband::Lsb,
                        bandwidth_hz: 10_000.0,
                    }
                )
                .expect("ssb settings")
            ),
            (-10_000.0, -100.0)
        );
        assert_eq!(
            occupied_band(&ChannelParams::new("wfm", &WfmParams::default()).expect("wfm settings")),
            (-100_000.0, 100_000.0)
        );
        assert_eq!(
            occupied_band(
                &ChannelParams::new(
                    "atv",
                    &AtvParams {
                        color: AtvColor::Pal,
                        sound_subcarrier_hz: Some(5_500_000.0),
                        ..AtvParams::default()
                    }
                )
                .expect("atv settings")
            ),
            (-5_033_618.75, 5_565_000.0)
        );
    }

    fn filter_rms(filter: &mut ChannelFilter, freq_norm: f64) -> f32 {
        let mut out = Vec::new();
        filter.process(&crate::testutil::complex_tone(freq_norm, 8_192), &mut out);
        let settled = &out[512..];
        (settled.iter().map(|v| f64::from(v.norm_sqr())).sum::<f64>() / settled.len() as f64).sqrt()
            as f32
    }

    #[test]
    fn channel_filter_passes_in_channel_and_rejects_adjacent() {
        let mut f = channel_filter(
            &ChannelParams::new("nfm", &NfmParams::default()).expect("nfm settings"),
        )
        .unwrap();
        let pass = filter_rms(&mut f, 1_000.0 / 48_000.0);
        assert!((0.9..1.05).contains(&pass), "in-channel rms {pass}");
        let mut f = channel_filter(
            &ChannelParams::new("nfm", &NfmParams::default()).expect("nfm settings"),
        )
        .unwrap();
        let reject = filter_rms(&mut f, 15_000.0 / 48_000.0);
        assert!(reject < 0.01, "adjacent leak rms {reject}");

        let ssb = ChannelParams::new("ssb", &SsbParams::default()).expect("ssb settings");
        let mut f = channel_filter(&ssb).unwrap();
        let pass = filter_rms(&mut f, 1_000.0 / 48_000.0);
        assert!((0.9..1.05).contains(&pass), "usb rms {pass}");
        let mut f = channel_filter(&ssb).unwrap();
        let reject = filter_rms(&mut f, -1_000.0 / 48_000.0);
        assert!(reject < 0.01, "lsb leak rms {reject}");
    }

    #[test]
    fn channel_filter_rejects_out_of_range_bandwidth() {
        for params in [
            ChannelParams::new(
                "nfm",
                &NfmParams {
                    bandwidth_hz: f64::NAN,
                    ..NfmParams::default()
                },
            )
            .expect("nfm settings"),
            ChannelParams::new(
                "am",
                &AmParams {
                    bandwidth_hz: 0.0,
                    ..AmParams::default()
                },
            )
            .expect("am settings"),
            ChannelParams::new(
                "ssb",
                &SsbParams {
                    sideband: Sideband::Usb,
                    bandwidth_hz: 50.0,
                },
            )
            .expect("ssb settings"),
        ] {
            assert!(
                matches!(
                    channel_filter(&params),
                    Err(ChannelError::InvalidSettings(_))
                ),
                "{} must be rejected",
                params.type_id()
            );
        }
    }
}
