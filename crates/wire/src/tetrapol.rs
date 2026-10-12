use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    decode::EventKind,
    present::{
        EventPayload, Field, Presentation, Unit, count, fields, flag, number, only_if, text,
        text_of,
    },
};

pub const TETRAPOL_BANDWIDTH_HZ: f64 = 12_500.0;
pub const MAX_TETRAPOL_SCRAMBLING: u8 = 127;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TetrapolBand {
    #[default]
    Uhf,
    Vhf,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TetrapolParams {
    #[serde(default)]
    pub band: TetrapolBand,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scrambling: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TetrapolLogicalChannel {
    Broadcast,
    Paging,
    Signalling,
    Traffic,
    Unknown,
}

impl TetrapolLogicalChannel {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Broadcast => "BCH",
            Self::Paging => "PCH",
            Self::Signalling => "SDCH",
            Self::Traffic => "TCH",
            Self::Unknown => "CH",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TetrapolAddressKind {
    Terminal,
    Random,
    Group,
    Object,
    AllStations,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TetrapolAddress {
    pub kind: TetrapolAddressKind,
    pub id: u16,
}

impl TetrapolAddress {
    #[must_use]
    pub fn label(self) -> String {
        match self.kind {
            TetrapolAddressKind::Terminal => format!("TTI {:04X}", self.id),
            TetrapolAddressKind::Random => format!("RTI {}", self.id),
            TetrapolAddressKind::Group => format!("CGI {}", self.id),
            TetrapolAddressKind::Object => format!("COI {}", self.id),
            TetrapolAddressKind::AllStations => "all".to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TetrapolCallType {
    OpenChannel,
    Internal,
    Broadcast,
    External,
}

impl TetrapolCallType {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::OpenChannel => "Open channel",
            Self::Internal => "Internal",
            Self::Broadcast => "Broadcast",
            Self::External => "External",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TetrapolKey {
    pub kind: u8,
    pub index: u8,
}

impl TetrapolKey {
    #[must_use]
    pub fn clear(self) -> bool {
        self.kind == 0 && self.index == 0 || self.kind == 15 && self.index == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TetrapolCell {
    pub bn: u8,
    pub rsw: u8,
    pub bs: u8,
}

impl TetrapolCell {
    #[must_use]
    pub fn label(&self) -> String {
        format!("{}.{}.{}", self.bn, self.rsw, self.bs)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TetrapolSystemInfo {
    pub cell: TetrapolCell,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location_area: Option<u8>,
    pub uplink_scrambling: u8,
    pub mode: u8,
    pub ciphered: bool,
    pub home_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superframe: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_channel: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TetrapolGroupCall {
    pub group: u16,
    pub call_type: TetrapolCallType,
    pub coverage: u8,
    pub key: TetrapolKey,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downlink_scrambling: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uplink_scrambling: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "message", rename_all = "snake_case")]
pub enum TetrapolPayload {
    SystemInfo(TetrapolSystemInfo),
    GroupActivation(TetrapolGroupCall),
    GroupPaging(TetrapolGroupCall),
    GroupIdle { cause: u8 },
    GroupEnd { cause: u8 },
    Paging { terminals: Vec<TetrapolAddress> },
    Voice { seconds: f32 },
    Other { codop: u8, name: String },
}

impl TetrapolPayload {
    #[must_use]
    pub fn summary(&self) -> String {
        match self {
            Self::SystemInfo(info) => {
                let mut parts = vec![format!("cell {}", info.cell.label())];
                if let Some(country) = info.country {
                    parts.push(format!("country {country}"));
                }
                if let Some(network) = info.network {
                    parts.push(format!("network {network}"));
                }
                if info.ciphered {
                    parts.push("ciphered".to_owned());
                }
                parts.join(" · ")
            }
            Self::GroupActivation(call) => {
                format!("group {} active{}", call.group, call_tail(call))
            }
            Self::GroupPaging(call) => format!("group {} paged{}", call.group, call_tail(call)),
            Self::GroupIdle { .. } => "group idle".to_owned(),
            Self::GroupEnd { cause } => format!("group end, cause {cause:#04x}"),
            Self::Paging { terminals } => format!(
                "paging {}",
                terminals
                    .iter()
                    .map(|address| address.label())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Voice { seconds } => format!("voice {seconds:.1} s"),
            Self::Other { name, .. } => name.clone(),
        }
    }
}

fn call_tail(call: &TetrapolGroupCall) -> String {
    let channel = call
        .channel
        .map(|channel| format!(" on channel {channel}"))
        .unwrap_or_default();
    let key = if call.key.clear() { "" } else { ", encrypted" };
    format!("{channel}{key}")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TetrapolMessage {
    pub channel: TetrapolLogicalChannel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<u8>,
    pub scrambling: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<TetrapolAddress>,
    pub payload: TetrapolPayload,
    pub data: String,
    pub rejected: u32,
}

impl TetrapolMessage {
    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts = vec![self.channel.label().to_owned()];
        if let Some(address) = self.address {
            parts.push(address.label());
        }
        parts.push(self.payload.summary());
        parts.join(" · ")
    }

    #[must_use]
    pub fn station(&self) -> Option<String> {
        match &self.payload {
            TetrapolPayload::SystemInfo(info) => Some(info.cell.label()),
            TetrapolPayload::GroupActivation(call) | TetrapolPayload::GroupPaging(call) => {
                Some(format!("group {}", call.group))
            }
            _ => self.address.map(TetrapolAddress::label),
        }
    }
}

impl EventPayload for TetrapolMessage {
    fn present(&self, _kind: EventKind) -> Presentation {
        Presentation {
            entity: self.station(),
            fields: tetrapol_fields(self),
            ..Presentation::of(self.summary())
        }
    }
}

fn tetrapol_fields(m: &TetrapolMessage) -> Vec<Field> {
    let name = match &m.payload {
        TetrapolPayload::Other { name, .. } => Some(name.as_str()),
        _ => None,
    };
    let mut shown = fields(&[
        ("Channel", text_of(m.channel.label())),
        ("Frame", count(m.frame)),
        ("Scrambling", count(Some(m.scrambling))),
        (
            "Address",
            m.address.and_then(|address| text_of(address.label())),
        ),
        ("Message", text(name)),
    ]);
    shown.extend(payload_fields(&m.payload));
    shown.extend(fields(&[
        (
            "Rejected",
            count(Some(m.rejected).filter(|rejected| *rejected > 0)),
        ),
        ("Data", text(Some(&m.data))),
    ]));
    shown
}

fn payload_fields(payload: &TetrapolPayload) -> Vec<Field> {
    match payload {
        TetrapolPayload::SystemInfo(info) => system_info_fields(info),
        TetrapolPayload::GroupActivation(call) | TetrapolPayload::GroupPaging(call) => {
            group_call_fields(call)
        }
        TetrapolPayload::GroupIdle { cause } | TetrapolPayload::GroupEnd { cause } => {
            fields(&[("Cause", count(Some(*cause)))])
        }
        TetrapolPayload::Paging { terminals } => {
            let terminals = terminals
                .iter()
                .map(|address| address.label())
                .collect::<Vec<_>>()
                .join(", ");
            fields(&[("Terminals", text(Some(&terminals)))])
        }
        TetrapolPayload::Voice { seconds } => {
            fields(&[("Voice", number(Some(*seconds), Unit::Seconds, 1))])
        }
        TetrapolPayload::Other { codop, .. } => {
            fields(&[("Codop", text_of(format!("{codop:#04x}")))])
        }
    }
}

fn system_info_fields(info: &TetrapolSystemInfo) -> Vec<Field> {
    fields(&[
        ("Cell", text_of(info.cell.label())),
        ("Country", count(info.country)),
        ("Network", count(info.network)),
        ("Location area", count(info.location_area)),
        ("Uplink code", count(Some(info.uplink_scrambling))),
        ("Ciphered", flag(Some(info.ciphered))),
        ("Home only", only_if(info.home_only)),
        ("Superframe", count(info.superframe)),
        ("Control channel", count(info.control_channel)),
    ])
}

fn group_call_fields(call: &TetrapolGroupCall) -> Vec<Field> {
    fields(&[
        ("Group", count(Some(call.group))),
        ("Call", text_of(call.call_type.label())),
        ("Coverage", count(Some(call.coverage))),
        ("Traffic channel", count(call.channel)),
        ("Downlink code", count(call.downlink_scrambling)),
        ("Uplink code", count(call.uplink_scrambling)),
        ("Encrypted", flag(Some(!call.key.clear()))),
    ])
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn a_group_activation_reads_as_one_line_and_round_trips() {
        let message = TetrapolMessage {
            channel: TetrapolLogicalChannel::Paging,
            frame: Some(98),
            scrambling: 70,
            address: Some(TetrapolAddress {
                kind: TetrapolAddressKind::AllStations,
                id: 0x7FFF,
            }),
            payload: TetrapolPayload::GroupActivation(TetrapolGroupCall {
                group: 393,
                call_type: TetrapolCallType::Internal,
                coverage: 200,
                key: TetrapolKey { kind: 4, index: 10 },
                channel: Some(2890),
                downlink_scrambling: Some(89),
                uplink_scrambling: Some(125),
            }),
            data: "551189c80b4a7d594a".to_owned(),
            rejected: 0,
        };
        assert_eq!(
            message.summary(),
            "PCH · all · group 393 active on channel 2890, encrypted"
        );
        assert_eq!(message.station().as_deref(), Some("group 393"));
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["payload"]["message"], "group_activation");
        assert_eq!(json["address"]["kind"], "all_stations");
        let back: TetrapolMessage = serde_json::from_value(json).unwrap();
        assert_eq!(back, message);
    }

    #[test]
    fn a_group_call_lists_its_group_and_key() {
        let message: TetrapolMessage = serde_json::from_value(serde_json::json!({
            "channel": "paging", "frame": 98, "scrambling": 70,
            "address": { "kind": "terminal", "id": 418 },
            "payload": {
                "message": "group_paging", "group": 171, "call_type": "broadcast",
                "coverage": 3, "key": { "kind": 15, "index": 0 },
            },
            "data": "5b", "rejected": 2,
        }))
        .unwrap();
        let view = message.present(EventKind::Tetrapol);
        assert_eq!(view.field("Channel"), text_of("PCH").as_ref());
        assert_eq!(view.field("Address"), text_of("TTI 01A2").as_ref());
        assert_eq!(view.field("Group"), count(Some(171)).as_ref());
        assert_eq!(view.field("Call"), text_of("Broadcast").as_ref());
        assert_eq!(view.field("Encrypted"), flag(Some(false)).as_ref());
        assert_eq!(view.field("Rejected"), count(Some(2)).as_ref());
        assert_eq!(view.field("Traffic channel"), None);
    }

    #[test]
    fn params_default_to_uhf_with_a_found_scrambling_code() {
        let params: TetrapolParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params, TetrapolParams::default());
        assert_eq!(params.band, TetrapolBand::Uhf);
        assert!(params.scrambling.is_none());
        assert_eq!(serde_json::to_string(&params).unwrap(), r#"{"band":"uhf"}"#);
    }
}
