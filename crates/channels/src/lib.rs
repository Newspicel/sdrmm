//! The built-in decoders, one private module each, registered by [`builtin::install`].

mod acars;
mod adsb;
mod ais;
mod am;
mod aprs;
mod apt;
mod atv;
mod ble;
mod bss;
pub mod builtin;
mod cw_skimmer;
mod dab;
mod datv;
mod dect;
mod dmr;
mod dpmr;
mod drm;
mod dsc;
mod dstar;
mod dtmf;
mod dvbt;
mod edacs;
mod eot;
mod ermes;
mod fleetsync;
mod flex;
mod freedv;
mod gk2a_lrit;
mod gnss;
mod goes_hrit;
mod gsm;
mod hd_radio;
mod hfdl;
mod horus;
mod hrpt;
mod ident;
mod ils;
mod inmarsat_aero;
mod inmarsat_stdc;
mod iridium;
mod ism;
mod ism_survey;
mod lora;
mod lrpt;
mod ltr;
mod m17;
mod mdc1200;
mod morse;
mod mpt1327;
mod navtex;
mod nfm;
mod noise_figure;
mod nxdn;
mod p25;
mod per;
mod pocsag;
mod psk;
mod radio_clock;
mod radiosonde;
mod remote_id;
mod rtty;
mod same;
mod sarsat;
mod selcall;
mod sid;
mod ssb;
mod sstv;
mod stanag;
mod tetra;
mod tetrapol;
mod uat;
mod vdl2;
mod vor;
mod weak_signal;
mod wefax;
mod wenet;
mod wfm;
mod wifi;
mod wmbus;
mod ysf;

/// Each decoder's own signal generators, the one public path to them as decoder modules are
/// private. Shared waveforms live in `sdrmm_modem::synth`.
#[cfg(any(test, feature = "synth"))]
pub mod synth {
    pub use crate::acars::synth as acars;
    pub use crate::adsb::synth as adsb;
    pub use crate::ais::synth as ais;
    pub use crate::am::synth as am;
    pub use crate::aprs::synth as aprs;
    pub use crate::apt::synth as apt;
    pub use crate::atv::synth as atv;
    pub use crate::ble::synth as ble;
    pub use crate::bss::synth as bss;
    pub use crate::dab::synth as dab;
    pub use crate::datv::synth as datv;
    pub use crate::dect::synth as dect;
    pub use crate::dmr::synth as dmr;
    pub use crate::dpmr::synth as dpmr;
    pub use crate::drm::synth as drm;
    pub use crate::dstar::synth as dstar;
    pub use crate::dtmf::synth as dtmf;
    pub use crate::dvbt::synth as dvbt;
    pub use crate::edacs::synth as edacs;
    pub use crate::eot::synth as eot;
    pub use crate::ermes::synth as ermes;
    pub use crate::flex::synth as flex;
    pub use crate::gk2a_lrit::synth as gk2a_lrit;
    pub use crate::gnss::synth as gnss;
    pub use crate::goes_hrit::synth as goes_hrit;
    pub use crate::gsm::synth as gsm;
    #[cfg(any(test, feature = "fixtures"))]
    pub use crate::ident::fixtures as ident_fixtures;
    pub use crate::ism_survey::synth as ism_survey;
    pub use crate::lora::synth as lora;
    pub use crate::lrpt::synth as lrpt;
    pub use crate::ltr::synth as ltr;
    pub use crate::m17::synth as m17;
    pub use crate::morse::synth as morse;
    pub use crate::mpt1327::synth as mpt1327;
    pub use crate::navtex::synth as navtex;
    pub use crate::nfm::synth as nfm;
    pub use crate::noise_figure::synth as noise_figure;
    pub use crate::nxdn::synth as nxdn;
    pub use crate::p25::synth as p25;
    pub use crate::per::synth as per;
    pub use crate::pocsag::synth as pocsag;
    pub use crate::psk::synth as psk;
    pub use crate::radio_clock::synth as radio_clock;
    pub use crate::radiosonde::synth as radiosonde;
    pub use crate::remote_id::synth as remote_id;
    pub use crate::rtty::synth as rtty;
    pub use crate::same::synth as same;
    pub use crate::sarsat::synth as sarsat;
    pub use crate::selcall::synth as selcall;
    pub use crate::sid::synth as sid;
    pub use crate::ssb::synth as ssb;
    pub use crate::sstv::synth as sstv;
    pub use crate::tetra::synth as tetra;
    pub use crate::tetrapol::synth as tetrapol;
    pub use crate::uat::synth as uat;
    pub use crate::vor::synth as vor;
    pub use crate::weak_signal::synth as weak_signal;
    pub use crate::wefax::synth as wefax;
    pub use crate::wenet::synth as wenet;
    pub use crate::wfm::rds::synth as rds;
    pub use crate::wfm::synth as wfm;
    pub use crate::wmbus::synth as wmbus;
    pub use crate::ysf::synth as ysf;
}

