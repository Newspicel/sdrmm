use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{
    ARRAY_LANE_PORT, ARRAY_PORT, AUDIO_PORT, Accept, AudioOrigin, AudioRecorderNode, BASEBAND_PORT,
    BasebandOrigin, BasebandRecorderNode, CONTROL_PORT, ChannelNode, DecoderLogNode, DeviceNode,
    EVENTS_PORT, EventOutputNode, GpsNode, HuntNode, IQ_PORT, IqOrigin, Keeps, NodeCategory,
    POSITION_PORT, Payload, PortRepeat, PortSpec, PortType as P, PortTypeInfo, RecorderNode,
    ScannerNode, SignalMapNode, SpeakerNode, TRANSMIT_PORT, TriangulationNode, TrunkNode,
    VIDEO_PORT, input, output,
};
use crate::{
    aim::AimNode,
    channel::{ChannelDescriptor, DecoderFamily, SettingField},
    control::ControlVerb,
    decode::{Capability, EventKind, EventKindEntry, KindSet},
    device::Direction,
    filter::EventFilterNode,
    monitor::SpectrumMonitorNode,
    network::NetworkExportNode,
    note::NoteNode,
    processor::{ProcessorType, processor_type, processor_types},
    propagation::PropagationNode,
    rotator::RotatorNode,
    satellite::SatelliteNode,
    schedule::ScheduleNode,
    sky_scan::SkyScanNode,
    speech::TranscriberNode,
    stream_out::StreamOutNode,
    transmit::{EncoderNode, MicrophoneNode, TransmitNode},
    voice_chat::VoiceChatNode,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "role", content = "of", rename_all = "snake_case")]
