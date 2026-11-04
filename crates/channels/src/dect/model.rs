use sdrmm_wire::{Capability, EventContract, Hue, KindSpec, KindView};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::labelled;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::FieldValue;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::count;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::flag;
use sdrmm_wire::present::joined;
use sdrmm_wire::present::number;
use sdrmm_wire::present::text;
use sdrmm_wire::present::text_of;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DectBand {
    #[default]
    Eu,
    Us,
}

pub const DECT_CARRIER_SPACING_HZ: f64 = 1_728_000.0;

const DECT_CARRIER_TOLERANCE_HZ: f64 = 50_000.0;

impl DectBand {
    #[must_use]
    pub const fn carriers(self) -> u8 {
        match self {
            Self::Eu => 10,
            Self::Us => 5,
        }
    }

    #[must_use]
    pub fn carrier_hz(self, carrier: u8) -> Option<f64> {
        if carrier >= self.carriers() {
            return None;
        }
        let step = f64::from(carrier) * DECT_CARRIER_SPACING_HZ;
        Some(match self {
            Self::Eu => 1_897_344_000.0 - step,
            Self::Us => 1_921_536_000.0 + step,
        })
    }

    #[must_use]
    pub fn center_hz(self) -> f64 {
        let last = self.carriers() - 1;
        match (self.carrier_hz(0), self.carrier_hz(last)) {
            (Some(first), Some(last)) => (first + last) / 2.0,
            _ => 0.0,
        }
    }

