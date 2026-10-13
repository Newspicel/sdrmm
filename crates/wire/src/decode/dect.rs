use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
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