pub enum Role {
    OpensRadio,
    Pass(P),
    Processor,
    Plain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FaceSize {
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Face {
    pub width: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<FaceSize>,
    #[serde(default)]
    pub dial: bool,
}

impl Face {
    #[must_use]
    pub const fn fit(width: u16) -> Self {
        Self {
            width,
            height: None,
            min: None,
            dial: false,
        }
    }

    #[must_use]
    pub const fn sized(width: u16, height: u16) -> Self {
        Self {
            height: Some(height),
            ..Self::fit(width)
        }
    }

    #[must_use]
    pub const fn min(self, width: u16, height: u16) -> Self {
        Self {
            min: Some(FaceSize { width, height }),
            ..self
        }
    }

    #[must_use]
    pub const fn dial(self) -> Self {
        Self { dial: true, ..self }
    }

    #[must_use]
    pub const fn resizable(self) -> bool {
        self.height.is_some()
    }
}

/// The registry a node picks the type it runs from: decoders, encoders or plugin processors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Picks {
    Decoder,
    Encoder,
    Processor,
}

impl Picks {
    const fn of(body: &NodeBody) -> Option<Self> {
        match body {
            NodeBody::Channel(_) => Some(Self::Decoder),
            NodeBody::Encoder(_) => Some(Self::Encoder),
            NodeBody::Processor(_) => Some(Self::Processor),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NodeTypeInfo {
    pub kind: NodeKind,
    pub name: String,
    pub category: NodeCategory,
    #[serde(default)]
    pub summary: String,
    pub role: Role,
    pub face: Face,
    pub ports: Vec<PortSpec>,
    /// Where a node of this kind picks the type it runs, when it is added.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picks: Option<Picks>,
    pub default_body: NodeBody,
    /// The form a face renders for this node's settings, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<SettingField>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PatchCatalog {
    pub nodes: Vec<NodeTypeInfo>,
    pub port_types: Vec<PortTypeInfo>,
    pub event_kinds: Vec<EventKindEntry>,
    #[serde(default)]
    pub feeds: Vec<crate::decode::FeedEntry>,
    /// Array processors, built in and from plugins.
    #[serde(default)]
    pub processors: Vec<ProcessorType>,
    /// Decoder groups in palette order, built in first.
    #[serde(default)]
    pub families: Vec<FamilyEntry>,
}

/// A decoder group and the heading it shows under.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FamilyEntry {
    pub id: DecoderFamily,
    pub title: String,
}

impl From<&DecoderFamily> for FamilyEntry {
    fn from(family: &DecoderFamily) -> Self {
        Self {
            id: family.clone(),
            title: family.title(),
        }
    }
}

impl PatchCatalog {
    #[must_use]
    pub fn build() -> Self {
        Self {
            nodes: NodeKind::ALL.iter().map(|kind| kind.info()).collect(),
            port_types: P::ALL.iter().map(|port_type| port_type.info()).collect(),
            event_kinds: crate::decode::catalog().entries(),
            feeds: crate::decode::catalog().feed_entries(),
            processors: processor_types()
                .iter()
                .map(ProcessorType::resolved)
                .collect(),
            families: DecoderFamily::BUILT_IN
                .iter()
                .map(FamilyEntry::from)
                .collect(),
        }
    }

    /// Adds the groups `decoders` bring beyond the built-in ones, by name.
    #[must_use]
    pub fn with_families_of(mut self, decoders: &[ChannelDescriptor]) -> Self {
        let mut added: Vec<&DecoderFamily> = decoders
            .iter()
            .map(|decoder| &decoder.family)
            .filter(|family| !DecoderFamily::BUILT_IN.contains(family))
            .collect();
        added.sort();
        added.dedup();
        self.families
            .extend(added.into_iter().map(FamilyEntry::from));
        self
    }
}

impl NodeKind {
    #[must_use]
    pub fn info(self) -> NodeTypeInfo {
        let default_body = NodeBody::default_for(self);
        NodeTypeInfo {
            kind: self,
            name: self.name().to_owned(),
            category: self.category(),
            summary: self.summary().to_owned(),
            role: self.role(),
            face: self.face(),
            ports: self.ports(),
            picks: Picks::of(&default_body),
            default_body,
            settings: self.settings(),
        }
    }
}

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

const ANY_IQ: &[IqOrigin] = IqOrigin::ALL;
const ANY_AUDIO: &[AudioOrigin] = AudioOrigin::ALL;
const ANY_BASEBAND: &[BasebandOrigin] = BasebandOrigin::ALL;
const CHANNEL_BASEBAND: &[BasebandOrigin] = &[BasebandOrigin::Channel];
const ENCODED: &[BasebandOrigin] = &[BasebandOrigin::Encoder];
const CHANNEL_AUDIO: &[AudioOrigin] = &[AudioOrigin::Channel];
const VOICE: &[AudioOrigin] = &[AudioOrigin::Voice];
const RADIO_IQ: &[IqOrigin] = &[IqOrigin::Radio];
const CHANNEL_VERBS: &[ControlVerb] =
    &[ControlVerb::Tune, ControlVerb::Doppler, ControlVerb::Sweep];
const SWEEP: &[ControlVerb] = &[ControlVerb::Sweep];
const SATELLITE_VERBS: &[ControlVerb] = &[
    ControlVerb::Tune,
    ControlVerb::Doppler,
    ControlVerb::Point,
    ControlVerb::Run,
];
const RUN: &[ControlVerb] = &[ControlVerb::Run];
const POINT: &[ControlVerb] = &[ControlVerb::Point];
const NETWORK_SOURCE: &str = "source";
const TRANSMIT_SOURCE: &str = "source";
const RUN_TAKE: &str = "Armed, it records only while a schedule or satellite says so";

fn iq_in() -> PortSpec {
    input(IQ_PORT, Payload::iq(ANY_IQ))
}

fn iq_out(origins: &[IqOrigin]) -> PortSpec {
    output(IQ_PORT, Payload::iq(origins))
}

fn position_in() -> PortSpec {
    input(POSITION_PORT, Payload::Position)
}

fn audio_in(origins: &[AudioOrigin]) -> PortSpec {
    input(AUDIO_PORT, Payload::audio(origins)).many()
}

fn audio_out(origins: &[AudioOrigin]) -> PortSpec {
    output(AUDIO_PORT, Payload::audio(origins))
}

fn run_in(note: &'static str) -> PortSpec {
    input(CONTROL_PORT, Payload::control(RUN))
        .many()
        .noted(note)
}

fn events_in(accept: Accept) -> PortSpec {
    input(EVENTS_PORT, Payload::events(accept)).many()
}

fn history_in() -> PortSpec {
    events_in(Accept::Caps(&[Capability::Log])).keeps(Keeps::History)
}

fn events_out(accept: Accept) -> PortSpec {
    output(EVENTS_PORT, Payload::events(accept))
}

macro_rules! kinds {
    ($($kind:ident),+ $(,)?) => {{
        const KINDS: &[EventKind] = &[$(EventKind::$kind),+];
        Accept::Kinds(KINDS)
    }};
}

macro_rules! nodes {
    ($((
        $variant:ident $(($body:ty))? $(| $alias:literal)?,
        $kind:literal,
        $category:ident,
        $name:literal,
        $summary:literal,
        $role:expr,
        $face:expr,
        [$($port:expr),* $(,)?] $(,)?
    )),* $(,)?) => {
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
        #[serde(tag = "kind", content = "data", rename_all = "snake_case")]
        pub enum NodeBody {
            $($(#[serde(alias = $alias)])? $variant $(($body))?,)*
        }

        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ToSchema,
        )]
        #[serde(rename_all = "snake_case")]
        pub enum NodeKind {
            $($(#[serde(alias = $alias)])? $variant,)*
        }

        impl NodeKind {
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];

            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $kind,)*
                }
            }

            #[must_use]
            pub fn parse(text: &str) -> Option<Self> {
                match text {
                    $($kind $(| $alias)? => Some(Self::$variant),)*
                    _ => None,
                }
            }

            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)*
                }
            }