pub use acars::AcarsChannel;
pub use adsb::AdsbChannel;
pub use ais::AisChannelRx;
pub use am::{AmChannel, AmEncoderSettings, AmModulator};
pub use aprs::{AprsChannel, AprsEncoderSettings, AprsModulator, MicE, MicEBit};
pub use apt::AptChannel;
pub use atv::AtvChannel;
pub use ble::BleChannel;
pub use bss::{BssChannel, BssPacket, BssParams};
pub use cw_skimmer::CwSkimmerChannel;
pub use dab::DabChannel;
pub use datv::DatvChannel;
#[cfg(any(test, feature = "synth"))]
pub use datv::dvbs2::frame::Modulation as Dvbs2Modulation;
pub use dect::DectChannel;
pub use dmr::DmrChannel;
pub use dpmr::DpmrChannel;
pub use drm::DrmChannel;
pub use dsc::DscChannel;
pub use dstar::DstarChannel;
pub use dtmf::DtmfChannel;
pub use dvbt::{DvbtBandwidth, DvbtChannel, DvbtParams, DvbtStandard, t2 as dvbt2};
pub use eot::EotChannel;
pub use ermes::ErmesChannel;
pub use flex::FlexChannel;
pub use freedv::FreeDvChannel;
pub use gk2a_lrit::Gk2aLritChannel;
pub use gnss::GnssChannel;
pub use goes_hrit::GoesHritChannel;
pub use gsm::GsmChannel;
pub use hd_radio::{HdRadioChannel, HdRadioParams};
pub use hfdl::HfdlChannel;
pub use hrpt::HrptChannel;
pub use ident::IdentChannel;
pub use ils::IlsChannel;
pub use inmarsat_aero::InmarsatAeroChannel;
pub use inmarsat_stdc::InmarsatStdcChannel;
pub use iridium::IridiumChannel;
pub use ism::IsmChannel;
pub use ism_survey::IsmSurveyChannel;
pub use lora::LoraChannel;
pub use lrpt::LrptChannel;
pub use m17::M17Channel;
pub use morse::MorseChannel;
pub use mpt1327::Mpt1327Channel;
pub use navtex::NavtexChannel;
pub use nfm::{NfmChannel, NfmEncoderSettings, NfmModulator};
pub use noise_figure::NoiseFigureChannel;
pub use nxdn::NxdnChannel;
pub use p25::{P25Channel, P25p2Channel};
pub use per::PerChannel;
pub use pocsag::PocsagChannel;
pub use psk::PskChannel;
pub use radio_clock::RadioClockChannel;
pub use radiosonde::RadiosondeChannel;
pub use remote_id::RemoteIdChannel;
pub use rtty::RttyChannel;
pub use same::SameChannel;
pub use sarsat::SarsatChannel;
#[cfg(any(test, feature = "synth"))]
pub use sdrmm_modem::dvb::ldpc::Rate as Dvbs2Rate;
pub use selcall::SelcallChannel;
pub use sid::SidChannel;
pub use ssb::{SsbChannel, SsbEncoderSettings, SsbModulator};
pub use sstv::SstvChannel;
pub use stanag::StanagChannel;
pub use tetra::TetraChannel;
pub use tetrapol::TetrapolChannel;
pub use uat::UatChannel;
pub use vdl2::Vdl2Channel;
pub use vor::VorChannel;
pub use weak_signal::{Ft4Channel, Ft8Channel, Js8Channel, Jt9Channel, Jt65Channel, WsprChannel};
pub use wefax::WefaxChannel;
pub use wenet::WenetChannel;
pub use wfm::WfmChannel;
pub use wifi::WifiOccupancyChannel;
pub use wmbus::WmbusChannel;
pub use ysf::YsfChannel;

