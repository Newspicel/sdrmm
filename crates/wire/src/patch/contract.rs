use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    control::ControlVerb,
    decode::{Capability, EventKind, KindSet},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PortType {
    Iq,
    Baseband,
    Audio,
    Events,
    Video,
    Control,
    Position,
    Array,
    Bearing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Hue {
    Accent,
    Muted,
    Blue,
    Cyan,
    Teal,
    Green,
    Amber,
    Orange,
    Violet,
    Indigo,
    Pink,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PortTypeInfo {
    pub port_type: PortType,
    pub name: String,
    pub hue: Hue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl PortType {
    pub const ALL: &'static [Self] = &[
        Self::Iq,
        Self::Baseband,
        Self::Audio,
        Self::Events,
        Self::Video,
        Self::Control,
        Self::Position,
        Self::Array,
        Self::Bearing,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Iq => "iq",
            Self::Baseband => "baseband",
            Self::Audio => "audio",
            Self::Events => "events",
            Self::Video => "video",
            Self::Control => "control",
            Self::Position => "position",
            Self::Array => "array",
            Self::Bearing => "bearing",
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Iq => "IQ",
            Self::Baseband => "Baseband",
            Self::Audio => "Audio",
            Self::Events => "Events",
            Self::Video => "Video",
            Self::Control => "Control",
            Self::Position => "Position",
            Self::Array => "Array",
            Self::Bearing => "Bearing",
        }
    }

    #[must_use]
    pub const fn hue(self) -> Hue {
        match self {
            Self::Iq => Hue::Blue,
            Self::Baseband => Hue::Cyan,
            Self::Audio => Hue::Green,
            Self::Events => Hue::Amber,
            Self::Video => Hue::Orange,
            Self::Control => Hue::Violet,
            Self::Position => Hue::Accent,
            Self::Array => Hue::Indigo,
            Self::Bearing => Hue::Teal,
        }
    }

    #[must_use]
    pub const fn note(self) -> Option<&'static str> {
        match self {
            Self::Iq => Some("Decoders on a lane retune it unless the lane is locked"),
            Self::Bearing => Some("A live bearing that steers an array"),
            Self::Audio => Some("Live audio"),
            Self::Events => Some("Decoded events. Calls and transmissions bring their clip"),
            Self::Baseband => Some("One channel, filtered and decimated"),
            Self::Video => Some("Decoded video frames and pictures"),
            Self::Control => Some("Commands that retune a decoder, point a rotator or run a radio"),
            Self::Position => Some("A live fix and the heading it comes with"),
            Self::Array => Some("One lane per antenna element"),
        }
    }

    #[must_use]
    pub fn info(self) -> PortTypeInfo {
        PortTypeInfo {
            port_type: self,
            name: self.name().to_owned(),
            hue: self.hue(),
            note: self.note().map(str::to_owned),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IqOrigin {
    Radio,
    Synthetic,
    Derived,
}

impl IqOrigin {
    pub const ALL: &'static [Self] = &[Self::Radio, Self::Synthetic, Self::Derived];
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Payload {
    Iq { origins: Vec<IqOrigin> },
    Baseband,
    Audio,
    Events { kinds: KindSet },
    Video,
    Control { verbs: Vec<ControlVerb> },
    Position,
    Array,
    Bearing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accept {
    Any,
    Caps(&'static [Capability]),
    Kinds(&'static [EventKind]),
}

impl Accept {
    #[must_use]
    pub fn kinds(self) -> KindSet {
        match self {
            Self::Any => KindSet::all(),
            Self::Caps(caps) => EventKind::ALL
                .iter()
                .copied()
                .filter(|kind| caps.iter().all(|cap| kind.has(*cap)))
                .collect(),
            Self::Kinds(kinds) => kinds.iter().copied().collect(),
        }
    }
}

impl Payload {
    #[must_use]
    pub fn iq(origins: &[IqOrigin]) -> Self {
        Self::Iq {
            origins: origins.to_vec(),
        }
    }

    #[must_use]
    pub fn events(accept: Accept) -> Self {
        Self::Events {
            kinds: accept.kinds(),
        }
    }

    #[must_use]
    pub fn control(verbs: &[ControlVerb]) -> Self {
        Self::Control {
            verbs: verbs.to_vec(),
        }
    }

    #[must_use]
    pub const fn port_type(&self) -> PortType {
        match self {
            Self::Iq { .. } => PortType::Iq,
            Self::Baseband => PortType::Baseband,
            Self::Audio => PortType::Audio,
            Self::Events { .. } => PortType::Events,
            Self::Video => PortType::Video,
            Self::Control { .. } => PortType::Control,
            Self::Position => PortType::Position,
            Self::Array => PortType::Array,
            Self::Bearing => PortType::Bearing,
        }
    }

    #[must_use]
    pub fn kinds(&self) -> KindSet {
        match self {
            Self::Events { kinds } => *kinds,
            _ => KindSet::EMPTY,
        }
    }

    #[must_use]
    pub fn admits(&self, offered: &Self) -> bool {
        match (self, offered) {
            (Self::Iq { origins }, Self::Iq { origins: offered }) => {
                offered.iter().all(|origin| origins.contains(origin))
            }
            (Self::Events { kinds }, Self::Events { kinds: offered }) => {
                offered.is_empty() || !kinds.intersection(*offered).is_empty()
            }
            (Self::Control { verbs }, Self::Control { verbs: offered }) => {
                offered.iter().any(|verb| verbs.contains(verb))
            }
            (accepted, offered) => accepted.port_type() == offered.port_type(),
        }
    }

    #[must_use]
    pub fn takes(&self) -> Option<String> {
        match self {
            Self::Iq { origins } => match origins.as_slice() {
                [IqOrigin::Radio] => Some("a radio lane".to_owned()),
                _ => None,
            },
            Self::Events { kinds } => kinds_noun(*kinds),
            Self::Control { verbs } => match verbs.as_slice() {
                [verb] => Some(verb.noun().to_owned()),
                _ => None,
            },
            Self::Baseband
            | Self::Audio
            | Self::Video
            | Self::Position
            | Self::Array
            | Self::Bearing => None,
        }
    }
}

const CAP_NOUNS: &[(Capability, &str)] = &[
    (Capability::Geo, "positions"),
    (Capability::Spot, "grid spots"),
    (Capability::Bearing, "bearings"),
    (Capability::Clip, "calls and transmissions"),
    (Capability::Picture, "pictures"),
    (Capability::Text, "text"),
    (Capability::Voice, "voice"),
];

fn kinds_noun(kinds: KindSet) -> Option<String> {
    if kinds == KindSet::all() {
        return None;
    }
    if let Some((_, noun)) = CAP_NOUNS
        .iter()
        .find(|(cap, _)| KindSet::with_cap(*cap) == kinds)
    {
        return Some((*noun).to_owned());
    }
    let names: Vec<&str> = kinds.iter().map(EventKind::name).collect();
    Some(names.join(", "))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Keeps {
    History,
    Clips,
    Pictures,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Multi {
    #[default]
    One,
    Many,
    ManyLanes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PortDirection {
    In,
    Out,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PortRepeat {
    #[default]
    Once,
    PerRxStream,
    PerLane,
}

impl PortRepeat {
    #[must_use]
    pub const fn limit(self) -> u32 {
        match self {
            Self::Once => 1,
            Self::PerRxStream => super::MAX_STREAMS,
            Self::PerLane => crate::array::MAX_ARRAY_LANES,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PortSpec {
    pub name: String,
    pub port_type: PortType,
    pub direction: PortDirection,
    pub payload: Payload,
    pub multi: Multi,
    #[serde(default, skip_serializing_if = "is_once")]
    pub repeat: PortRepeat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<u32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exclusive: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub coherent: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub direct: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keeps: Vec<Keeps>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub takes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

fn is_once(repeat: &PortRepeat) -> bool {
    *repeat == PortRepeat::Once
}

#[must_use]
pub fn input(name: &str, payload: Payload) -> PortSpec {
    port(name, PortDirection::In, payload, Multi::One)
}

#[must_use]
pub fn output(name: &str, payload: Payload) -> PortSpec {
    port(name, PortDirection::Out, payload, Multi::Many)
}

fn port(name: &str, direction: PortDirection, payload: Payload, multi: Multi) -> PortSpec {
    PortSpec {
        name: name.to_owned(),
        port_type: payload.port_type(),
        direction,
        takes: (direction == PortDirection::In)
            .then(|| payload.takes())
            .flatten(),
        payload,
        multi,
        repeat: PortRepeat::Once,
        stream: None,
        exclusive: false,
        coherent: false,
        direct: false,
        keeps: Vec::new(),
        group: None,
        note: None,
    }
}

impl PortSpec {
    #[must_use]
    pub fn many(self) -> Self {
        Self {
            multi: Multi::Many,
            ..self
        }
    }

    #[must_use]
    pub fn one(self) -> Self {
        Self {
            multi: Multi::One,
            ..self
        }
    }

    #[must_use]
    pub fn lanes(self) -> Self {
        Self {
            multi: Multi::ManyLanes,
            ..self
        }
    }

    #[must_use]
    pub fn exclusive(self) -> Self {
        Self {
            exclusive: true,
            ..self
        }
    }

    #[must_use]
    pub fn coherent(self) -> Self {
        Self {
            coherent: true,
            ..self
        }
    }

    #[must_use]
    pub fn direct(self) -> Self {
        Self {
            direct: true,
            ..self
        }
    }

    #[must_use]
    pub fn keeps(mut self, keeps: Keeps) -> Self {
        if !self.keeps.contains(&keeps) {
            self.keeps.push(keeps);
        }
        self
    }

    #[must_use]
    pub fn group(self, group: &str) -> Self {
        Self {
            group: Some(group.to_owned()),
            ..self
        }
    }

    #[must_use]
    pub fn takes(self, takes: &str) -> Self {
        Self {
            takes: Some(takes.to_owned()),
            ..self
        }
    }

    #[must_use]
    pub fn noted(self, note: &str) -> Self {
        Self {
            note: Some(note.to_owned()),
            ..self
        }
    }

    #[must_use]
    pub fn repeated(self, repeat: PortRepeat) -> Self {
        Self { repeat, ..self }
    }

    #[must_use]
    pub fn carrying(self, payload: Payload) -> Self {
        Self {
            takes: (self.direction == PortDirection::In)
                .then(|| payload.takes())
                .flatten(),
            payload,
            ..self
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "refusal", rename_all = "snake_case")]
pub enum Refusal {
    OneWire,
    LanesOnly,
    OneTarget,
    LaneTaken { holder: String },
    SharedClock,
    Direct,
    Group,
    Takes { node: String, what: String },
}

pub const REFUSALS: [(&str, &str); 8] = [
    ("one_wire", "That input takes one wire"),
    ("lanes_only", "Only radio lanes share an input"),
    ("one_target", "That output drives one node"),
    ("lane_taken", "That lane is in {holder}"),
    ("shared_clock", "This radio's lanes share no clock"),
    ("direct", "Wire it straight from the source"),
    ("group", "Takes one source"),
    ("takes", "{node} takes {what}"),
];

impl Refusal {
    #[must_use]
    pub const fn key(&self) -> &'static str {
        match self {
            Self::OneWire => "one_wire",
            Self::LanesOnly => "lanes_only",
            Self::OneTarget => "one_target",
            Self::LaneTaken { .. } => "lane_taken",
            Self::SharedClock => "shared_clock",
            Self::Direct => "direct",
            Self::Group => "group",
            Self::Takes { .. } => "takes",
        }
    }

    #[must_use]
    pub fn text(&self) -> String {
        let template = REFUSALS
            .iter()
            .find(|(key, _)| *key == self.key())
            .map_or("", |(_, template)| template);
        match self {
            Self::LaneTaken { holder } => template.replace("{holder}", holder),
            Self::Takes { node, what } => template.replace("{node}", node).replace("{what}", what),
            Self::OneWire
            | Self::LanesOnly
            | Self::OneTarget
            | Self::SharedClock
            | Self::Direct
            | Self::Group => template.to_owned(),
        }
    }
}