            #[must_use]
            pub const fn summary(self) -> &'static str {
                match self {
                    $(Self::$variant => $summary,)*
                }
            }

            #[must_use]
            pub const fn category(self) -> NodeCategory {
                match self {
                    $(Self::$variant => NodeCategory::$category,)*
                }
            }

            #[must_use]
            pub const fn role(self) -> Role {
                match self {
                    $(Self::$variant => $role,)*
                }
            }

            #[must_use]
            pub const fn face(self) -> Face {
                match self {
                    $(Self::$variant => $face,)*
                }
            }

            fn declared(self) -> Vec<PortSpec> {
                match self {
                    $(Self::$variant => vec![$($port),*],)*
                }
            }
        }

        impl NodeBody {
            #[must_use]
            pub const fn kind(&self) -> NodeKind {
                match self {
                    $(Self::$variant { .. } => NodeKind::$variant,)*
                }
            }

            #[must_use]
            pub fn default_for(kind: NodeKind) -> Self {
                match kind {
                    $(NodeKind::$variant => Self::$variant $((<$body>::default()))?,)*
                }
            }
        }
    };
}

nodes! {
            (Device(DeviceNode), "device", Source, "Device",
                "A radio, a recording or a generator: where a patch starts",
                Role::OpensRadio, Face::fit(320).dial(), [
                iq_out(RADIO_IQ).repeated(PortRepeat::PerRxStream).on_side(Direction::Rx),
                run_in("Opens the radio only while a schedule or satellite says so"),
                input(TRANSMIT_PORT, Payload::Transmit)
                    .on_side(Direction::Tx)
                    .noted("The transmitter this radio sends"),
            ]),
            (Gps(GpsNode), "gps", Source, "GPS position", "Your station location, live or fixed",
                Role::Plain, Face::fit(360), [
                output(POSITION_PORT, Payload::Position),
            ]),
            (Array(crate::array::ArrayNode), "array", Tool, "Array",
                "Radio lanes as one antenna array", Role::Plain, Face::fit(460), [
                input(ARRAY_LANE_PORT, Payload::iq(RADIO_IQ))
                    .repeated(PortRepeat::PerLane)
                    .exclusive()
                    .coherent()
                    .noted("One radio lane per antenna, in antenna order"),
                position_in().noted("Array place and heading"),
                output(ARRAY_PORT, Payload::Array),
            ]),
            (Channel(ChannelNode), "channel", Channel, "Channel", "Tunes and decodes one signal",
                Role::Plain, Face::fit(320).dial(), [
                iq_in().lanes().noted(
                    "every radio that may carry this decoder; it runs on the one that hears it",
                ),
                input(CONTROL_PORT, Payload::control(CHANNEL_VERBS)).noted(
                    "a scanner, signal hunt or satellite drives this decoder; its radio follows",
                ),
                position_in(),
                output(BASEBAND_PORT, Payload::baseband(CHANNEL_BASEBAND)),
                audio_out(CHANNEL_AUDIO),
                events_out(Accept::Any).noted("Decoded messages and calls. Analog calls need squelch"),
                output(VIDEO_PORT, Payload::Video),
            ]),
            (Scope, "scope", Output, "Scope", "Spectrum and waterfall", Role::Plain,
                Face::sized(520, 360).min(320, 200), [
                iq_in(),
            ]),
            (Speaker(SpeakerNode), "speaker", Output, "Speaker", "Plays audio", Role::Plain,
                Face::fit(320), [
                audio_in(ANY_AUDIO),
            ]),
            (Map, "map", Output, "Map", "Decoded positions on a map", Role::Plain,
                Face::sized(520, 380).min(300, 220), [
                events_in(Accept::Caps(&[Capability::Geo])),
                position_in().many(),
            ]),
            (SignalMap(SignalMapNode), "signal_map", Output, "Signal survey",
                "Maps signal strength while you move", Role::Plain,
                Face::sized(600, 440).min(400, 300), [
                iq_in(),
                position_in(),
            ]),
            (Propagation(PropagationNode), "propagation", Output, "Propagation map",
                "Where FT8, FT4 and WSPR signals came from", Role::Plain,
                Face::sized(640, 560).min(440, 380), [
                events_in(Accept::Caps(&[Capability::Spot])).keeps(Keeps::History),
                position_in(),
            ]),
            (Readout, "readout", Output, "Readout", "Current decoder state", Role::Plain,
                Face::sized(560, 320).min(300, 160), [
                events_in(Accept::Any),
            ]),
            (DecoderLog(DecoderLogNode), "decoder_log", Output, "Decoder log",
                "Every decoded message in a table", Role::Plain,
                Face::sized(720, 380).min(360, 200), [
                history_in().keeps(Keeps::Clips),
            ]),
            (SpectrumMonitor(SpectrumMonitorNode), "spectrum_monitor", Tool,
                "Spectrum monitor", "Catches and decodes everything in view", Role::Plain,
                Face::fit(320), [
                iq_in(),
                events_out(Accept::Any),
            ]),
            (Trunk(TrunkNode) | "dmr_trunk", "trunk", Tool, "Trunk system",
                "Follows calls across a trunked radio system", Role::Plain,
                Face::sized(480, 360).min(380, 240), [
                iq_in().noted(
                    "the radio the control channel sits on; the system runs its own decoders",
                ),
                events_out(kinds!(DV, CALL)),
                audio_out(CHANNEL_AUDIO),
            ]),
            (EventFilter(EventFilterNode), "event_filter", Tool, "Event filter",
                "Passes only matching events", Role::Pass(P::Events), Face::fit(380), [
                events_in(Accept::Any),
                events_out(Accept::Any),
            ]),
            (EventOutput(EventOutputNode), "event_output", Output, "Event output",
                "Sends events out, saves call audio", Role::Plain, Face::fit(420), [
                events_in(Accept::Caps(&[Capability::Log])),
            ]),
            (Video, "video", Output, "Video", "ATV frames and decoded pictures", Role::Plain,
                Face::sized(380, 320).min(240, 200), [
                input(VIDEO_PORT, Payload::Video).many().keeps(Keeps::Pictures),
            ]),
            (Recorder(RecorderNode), "recorder", Output, "Recorder",
                "Records a radio's full IQ, with an optional pre-roll",
                Role::Plain, Face::fit(340), [
                iq_in(),
                position_in(),
                run_in(RUN_TAKE),
            ]),
            (AudioRecorder(AudioRecorderNode), "audio_recorder", Output, "Audio recorder",
                "Records audio to WAV", Role::Plain, Face::fit(340), [
                audio_in(ANY_AUDIO),
                run_in(RUN_TAKE),
            ]),
            (BasebandRecorder(BasebandRecorderNode), "baseband_recorder", Output,
                "Baseband recorder",
                "Records one channel's IQ", Role::Plain, Face::fit(340), [
                input(BASEBAND_PORT, Payload::baseband(ANY_BASEBAND)).many(),
                run_in(RUN_TAKE),
            ]),
            (NetworkExport(NetworkExportNode), "network_export", Output, "Network IQ",
                "Streams IQ to other programs", Role::Plain, Face::fit(380), [
                iq_in().group(NETWORK_SOURCE),
                input(BASEBAND_PORT, Payload::baseband(ANY_BASEBAND)).group(NETWORK_SOURCE),
            ]),
            (StreamOut(StreamOutNode), "stream_out", Output, "Stream out",
                "Video for VLC, OBS and other players", Role::Plain, Face::fit(340), [
                input(VIDEO_PORT, Payload::Video),
                input(AUDIO_PORT, Payload::audio(ANY_AUDIO)),
            ]),
            (Export, "export", Output, "Export", "Saves logged rows as CSV or JSON", Role::Plain,
                Face::fit(320), [
                history_in(),
            ]),
            (Scanner(ScannerNode), "scanner", Tool, "Scanner",
                "Steps through frequencies, stops on activity", Role::Plain, Face::fit(400), [
                iq_in().noted("The lane it watches"),
                output(CONTROL_PORT, Payload::control(SWEEP)).one(),
            ]),
            (Hunt(HuntNode), "hunt", Tool, "Signal hunt", "Walks you towards a transmitter",
                Role::Plain, Face::fit(360), [
                iq_in().noted("The lane it watches"),
                output(CONTROL_PORT, Payload::control(SWEEP)).one(),
                position_in().noted("Where you stand and which way you point"),
                events_out(kinds!(DF)).noted("Sweep and mark bearings"),
            ]),
            (Schedule(ScheduleNode), "schedule", Tool, "Schedule",
                "Runs radios and recorders by the clock", Role::Plain, Face::fit(360), [
                output(CONTROL_PORT, Payload::control(RUN))
                    .noted("Radios and recorders to run in its windows"),
            ]),
            (Satellite(SatelliteNode), "satellite", Tool, "Satellite",
                "Predicts passes and follows Doppler", Role::Plain, Face::fit(320).dial(), [
                position_in(),
                output(CONTROL_PORT, Payload::control(SATELLITE_VERBS)).noted(
                    "decoders to tune and Doppler correct, rotators to point, radios to run",
                ),
                events_out(kinds!(SATELLITE))
                    .noted("Position and ground track for a Map"),
            ]),
            (Rotator(RotatorNode), "rotator", Output, "Rotator",
                "Points an antenna", Role::Plain, Face::fit(340), [
                input(CONTROL_PORT, Payload::control(POINT))
                    .many()
                    .noted("Satellites, aims or sky scans. The best pass in view wins"),
            ]),
            (Aim(AimNode), "aim", Tool, "Aim", "Points a rotator at a decoded position",
                Role::Plain, Face::fit(340), [
                events_in(Accept::Caps(&[Capability::Geo])).noted("Positions to follow"),
                position_in().noted("Where the antenna stands"),
                output(CONTROL_PORT, Payload::control(POINT)).noted("Rotators to point"),
            ]),
            (SkyScan(SkyScanNode), "sky_scan", Tool, "Sky scan",
                "Sweeps a rotator and maps signal level", Role::Plain, Face::fit(400), [
                iq_in().noted("The lane it measures"),
                output(CONTROL_PORT, Payload::control(POINT)).one().noted("The rotator to sweep"),
            ]),
            (Processor(crate::processor::ProcessorNode), "processor", Tool, "Processor",
                "Runs an array processor", Role::Processor, Face::fit(400), [
                input(ARRAY_PORT, Payload::Array),
                events_out(Accept::Any),
            ]),
            (Triangulation(TriangulationNode), "triangulation", Tool, "Triangulation",
                "Crosses bearings into a position", Role::Plain, Face::fit(400), [
                events_in(kinds!(DF)).takes("bearings").noted("Bearings"),
                position_in().noted("Vehicle to guide"),
                events_out(kinds!(DF_FIX, NAV)),
            ]),
            (BearingFix, "bearing_fix", Tool, "Bearing fix",
                "Live fix from each station's newest bearing", Role::Plain, Face::fit(320), [
                events_in(Accept::Caps(&[Capability::Bearing]))
                    .takes("bearings")
                    .noted("Bearings with a known origin"),
                events_out(kinds!(BEARING_FIX)),
            ]),
            (Microphone(MicrophoneNode), "microphone", Source, "Microphone",
                "Your voice on the air", Role::Plain, Face::fit(300), [
                audio_out(VOICE),
            ]),
            (Encoder(EncoderNode), "encoder", Encoder, "Encoder",
                "Turns audio or a message into a signal", Role::Plain, Face::fit(320).dial(), [
                audio_in(ANY_AUDIO).noted("Voice for an analog mode"),
                output(BASEBAND_PORT, Payload::baseband(ENCODED)),
            ]),
            (Transmit(TransmitNode), "transmit", Output, "Transmitter",
                "Puts an encoder or a lane on the air", Role::Plain, Face::fit(320).dial(), [
                iq_in().group(TRANSMIT_SOURCE).noted("A lane to send as it is"),
                input(BASEBAND_PORT, Payload::baseband(ANY_BASEBAND)).group(TRANSMIT_SOURCE),
                output(TRANSMIT_PORT, Payload::Transmit).one(),
            ]),
            (Transcriber(TranscriberNode), "transcriber", Tool, "Transcriber",
                "Writes down what calls say", Role::Plain, Face::fit(320), [
                events_in(Accept::Caps(&[Capability::Clip]))
                    .keeps(Keeps::Clips)
                    .noted("Calls and transmissions with their audio"),
                events_out(kinds!(CALL, TRANSMISSION)).noted("The same events with their text"),
            ]),
            (Note(NoteNode), "note", Tool, "Note", "Markdown you leave on the canvas",
                Role::Plain, Face::sized(320, 200).min(200, 120), []),
            (VoiceChat(VoiceChatNode), "voice_chat", Output, "Voice chat",
                "Discord, TeamSpeak or Matrix, both ways", Role::Plain, Face::fit(340), [
                audio_in(ANY_AUDIO),
                audio_out(VOICE),
            ]),
}

