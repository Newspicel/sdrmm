use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{
    ARRAY_LANE_PORT, ARRAY_PORT, AUDIO_PORT, Accept, AudioRecorderNode, BASEBAND_PORT, BEAM_PORT,
    BEARING_PORT, BasebandRecorderNode, CONTROL_PORT, ChannelNode, DecoderLogNode, DeviceNode,
    DmrTrunkNode, EVENTS_PORT, EventOutputNode, GpsNode, HuntNode, IQ_PORT, IqOrigin, Keeps,
    NodeCategory, POSITION_PORT, Payload, PortRepeat, PortSpec, PortType as P, PortTypeInfo,
    RADAR_TRUTH_PORT, RADAR_TX_PORT, RecorderNode, STEER_PORT, STITCH_WIDE_PORT, ScannerNode,
    SignalMapNode, TriangulationNode, VIDEO_PORT, input, output,
};
use crate::{
    channel::ChannelDescriptor,
    control::ControlVerb,
    decode::{Capability, EventKind, EventKindEntry, KindSet},
    filter::EventFilterNode,
    monitor::SpectrumMonitorNode,
    network::NetworkExportNode,
    note::NoteNode,
    processor::processors,
    propagation::PropagationNode,
    rotator::RotatorNode,
    satellite::SatelliteNode,
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
    #[serde(default)]
    pub needs_channel_type: bool,
    pub default_body: NodeBody,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PatchCatalog {
    pub nodes: Vec<NodeTypeInfo>,
    pub port_types: Vec<PortTypeInfo>,
    pub event_kinds: Vec<EventKindEntry>,
}

impl PatchCatalog {
    #[must_use]
    pub fn build() -> Self {
        Self {
            nodes: NodeKind::ALL.iter().map(|kind| kind.info()).collect(),
            port_types: P::ALL.iter().map(|port_type| port_type.info()).collect(),
            event_kinds: EventKind::entries(),
        }
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
            needs_channel_type: matches!(default_body, NodeBody::Channel(_)),
            default_body,
        }
    }
}

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

const ANY_IQ: &[IqOrigin] = IqOrigin::ALL;
const RADIO_IQ: &[IqOrigin] = &[IqOrigin::Radio];
const DERIVED_IQ: &[IqOrigin] = &[IqOrigin::Derived];
const CHANNEL_VERBS: &[ControlVerb] =
    &[ControlVerb::Tune, ControlVerb::Doppler, ControlVerb::Sweep];
const SWEEP: &[ControlVerb] = &[ControlVerb::Sweep];
const SATELLITE_VERBS: &[ControlVerb] =
    &[ControlVerb::Tune, ControlVerb::Doppler, ControlVerb::Point];
const POINT: &[ControlVerb] = &[ControlVerb::Point];
const NETWORK_SOURCE: &str = "source";

fn iq_in() -> PortSpec {
    input(IQ_PORT, Payload::iq(ANY_IQ))
}

fn iq_out(origins: &[IqOrigin]) -> PortSpec {
    output(IQ_PORT, Payload::iq(origins))
}

fn position_in() -> PortSpec {
    input(POSITION_PORT, Payload::Position)
}

fn audio_in() -> PortSpec {
    input(AUDIO_PORT, Payload::Audio).many()
}