    #[must_use]
    pub fn carrier_at(self, hz: f64) -> Option<u8> {
        (0..self.carriers()).find(|&carrier| {
            self.carrier_hz(carrier)
                .is_some_and(|center| (center - hz).abs() <= DECT_CARRIER_TOLERANCE_HZ)
        })
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Eu => "EU 1880-1900 MHz",
            Self::Us => "US 1920-1930 MHz",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DectSides {
    #[default]
    Both,
    Rfp,
    Pp,
}

impl DectSides {
    #[must_use]
    pub const fn accepts_rfp(self) -> bool {
        matches!(self, Self::Both | Self::Rfp)
    }

    #[must_use]
    pub const fn accepts_pp(self) -> bool {
        matches!(self, Self::Both | Self::Pp)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DectSpan {
    #[default]
    Carrier,
    Band,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DectParams {
    #[serde(default)]
    pub band: DectBand,
    #[serde(default)]
    pub sides: DectSides,
    #[serde(default)]
    pub span: DectSpan,
}

labelled! {
    DectBand { Eu => "EU", Us => "US" }
}

labelled! {
    DectSpan {
        Carrier => "Carrier": "One carrier at 2.304 MS/s",
        Band => "Band":
            "Every carrier at once. Tune to the band centre, radio at 20 MS/s (EU) or 10 MS/s (US)",
    }
}

labelled! {
    DectSides { Both => "Both", Rfp => "Base", Pp => "Handset" }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DectSide {
    #[default]
    Rfp,
    Pp,
}

impl DectSide {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Rfp => "base",
            Self::Pp => "handset",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DectArc {
    #[default]
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
}

impl DectArc {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::A => "A residential / small PBX",
            Self::B => "B private multi-cell",
            Self::C => "C public access",
            Self::D => "D public GSM/UMTS",
            Self::E => "E PP-to-PP direct",
            Self::F => "F reserved",
            Self::G => "G reserved",
            Self::H => "H reserved",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DectUpdate {
    #[default]
    Identity,
    SystemInfo,
    Capabilities,
    Encryption,
    Paging,
    Bearer,
    Voice,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DectCipherState {
    #[default]
    Clear,
    Requested,
    Confirmed,
    Active,
    Stopped,
}

impl DectCipherState {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Clear => "no encryption seen",
            Self::Requested => "start requested",
            Self::Confirmed => "start confirmed",
            Self::Active => "encryption active",
            Self::Stopped => "encryption stopped",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DectCapability {
    ExtendedFpInfo,
    DoubleDuplexBearer,
    DoubleSlot,
    HalfSlot,
    FullSlot,
    FrequencyControl,
    PageRepetition,
    CoSetupOnDummy,
    ClUplink,
    ClDownlink,
    BasicAFieldSetup,
    AdvancedAFieldSetup,
    BFieldSetup,
    CfMessages,
    InMinimumDelay,
    InNormalDelay,
    IpErrorDetection,
    IpErrorCorrection,
    MultibearerConnections,
    Adpcm,
    GapBasicSpeech,
    NonVoiceCircuitSwitched,
    NonVoicePacketSwitched,
    StandardAuthentication,
    StandardCiphering,
    LocationRegistration,
    SimServices,
    NonStaticFixedPart,
    CissServices,
    ClmsService,
    ComsService,
    AccessRightsRequests,
    ExternalHandover,
    ConnectionHandover,
    CrfpEncryption,
    RelayV2,
    ProlongedPreamble,
    FrequencyReplacement,
    MacSuspendResume,
    IpqService,
    ExtendedFpInfo2,
    FMmsInterworking,
    BasicOdap,
    GmeTransport,
    IpRoaming,
    Ethernet,
    TokenRing,
    Ip,
    Ppp,
    V24,
    RapPart1,
    IsdnIntermediateSystem,
    GpsSynchronized,
    TpuiRegistration,
    EmergencyCall,
    AsymmetricBearers,
    Lrms,
    DataServiceProfileD,
    DprsClass3Or4,
    DprsClass2,
    IsdnDataServices,
    LongSlot640,
    LongSlot672,
    EuMux,
    IpfAdvanced,
    SipfChannel,
    GfChannel,
    UleWrsDelayedPaging,
    ExtendedFpInfo3,
    NoEmissionAnyCarrier,
    WidebandVoice,
    PacketData1,
    PacketData2,
    PacketData3,
    PacketData4,
    PacketData5,
    ExtendedWidebandVoice,
    PermanentClir,
    ThirdPartyConference,
    IntrusionCall,
    CallDeflection,
    MultipleLines,
    NoEmission,
    NgDect5,
    UNemo,
    UNemoOpportunistic,
    UlePhase1,
    UlePhase1Revised,
    UlePhase2,
    UlePhase3,
    ReKeying,
    Dsaa2,
    Dsc2,
    LightData,
    HalfSlotSecondHalf,
    ModulationBpsk,
    ModulationQpsk,
    Modulation8psk,
    Modulation16qam,
    Modulation64qam,
    HighLevelAField,
    WirelessMicrophone,
    AudioMicrophone,
    AudioLowLatencyMicrophone,
    AudioSpeaker,
    AudioHighResolution,
    AudioGamingHeadset,
}

impl DectCapability {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ExtendedFpInfo => "extended FP info",
            Self::DoubleDuplexBearer => "double duplex bearer",
            Self::DoubleSlot => "double slot",
            Self::HalfSlot => "half slot",
            Self::FullSlot => "full slot",
            Self::FrequencyControl => "frequency control",
            Self::PageRepetition => "page repetition",
            Self::CoSetupOnDummy => "C/O setup on dummy",
            Self::ClUplink => "C/L uplink",
            Self::ClDownlink => "C/L downlink",
            Self::BasicAFieldSetup => "basic A-field setup",
            Self::AdvancedAFieldSetup => "advanced A-field setup",
            Self::BFieldSetup => "B-field setup",
            Self::CfMessages => "Cf messages",
            Self::InMinimumDelay => "IN minimum delay",
            Self::InNormalDelay => "IN normal delay",
            Self::IpErrorDetection => "IP error detection",
            Self::IpErrorCorrection => "IP error correction",
            Self::MultibearerConnections => "multibearer connections",
            Self::Adpcm => "ADPCM/G.726",
            Self::GapBasicSpeech => "GAP basic speech",
            Self::NonVoiceCircuitSwitched => "non-voice circuit switched",
            Self::NonVoicePacketSwitched => "non-voice packet switched",
            Self::StandardAuthentication => "standard authentication (DSAA)",
            Self::StandardCiphering => "standard ciphering (DSC)",
            Self::LocationRegistration => "location registration",
            Self::SimServices => "SIM services",
            Self::NonStaticFixedPart => "non-static fixed part",
            Self::CissServices => "CISS services",
            Self::ClmsService => "CLMS service",
            Self::ComsService => "COMS service",
            Self::AccessRightsRequests => "access rights requests",
            Self::ExternalHandover => "external handover",
            Self::ConnectionHandover => "connection handover",
            Self::CrfpEncryption => "CRFP encryption",
            Self::RelayV2 => "relay stations V2",
            Self::ProlongedPreamble => "prolonged preamble",
            Self::FrequencyReplacement => "frequency replacement",
            Self::MacSuspendResume => "MAC suspend and resume",
            Self::IpqService => "IPQ service",
            Self::ExtendedFpInfo2 => "extended FP info part 2",
            Self::FMmsInterworking => "F-MMS interworking",
            Self::BasicOdap => "basic ODAP",
            Self::GmeTransport => "DPRS media encapsulation",
            Self::IpRoaming => "IP roaming",
            Self::Ethernet => "Ethernet",
            Self::TokenRing => "Token Ring",
            Self::Ip => "IP",
            Self::Ppp => "PPP",
            Self::V24 => "V.24",
            Self::RapPart1 => "RAP part 1",
            Self::IsdnIntermediateSystem => "ISDN intermediate system",
            Self::GpsSynchronized => "GPS synchronized",
            Self::TpuiRegistration => "location registration with TPUI",
            Self::EmergencyCall => "emergency call",
            Self::AsymmetricBearers => "asymmetric bearers",
            Self::Lrms => "LRMS",
            Self::DataServiceProfileD => "data service profile D",
            Self::DprsClass3Or4 => "DPRS class 3 or 4",
            Self::DprsClass2 => "DPRS class 2",
            Self::IsdnDataServices => "ISDN data services",
            Self::LongSlot640 => "long slot 640",
            Self::LongSlot672 => "long slot 672",
            Self::EuMux => "E+U-type mux",
            Self::IpfAdvanced => "IPF advanced",
            Self::SipfChannel => "SIPF channel",
            Self::GfChannel => "GF channel",
            Self::UleWrsDelayedPaging => "ULE relay delayed paging",
            Self::ExtendedFpInfo3 => "extended FP info part 3",
            Self::NoEmissionAnyCarrier => "no-emission on any carrier",
            Self::WidebandVoice => "NG-DECT wideband voice",
            Self::PacketData1 => "packet data category 1",
            Self::PacketData2 => "packet data category 2",
            Self::PacketData3 => "packet data category 3",
            Self::PacketData4 => "packet data category 4",
            Self::PacketData5 => "packet data category 5",
            Self::ExtendedWidebandVoice => "extended wideband voice",
            Self::PermanentClir => "permanent CLIR",
            Self::ThirdPartyConference => "third party conference",
            Self::IntrusionCall => "intrusion call",
            Self::CallDeflection => "call deflection",
            Self::MultipleLines => "multiple lines",
            Self::NoEmission => "no-emission mode",
            Self::NgDect5 => "NG-DECT part 5",
            Self::UNemo => "U-NEMo",
            Self::UNemoOpportunistic => "U-NEMo opportunistic transfer",
            Self::UlePhase1 => "ULE phase 1",
            Self::UlePhase1Revised => "ULE phase 1 v1.2",
            Self::UlePhase2 => "ULE phase 2",
            Self::UlePhase3 => "ULE phase 3",
            Self::ReKeying => "re-keying and early encryption",
            Self::Dsaa2 => "DSAA2 authentication",
            Self::Dsc2 => "DSC2 ciphering",
            Self::LightData => "light data services",
            Self::HalfSlotSecondHalf => "half slot setup on second half",
            Self::ModulationBpsk => "BPSK",
            Self::ModulationQpsk => "up to QPSK",
            Self::Modulation8psk => "up to 8PSK",
            Self::Modulation16qam => "up to 16QAM",
            Self::Modulation64qam => "up to 64QAM",
            Self::HighLevelAField => "high level modulation in A-field",
            Self::WirelessMicrophone => "wireless microphone profile 1",
            Self::AudioMicrophone => "advanced audio microphone",
            Self::AudioLowLatencyMicrophone => "advanced audio low latency microphone",
            Self::AudioSpeaker => "advanced audio speaker or headphone",
            Self::AudioHighResolution => "advanced audio high resolution headphone",
            Self::AudioGamingHeadset => "advanced audio gaming headset",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DectIdentity {
    pub rfpi: String,
    pub pari: String,
    pub arc: DectArc,
    pub sari_available: bool,
    pub rpn: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multicell: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emc: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eic: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poc: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gop: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcc: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mnc: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fil: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fpn: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DectSecurity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authentication_supported: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ciphering_supported: Option<bool>,
    pub cipher_state: DectCipherState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cipher_key_index: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_command: Option<String>,
    #[serde(default)]
    pub encryption_events: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dsaa2_supported: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dsc2_supported: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DectVoice {
    pub frames: u32,
    pub playing: bool,
    #[serde(default)]
    pub x_crc_errors: u32,
    #[serde(default)]
    pub unsynced: u32,
    #[serde(default)]
    pub encrypted: u32,
    #[serde(default)]
    pub late: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DectFrame {
    pub side: DectSide,
    pub update: DectUpdate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<DectIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carrier: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carrier_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot_pair: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rf_carriers: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transceivers: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pscn: Option<u8>,
    #[serde(default)]
    pub extended_carriers: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiframe: Option<u32>,
    pub security: DectSecurity,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<DectCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fmid: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pmid: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handsets: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<DectVoice>,
    pub bursts: u32,
    pub level_dbfs: f32,
}

impl DectFrame {
    #[must_use]
    pub fn has(&self, capability: DectCapability) -> bool {
        self.capabilities.contains(&capability)
    }
}

const DECT_CARRIERS: u16 = 10;

impl EventPayload for DectFrame {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            entity: self.identity.as_ref().map(|id| id.rfpi.clone()),
            fields: dect_fields(self),
            ..Presentation::of(dect_summary(self))
        }
    }
}

fn dect_fields(f: &DectFrame) -> Vec<Field> {
    let side = match f.side {
        DectSide::Rfp => "base station",
        DectSide::Pp => "handset",
    };
    let mut shown = fields(&[("side", "Side", text_of(side))]);
    shown.extend(f.identity.as_ref().map(identity_fields).unwrap_or_default());
    shown.extend(fields(&[
        ("carrier", "Carrier", count(f.carrier)),
        ("frequency", "Frequency", number(f.carrier_hz, Unit::Hz, 0)),
        ("slot_pair", "Slot pair", count(f.slot_pair)),
        ("transceivers", "Transceivers", count(f.transceivers)),
        (
            "carriers_available",
            "Carriers available",
            f.rf_carriers.and_then(carriers),
        ),
        ("scan_carrier", "Scan carrier", count(f.pscn)),
        ("multiframe", "Multiframe", count(f.multiframe)),
    ]));
    shown.extend(security_fields(&f.security));
    shown.extend(fields(&[
        (
            "fmid",
            "FMID",
            f.fmid.and_then(|id| text_of(format!("{id:04X}"))),
        ),
        (
            "pmid",
            "PMID",
            f.pmid.and_then(|id| text_of(format!("{id:05X}"))),
        ),
        ("handsets", "Handsets", text(Some(&handsets(f)))),
    ]));
    shown.extend(voice_fields(f));
    shown.extend(fields(&[
        ("bursts", "Bursts", count(Some(f.bursts))),
        ("level", "Level", number(Some(f.level_dbfs), Unit::Dbfs, 1)),
        ("capabilities", "Capabilities", text(Some(&capabilities(f)))),
    ]));
    shown
}

fn identity_fields(id: &DectIdentity) -> Vec<Field> {
    let cell = id
        .multicell
        .map(|multi| if multi { "multi-cell" } else { "single cell" });
    fields(&[
        ("rfpi", "RFPI", text(Some(&id.rfpi))),
        ("access_class", "Access class", text_of(id.arc.label())),
        ("pari", "PARI", text(Some(&id.pari))),
        (
            "manufacturer",
            "Manufacturer",
            id.emc.and_then(|code| text_of(format!("{code:04X}"))),
        ),
        (
            "installer",
            "Installer",
            id.eic.and_then(|code| text_of(format!("{code:04X}"))),
        ),
        (
            "operator",
            "Operator",
            id.poc.and_then(|code| text_of(format!("{code:04X}"))),
        ),
        (
            "gsm_umts_operator",
            "GSM/UMTS operator",
            id.gop.and_then(|code| text_of(format!("{code:05X}"))),
        ),
        ("fixed_part", "Fixed part", count(id.fpn)),
        ("fixed_part_sub", "Fixed part sub", count(id.fps)),
        ("radio_fixed_part", "Radio fixed part", count(Some(id.rpn))),
        ("cell", "Cell", cell.and_then(text_of)),
        ("sari_list", "SARI list", flag(Some(id.sari_available))),
    ])
}

fn security_fields(security: &DectSecurity) -> Vec<Field> {
    fields(&[
        (
            "authentication",
            "Authentication",
            flag(security.authentication_supported),
        ),
        ("ciphering", "Ciphering", flag(security.ciphering_supported)),
        (
            "encryption",
            "Encryption",
            text_of(security.cipher_state.label()),
        ),
        ("dsaa2", "DSAA2", flag(security.dsaa2_supported)),
        ("dsc2", "DSC2", flag(security.dsc2_supported)),
        (
            "last_cipher_command",
            "Last cipher command",
            text(security.last_command.as_deref()),
        ),
        (
            "cipher_key_index",
            "Cipher key index",
            count(security.cipher_key_index),
        ),
    ])
}

fn voice_fields(f: &DectFrame) -> Vec<Field> {
    let Some(voice) = f.voice else {
        return Vec::new();
    };
    let nonzero = |value: u32| count(Some(value).filter(|value| *value > 0));
    fields(&[
        (
            "voice",
            "Voice",
            text_of(if voice.playing { "playing" } else { "muted" }),
        ),
        ("voice_frames", "Voice frames", count(Some(voice.frames))),
        (
            "voice_x_crc_errors",
            "Voice X-CRC errors",
            nonzero(voice.x_crc_errors),
        ),
        (
            "voice_before_sync",
            "Voice before sync",
            nonzero(voice.unsynced),
        ),
        (
            "voice_encrypted",
            "Voice encrypted",
            nonzero(voice.encrypted),
        ),
        ("voice_late", "Voice late", nonzero(voice.late)),
    ])
}

fn carriers(mask: u16) -> Option<FieldValue> {
    let on = (0..DECT_CARRIERS)
        .filter(|carrier| mask >> (DECT_CARRIERS - 1 - carrier) & 1 == 1)
        .map(|carrier| carrier.to_string())
        .collect::<Vec<_>>();
    text(Some(&on.join(", ")))
}

fn handsets(f: &DectFrame) -> String {
    f.handsets
        .iter()
        .map(|id| format!("{id:05X}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn capabilities(f: &DectFrame) -> String {
    f.capabilities
        .iter()
        .map(|capability| capability.label())
        .collect::<Vec<_>>()
        .join("\n")
}

fn yes_no(flag: bool) -> &'static str {
    if flag { "yes" } else { "no" }
}

fn dect_summary(f: &DectFrame) -> String {
    let mut parts = Vec::new();
    if let Some(identity) = &f.identity {
        parts.push(format!("RFPI {}", identity.rfpi));
        parts.push(format!("class {}", identity.arc.label()));
    } else {
        parts.push(f.side.label().to_owned());
    }
    parts.extend(f.carrier.map(|carrier| format!("carrier {carrier}")));
    parts.extend(f.slot_pair.map(|slot| format!("slot {slot}")));
    parts.extend(security_parts(&f.security));
    if f.voice.is_some_and(|voice| voice.playing) {
        parts.push("voice".to_owned());
    }
    joined(parts)
}

fn security_parts(security: &DectSecurity) -> Vec<String> {
    let mut parts = Vec::new();
    if let (Some(auth), Some(cipher)) = (
        security.authentication_supported,
        security.ciphering_supported,
    ) {
        parts.push(format!("auth {} · cipher {}", yes_no(auth), yes_no(cipher)));
    }
    let dsaa2 = security.dsaa2_supported == Some(true);
    let dsc2 = security.dsc2_supported == Some(true);
    if dsaa2 || dsc2 {
        parts.push(format!("DSAA2 {} · DSC2 {}", yes_no(dsaa2), yes_no(dsc2)));
    }
    if security.cipher_state != DectCipherState::Clear {
        parts.push(security.cipher_state.label().to_owned());
    }
    parts
}

pub(crate) fn dect_kind() -> KindSpec {
    KindSpec::of::<DectFrame>("DECT", "Base stations", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station])
        .sighting()
        .view(
            KindView::table(&[
                "rfpi",
                "access_class",
                "carrier",
                "slot_pair",
                "authentication",
                "encryption",
                "handsets",
                "voice",
                "level",
                "bursts",
            ])
            .sorted("carrier", false)
            .keyed("rfpi"),
        )
        .build()
}

impl EventContract for DectFrame {
    const KIND: EventKind = EventKind::named("dect");
}

impl sdrmm_wire::DecoderSettings for DectParams {
    const TYPE_ID: &'static str = "dect";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::ChannelParams;

    #[test]
    fn dect_span_defaults_to_one_carrier_and_older_settings_still_load() {
        let legacy: ChannelParams =
            serde_json::from_str(r#"{"type":"dect","settings":{"band":"us","sides":"rfp"}}"#)
                .unwrap();
        assert_eq!(legacy.type_id(), "dect");
        let params: DectParams = legacy.parse().expect("dect params");
        assert_eq!(params.span, DectSpan::Carrier);
        assert_eq!(params.band, DectBand::Us);
        assert_eq!(serde_json::to_value(DectSpan::Band).unwrap(), "band");
    }

    #[test]
    fn dect_band_centres_and_carrier_lookup_follow_the_band_plan() {
        assert_eq!(DectBand::Eu.center_hz(), 1_889_568_000.0);
        assert_eq!(DectBand::Us.center_hz(), 1_924_992_000.0);
        assert_eq!(DectBand::Eu.carrier_at(1_897_344_000.0), Some(0));
        assert_eq!(DectBand::Eu.carrier_at(1_881_800_000.0), Some(9));
        assert_eq!(DectBand::Eu.carrier_at(1_889_568_000.0), None);
        assert_eq!(DectBand::Us.carrier_at(1_928_448_000.0), Some(4));
        assert_eq!(DectBand::Us.carrier_at(1_897_344_000.0), None);
    }

    #[test]
    fn a_bearer_shows_dsaa2_dsc2_and_voice_counters() {
        let view = serde_json::from_value::<DectFrame>(serde_json::json!({
            "side": "rfp", "update": "voice",
            "capabilities": ["dsaa2", "dsc2", "ule_phase1_revised"],
            "security": {
                "cipher_state": "clear", "encryption_events": 0,
                "dsaa2_supported": true, "dsc2_supported": false,
            },
            "voice": { "frames": 120, "playing": true, "x_crc_errors": 3, "unsynced": 0,
                "encrypted": 0, "late": 0 },
            "extended_carriers": false, "bursts": 140, "level_dbfs": -40.0,
        }))
        .unwrap()
        .present(&EventKind::named("dect"));
        assert_eq!(view.field("DSAA2"), flag(Some(true)).as_ref());
        assert_eq!(view.field("DSC2"), flag(Some(false)).as_ref());
        assert_eq!(view.field("Voice"), text_of("playing").as_ref());
        assert_eq!(view.field("Voice frames"), count(Some(120)).as_ref());
        assert_eq!(view.field("Voice X-CRC errors"), count(Some(3)).as_ref());
        assert_eq!(view.field("Voice before sync"), None);
        assert_eq!(
            view.field("Capabilities"),
            text_of("DSAA2 authentication\nDSC2 ciphering\nULE phase 1 v1.2").as_ref()
        );
    }

    #[test]
    fn a_base_station_names_its_identity_and_carriers() {
        let view = serde_json::from_value::<DectFrame>(serde_json::json!({
            "side": "rfp", "update": "identity",
            "identity": { "rfpi": "0123456789", "pari": "01234567", "arc": "a",
                "sari_available": false, "rpn": 9, "emc": 291 },
            "rf_carriers": 0b10_0000_0001,
            "security": { "cipher_state": "active", "encryption_events": 1 },
            "bursts": 4, "level_dbfs": -61.25,
        }))
        .unwrap()
        .present(&EventKind::named("dect"));
        assert_eq!(view.field("RFPI"), text_of("0123456789").as_ref());
        assert_eq!(
            view.field("Access class"),
            text_of("A residential / small PBX").as_ref()
        );
        assert_eq!(view.field("Manufacturer"), text_of("0123").as_ref());
        assert_eq!(view.field("Carriers available"), text_of("0, 9").as_ref());
        assert_eq!(
            view.field("Encryption"),
            text_of("encryption active").as_ref()
        );
        assert_eq!(
            view.field("Level"),
            number(Some(-61.25_f32), Unit::Dbfs, 1).as_ref()
        );
    }

    #[test]
    fn the_base_station_table_names_fields_a_frame_shows() {
        let frame = DectFrame {
            identity: Some(DectIdentity {
                rfpi: "01594D6FA8".to_owned(),
                ..DectIdentity::default()
            }),
            carrier: Some(3),
            slot_pair: Some(5),
            security: DectSecurity {
                authentication_supported: Some(true),
                ..DectSecurity::default()
            },
            handsets: vec![0x1234],
            voice: Some(DectVoice::default()),
            bursts: 40,
            level_dbfs: -61.0,
            ..DectFrame::default()
        };
        crate::testutil::assert_views_hold(&dect_kind(), &[sdrmm_wire::DecoderEvent::of(&frame)]);
    }
}