impl NodeKind {
    /// The settings form of a node that carries settings of its own.
    #[must_use]
    pub fn settings(self) -> Vec<SettingField> {
        match self {
            Self::Triangulation => crate::fusion::TriangulationParams::form(),
            Self::Aim => crate::aim::AimParams::form(),
            Self::SkyScan => crate::sky_scan::SkyScanParams::form(),
            Self::Transcriber => crate::speech::TranscriberParams::form(),
            _ => Vec::new(),
        }
    }
}

impl NodeBody {
    /// What a refusal calls this node: the processor type it runs, or its kind.
    #[must_use]
    pub fn name(&self) -> String {
        match self {
            Self::Processor(node) => processor_type(&node.processor_type)
                .map_or_else(|| self.kind().name().to_owned(), |kind| kind.name),
            _ => self.kind().name().to_owned(),
        }
    }

    /// The lanes a processor node writes, in port order.
    #[must_use]
    pub fn lane_outputs(&self) -> Vec<String> {
        match self {
            Self::Processor(node) => processor_type(&node.processor_type)
                .map_or_default(|kind| kind.lane_ports().map(str::to_owned).collect()),
            _ => Vec::new(),
        }
    }

    #[must_use]
    pub fn processor_params(&self) -> Option<crate::processor::ProcessorParams> {
        match self {
            Self::Processor(node) => Some(node.params()),
            _ => None,
        }
    }
}