fn audio_out() -> PortSpec {
    output(AUDIO_PORT, Payload::Audio)
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

macro_rules! port_row {
    (array_in) => {
        input(ARRAY_PORT, Payload::Array)
    };
    (events_out($note:literal $(, $kind:ident)*)) => {
        events_out(Accept::Kinds(&[$(EventKind::$kind),*])).noted($note)
    };
    (bearing_out($note:literal)) => {
        output(BEARING_PORT, Payload::Bearing).noted($note)
    };
    (steer_in($note:literal)) => {
        input(STEER_PORT, Payload::Bearing).noted($note)
    };
    (beam_out($note:literal)) => {
        output(BEAM_PORT, Payload::iq(DERIVED_IQ)).noted($note)
    };
    (wide_out($note:literal)) => {
        output(STITCH_WIDE_PORT, Payload::iq(DERIVED_IQ)).noted($note)
    };
    (tx_in($note:literal)) => {
        input(RADAR_TX_PORT, Payload::Position).noted($note)
    };
    (adsb_in($note:literal)) => {
        input(RADAR_TRUTH_PORT, Payload::events(Accept::Kinds(&[EventKind::Adsb])))
            .many()
            .takes("ADS-B")
            .noted($note)
    };
}

macro_rules! lane_outputs {
    ([$($found:expr),*]) => {
        &[$($found),*]
    };
    ([$($found:expr),*] beam_out $(($($arg:tt)*))? $(, $($rest:tt)*)?) => {
        lane_outputs!([$($found,)* BEAM_PORT] $($($rest)*)?)
    };
    ([$($found:expr),*] wide_out $(($($arg:tt)*))? $(, $($rest:tt)*)?) => {
        lane_outputs!([$($found,)* STITCH_WIDE_PORT] $($($rest)*)?)
    };
    ([$($found:expr),*] $skip:ident $(($($arg:tt)*))? $(, $($rest:tt)*)?) => {
        lane_outputs!([$($found),*] $($($rest)*)?)
    };
}

macro_rules! nodes {
    ($((
        $variant:ident $(($body:ty))?,
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
            $($variant $(($body))?,)*
        }

        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ToSchema,
        )]
        #[serde(rename_all = "snake_case")]
        pub enum NodeKind {
            $($variant,)*
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
                    $($kind => Some(Self::$variant),)*
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

macro_rules! node_registry {
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
            [$($port:ident $(($($arg:tt)*))?),* $(,)?]
        )),* $(,)?],
        probe: ($probe:ident, $probe_id:literal, $probe_params:ty) $(,)?
    ) => {
        nodes! {
            (Device(DeviceNode), "device", Source, "Device",
                "A radio, a recording or a generator: where a patch starts",
                Role::OpensRadio, Face::fit(420).dial(), [
                iq_out(RADIO_IQ).repeated(PortRepeat::PerRxStream),
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
                Role::Plain, Face::fit(440).dial(), [
                iq_in().lanes().noted(
                    "every radio that may carry this decoder; it runs on the one that hears it",
                ),
                input(CONTROL_PORT, Payload::control(CHANNEL_VERBS)).noted(
                    "a scanner, signal hunt or satellite drives this decoder; its radio follows",
                ),
                position_in(),
                output(BASEBAND_PORT, Payload::Baseband),
                audio_out(),
                events_out(Accept::Any).noted("Decoded messages and calls. Analog calls need squelch"),
                output(VIDEO_PORT, Payload::Video),
            ]),
            (Scope, "scope", Output, "Scope", "Spectrum and waterfall", Role::Plain,
                Face::sized(520, 360).min(320, 200), [
                iq_in(),
            ]),
            (BasebandScope, "baseband_scope", Output, "Baseband scope",
                "Constellation and eye of one channel", Role::Plain,
                Face::sized(420, 340).min(320, 200), [
                input(BASEBAND_PORT, Payload::Baseband),
            ]),
            (Speaker, "speaker", Output, "Speaker", "Plays channel audio", Role::Plain,
                Face::fit(320), [
                audio_in(),
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
            (DmrTrunk(DmrTrunkNode), "dmr_trunk", Tool, "DMR trunk system",
                "Follows calls across a DMR trunk system", Role::Plain,
                Face::sized(480, 360).min(380, 240), [
                iq_in().noted(
                    "the radio the control channel sits on; the system runs its own decoders",
                ),
                events_out(Accept::Kinds(&[EventKind::Dv, EventKind::Call])),
                audio_out(),
            ]),
            (EventFilter(EventFilterNode), "event_filter", Tool, "Event filter",
                "Passes only matching events", Role::Pass(P::Events), Face::fit(380), [
                events_in(Accept::Any),
                events_out(Accept::Any),
            ]),
            (AudioFx(crate::AudioFxNode), "audio_fx", Tool, "Audio FX",
                "Filters, denoise and AGC", Role::Pass(P::Audio), Face::fit(400), [
                audio_in(),
                audio_out(),
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
            ]),
            (AudioRecorder(AudioRecorderNode), "audio_recorder", Output, "Audio recorder",
                "Records channel audio to WAV", Role::Plain, Face::fit(340), [
                audio_in(),
            ]),
            (BasebandRecorder(BasebandRecorderNode), "baseband_recorder", Output,
                "Baseband recorder",
                "Records one channel's IQ", Role::Plain, Face::fit(340), [
                input(BASEBAND_PORT, Payload::Baseband).many(),
            ]),
            (NetworkExport(NetworkExportNode), "network_export", Output, "Network IQ",
                "Streams IQ to other programs", Role::Plain, Face::fit(380), [
                iq_in().group(NETWORK_SOURCE),
                input(BASEBAND_PORT, Payload::Baseband).group(NETWORK_SOURCE),
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
                events_out(Accept::Kinds(&[EventKind::Df])).noted("Sweep and mark bearings"),
            ]),
            (Satellite(SatelliteNode), "satellite", Tool, "Satellite",
                "Predicts passes and follows Doppler", Role::Plain, Face::fit(420).dial(), [
                position_in(),
                output(CONTROL_PORT, Payload::control(SATELLITE_VERBS))
                    .noted("decoders to tune and Doppler correct, rotators to point"),
                events_out(Accept::Kinds(&[EventKind::Satellite]))
                    .noted("Position and ground track for a Map"),
            ]),
            (Rotator(RotatorNode), "rotator", Output, "Rotator",
                "Points an antenna at a satellite", Role::Plain, Face::fit(340), [
                input(CONTROL_PORT, Payload::control(POINT)).noted("The satellite to follow"),
            ]),
            $(
                ($variant($node), $type_id, Tool, $name, $summary, Role::Processor, $face, [
                    $(port_row!($port $(($($arg)*))?)),*
                ]),
            )*
            (Triangulation(TriangulationNode), "triangulation", Tool, "Triangulation",
                "Crosses bearings into a position", Role::Plain, Face::fit(400), [
                events_in(Accept::Kinds(&[EventKind::Df])).takes("bearings").noted("Bearings"),
                position_in().noted("Vehicle to guide"),
                events_out(Accept::Kinds(&[EventKind::DfFix, EventKind::Nav])),
            ]),
            (Note(NoteNode), "note", Tool, "Note", "Markdown you leave on the canvas",
                Role::Plain, Face::sized(320, 200).min(200, 120), []),
        }
    };
}

macro_rules! processor_nodes {
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
        $(
            #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
            pub struct $node {
                #[serde(default)]
                pub settings: $params,
            }
        )*

        impl NodeBody {
            #[must_use]
            pub const fn lane_outputs(&self) -> &'static [&'static str] {
                match self {
                    $(Self::$variant(_) => lane_outputs!([] $($port)*),)*
                    _ => &[],
                }
            }

            #[must_use]
            pub fn processor_params(&self) -> Option<crate::processor::ProcessorParams> {
                match self {
                    $(
                        Self::$variant(node) => Some(crate::processor::ProcessorParams::$variant(
                            Clone::clone(&node.settings),
                        )),
                    )*
                    _ => None,
                }
            }
        }
    };
}