pub use acars::{AcarsMessage, AcarsParams};
pub use adsb::{AdsbMessage, AdsbParams};
pub use ais::{AisChannel, AisMessage, AisParams};
pub use am::AmParams;
pub use aprs::{AprsMode, AprsPacket, AprsParams, AprsWeather};
pub use apt::{AptImage, AptParams, AvhrrChannel};
pub use atv::{AtvColor, AtvModulation, AtvPalSplit, AtvParams, AtvStandard};
pub use ble::{
    BleAddress, BleAddressKind, BleAdi, BleAdvert, BleAuxPointer, BleBeacon, BleFlags, BleLink,
    BleManufacturer, BleParams, BlePdu, BlePhy, BleService,
};
pub use cw_skimmer::{CwSkimmerParams, CwSkimmerSpot};
pub use dab::{DabMode, DabParams, DabTransmissionMode};
pub use datv::{
    DatvCodeRate, DatvParams, DatvRollOff, DatvStandard, MAX_DATV_SYMBOL_RATE, MAX_SUPERFRAME_CODE,
    MIN_DATV_SYMBOL_RATE, SuperframeStatus,
};
pub use dect::{
    DECT_CARRIER_SPACING_HZ, DectArc, DectBand, DectCapability, DectCipherState, DectFrame,
    DectIdentity, DectParams, DectSecurity, DectSide, DectSides, DectSpan, DectUpdate, DectVoice,
};
pub use dmr::{DmrParams, DmrSlots};
pub use dpmr::DpmrParams;
pub use drm::{DrmMode, DrmParams};
pub use dsc::DscParams;
pub use dstar::DstarParams;
pub use dtmf::{DTMF_GAPS_MS, DtmfPace, DtmfParams, DtmfSequence};
pub use eot::{
    EotArming, EotBattery, EotMessage, EotParams, EotReport, EotStatus, HotCommand, HotRequest,
};
pub use ermes::{ErmesMessage, ErmesParams};
pub use flex::{FlexMessage, FlexParams};
pub use freedv::{FreeDvMode, FreeDvParams};
pub use gk2a_lrit::{Gk2aLritImage, Gk2aLritParams};
pub use gnss::{GnssFix, GnssFrame, GnssParams};
pub use goes_hrit::{GoesHritImage, GoesHritParams, GoesPicture};
pub use gsm::{
    GSM_CHANNEL_SPACING_HZ, GSM_SYMBOL_RATE_HZ, GsmAssignment, GsmBand, GsmCcch, GsmCell,
    GsmChannelType, GsmDedicated, GsmFrame, GsmHopping, GsmIdentity, GsmIdentityKind,
    GsmLapdmFrame, GsmLogicalChannel, GsmMessage, GsmParams, GsmProtocol, GsmSi, cipher_label,
};
pub use hfdl::HfdlParams;
pub use hrpt::{HrptImage, HrptParams, HrptPicture};
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
pub use ism_survey::{IsmKind, IsmKindLoad, IsmSurveyParams, IsmSurveyReport, SURVEY_CENTRES};
pub use lora::{
    LoraBandwidth, LoraCodingRate, LoraFrame, LoraImplicitHeader, LoraIntegrity, LoraIq, LoraKey,
    LoraKeyField, LoraParams, LoraPayload, LoraProtocol, LoraSpreadingFactor, LorawanFrame,
    LorawanMessageType, MAX_LORA_KEY_NAME_LEN, MAX_LORA_KEY_TEXT_LEN, MAX_LORA_KEYS,
    MeshcoreContent, MeshcoreNodeType, MeshcorePacket, MeshcorePayloadType, MeshcoreRoute,
    MeshtasticContent, MeshtasticEncryption, MeshtasticMetric, MeshtasticNeighbor,
    MeshtasticPacket,
};
pub use lrpt::{LrptImage, LrptMode, LrptParams};
pub use m17::M17Params;
pub use morse::{MorseParams, MorseText};
pub use mpt1327::Mpt1327Params;
pub use navtex::{NavtexMessage, NavtexParams};
pub use nfm::{NfmParams, NfmScramblerMode, NfmToneMode, ScramblerStatus};
pub use noise_figure::{
    MAX_ENR_DB, MEASUREMENT_REPORT_MS, MIN_ENR_DB, NoiseFigureParams, NoiseFigureReport,
};
pub use nxdn::{NxdnBandwidth, NxdnParams};
pub use p25::{P25Params, P25p2Params, P25p2Slots};
pub use per::{MAX_PER_PAYLOAD_BYTES, MIN_PER_PAYLOAD_BYTES, PerBaud, PerParams, PerReport};
pub use pocsag::{PocsagBaud, PocsagMessage, PocsagParams, PocsagPayload};
pub use psk::{PskBaud, PskParams, PskText};
pub use radio_clock::{RadioClockFrame, RadioClockParams, RadioClockStandard};
pub use radiosonde::{
    DEFAULT_SONDE_BURST_M, RadiosondeFrame, RadiosondeParams, SondeForecast, SondeType,
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
pub use sdrmm_wire::PagerPayload;
pub use sdrmm_wire::VoiceKey;
pub use sdrmm_wire::{
    AIRTIME_REPORT_MS, AirtimeSpan, MAX_AIRTIME_MARGIN_DB, MIN_AIRTIME_MARGIN_DB,
};
pub use sdrmm_wire::{MAX_NAVAID_REPORT_MS, MIN_NAVAID_REPORT_MS};
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
pub use uat::{FisbReport, UatParams};
pub use vdl2::Vdl2Params;
pub use vor::{VorParams, VorReading};
pub use weak_signal::{
    Fst4Message, Fst4Params, Fst4Period, Fst4wParams, Fst4wPeriod, Fst4wSpot, Js8Message,
    Msk144Params, Q65Params, Q65Period, Q65Submode, WsjtMessage, WsjtParams, WsprParams, WsprSpot,
};
pub use wefax::{WefaxIoc, WefaxLpm, WefaxParams, WefaxPicture};
pub use wenet::{WenetImage, WenetMode, WenetPacket, WenetParams, WenetTelemetry};
pub use wfm::rds::RdsUpdate;
pub use wfm::{DEEMPHASIS_US, WfmParams};
pub use wifi::{WifiBand, WifiChannelLoad, WifiOccupancyParams, WifiOccupancyReport};
pub use wmbus::{
    KEY_DIGITS, MAX_WMBUS_KEYS, METER_DIGITS, WMBUS_BANDWIDTH_HZ, WMBUS_S_HZ, WMBUS_SAMPLE_RATE_HZ,
    WMBUS_T_C_HZ, WmbusFormat, WmbusFunction, WmbusKey, WmbusMessage, WmbusMode, WmbusParams,
    WmbusQuantity, WmbusRecord, WmbusSecurity, control_label, device_label,
};
pub use ysf::YsfParams;