impl NodeBody {
    #[must_use]
    pub const fn category(&self) -> NodeCategory {
        self.kind().category()
    }

    #[must_use]
    pub const fn role(&self) -> Role {
        self.kind().role()
    }

    #[must_use]
    pub const fn face(&self) -> Face {
        self.kind().face()
    }

    #[must_use]
    pub const fn opens_device(&self) -> bool {
        matches!(self.role(), Role::OpensRadio)
    }

    #[must_use]
    pub const fn is_array_processor(&self) -> bool {
        matches!(self.role(), Role::Processor)
    }

    #[must_use]
    pub fn ports(&self, descriptors: Option<&[ChannelDescriptor]>) -> Vec<PortSpec> {
        let declared = self.kind().declared();
        match self {
            Self::Device(device) => narrow_device(declared, device.device.as_ref()),
            Self::Channel(channel) => narrow_channel(
                declared,
                descriptors.and_then(|all| {
                    all.iter()
                        .find(|descriptor| descriptor.type_id == channel.channel_type)
                }),
            ),
            Self::SpectrumMonitor(monitor) => {
                carrying_events(declared, &monitor_kinds(monitor, descriptors))
            }
            Self::Processor(node) => {
                processor_type(&node.processor_type).map_or(declared, |kind| kind.port_specs())
            }
            Self::EventOutput(node) => carrying_events(declared, &node.target.accepts())
                .into_iter()
                .map(|port| match port.port_type {
                    P::Events if node.target.keeps_clips() => port.keeps(Keeps::Clips),
                    _ => port,
                })
                .collect(),
            _ => declared,
        }
    }