processors!(processor_nodes);
processors!(node_registry);

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
                carrying_events(declared, monitor_kinds(monitor, descriptors))
            }
            Self::EventOutput(node) => carrying_events(declared, node.target.accepts())
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
/// recording plays back the place it was made at, too.
fn narrow_device(declared: Vec<PortSpec>, pinned: Option<&super::DeviceRef>) -> Vec<PortSpec> {
    if pinned.is_some_and(super::DeviceRef::is_recording) {
        let mut ports = declared;
        ports.push(output(POSITION_PORT, Payload::Position).noted("Recorded position"));
        return ports;
    }
    if !pinned.is_some_and(super::DeviceRef::is_generator) {
        return declared;
    }
    declared
        .into_iter()
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
    let mut emitted: KindSet = descriptor.emits.iter().copied().collect();
    if descriptor.calls.is_some() {
        emitted = emitted.with(EventKind::Call);
    }
    declared
        .into_iter()
        .filter(|port| match port.payload {
            Payload::Position => descriptor.needs_position,
            Payload::Audio => descriptor.has_audio,
            Payload::Video => descriptor.has_video,
            Payload::Events { .. } => !emitted.is_empty(),
            _ => true,
        })
        .map(|port| match port.payload {
            Payload::Events { .. } => port.carrying(Payload::Events { kinds: emitted }),
            _ => port,
        })
        .collect()
}

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
        .flat_map(|descriptor| descriptor.emits.iter().copied())
        .chain([EventKind::Transmission])
        .collect()
}

fn carrying_events(declared: Vec<PortSpec>, kinds: KindSet) -> Vec<PortSpec> {
    declared
        .into_iter()
        .map(|port| match port.payload {
            Payload::Events { .. } => port.carrying(Payload::Events { kinds }),
            _ => port,
        })
        .collect()
}