    #[must_use]
    pub const fn event_rule(&self) -> Option<&EventFilterNode> {
        match self {
            Self::EventFilter(rule) => Some(rule),
            _ => None,
        }
    }
}

impl NodeKind {
    #[must_use]
    pub fn ports(self) -> Vec<PortSpec> {
        NodeBody::default_for(self).ports(None)
    }
}

#[must_use]
pub fn channel_ports(descriptor: &ChannelDescriptor) -> Vec<PortSpec> {
    narrow_channel(NodeKind::Channel.declared(), Some(descriptor))
}

/// A generator makes its own samples, so its lane is synthetic rather than a radio's. A
/// recording plays back the place it was made at, too. Neither has an antenna to send from.
fn narrow_device(declared: Vec<PortSpec>, pinned: Option<&super::DeviceRef>) -> Vec<PortSpec> {
    let receive_only = |port: &PortSpec| port.port_type != P::Transmit;
    if pinned.is_some_and(super::DeviceRef::is_recording) {
        let mut ports: Vec<PortSpec> = declared.into_iter().filter(receive_only).collect();
        ports.push(output(POSITION_PORT, Payload::Position).noted("Recorded position"));
        return ports;
    }
    if !pinned.is_some_and(super::DeviceRef::is_generator) {
        return declared;
    }
    declared
        .into_iter()
        .filter(receive_only)
        .map(|port| match port.payload {
            Payload::Iq { .. } => port.carrying(Payload::iq(&[IqOrigin::Synthetic])),
            _ => port,
        })
        .collect()
}

fn narrow_channel(
    declared: Vec<PortSpec>,
    descriptor: Option<&ChannelDescriptor>,
) -> Vec<PortSpec> {
    let Some(descriptor) = descriptor else {
        return declared;
    };
    let mut emitted: KindSet = descriptor.emits.iter().cloned().collect();
    if descriptor.calls.is_some() {
        emitted = emitted.with(EventKind::CALL);
    }
    declared
        .into_iter()
        .filter(|port| match port.payload {
            Payload::Position => descriptor.needs_position,
            Payload::Audio { .. } => descriptor.has_audio,
            Payload::Video => descriptor.has_video,
            Payload::Events { .. } => !emitted.is_empty(),
            _ => true,
        })
        .map(|port| match port.payload {
            Payload::Events { .. } => port.carrying(Payload::Events {
                kinds: emitted.clone(),
            }),
            _ => port,
        })
        .collect()
}

/// A plugin processor's events port carries the kinds its type declares, and goes when it
/// declares none. A type not loaded keeps every port, so its wires survive a reload.
fn monitor_kinds(
    monitor: &SpectrumMonitorNode,
    descriptors: Option<&[ChannelDescriptor]>,
) -> KindSet {
    let Some(descriptors) = descriptors else {
        return KindSet::all();
    };
    descriptors
        .iter()
        .filter(|descriptor| descriptor.identifiable && monitor.decodes(&descriptor.type_id))
        .flat_map(|descriptor| descriptor.emits.iter().cloned())
        .chain([EventKind::TRANSMISSION])
        .collect()
}

fn carrying_events(declared: Vec<PortSpec>, kinds: &KindSet) -> Vec<PortSpec> {
    declared
        .into_iter()
        .map(|port| match port.payload {
            Payload::Events { .. } => port.carrying(Payload::Events {
                kinds: kinds.clone(),
            }),
            _ => port,
        })
        .collect()
}
