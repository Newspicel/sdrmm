use sdrmm_decoder::{ChannelError, TrunkGrant, Trunking};
use sdrmm_wire::{ChannelParams, DvFrame, DvFrameKind, TrunkKind, TrunkScheme, TrunkSpec};

use super::EdacsParams;

const PROTOCOL: &str = "edacs";
const LABEL: &str = "EDACS";
const MODE: &str = "EDACS";

/// Five bits of logical channel, the top six of them call states.
const MAX_LCN: u16 = 25;

pub(crate) fn trunking() -> Trunking {
    Trunking {
        watch,
        follow,
        spec,
        grants,
        placements: sdrmm_decoder::announced_placements,
    }
}

fn spec() -> TrunkSpec {
    TrunkSpec {
        slots: 1,
        max_channel: MAX_LCN,
        schemes: vec![TrunkScheme {
            id: PROTOCOL.to_owned(),
            label: LABEL.to_owned(),
            kind: TrunkKind::Grants,
        }],
        vocoder: false,
    }
}

fn watch(_ignore_crc: bool) -> Result<ChannelParams, ChannelError> {
    ChannelParams::of(&EdacsParams::default()).map_err(ChannelError::InvalidSettings)
}

/// A working channel runs the same decoder: it plays the voice once the site's data stops.
fn follow(slot: Option<u8>, calls: &ChannelParams) -> Result<ChannelParams, ChannelError> {
    if let Some(slot) = slot {
        return Err(ChannelError::InvalidSettings(format!(
            "EDACS carries one call per channel, not slot {slot}"
        )));
    }
    let params: EdacsParams = calls.parse().map_err(ChannelError::InvalidSettings)?;
    ChannelParams::of(&params).map_err(ChannelError::InvalidSettings)
}

/// An analog voice call the site put on a channel. Only those name their channel as one:
/// digital and data calls carry theirs as text, so they are never followed.
fn grants(frame: &DvFrame) -> Vec<TrunkGrant> {
    if frame.kind != DvFrameKind::Control {
        return Vec::new();
    }
    match (frame.channel, frame.destination, frame.group_call) {
        (Some(logical_channel), Some(destination), Some(_)) => vec![TrunkGrant {
            logical_channel,
            slot: None,
            destination,
            source: frame.source,
            group_call: frame.group_call,
        }],
        _ => Vec::new(),
    }
}

fn marked(kind: DvFrameKind, control: bool) -> DvFrame {
    DvFrame {
        trunk_protocol: Some(PROTOCOL.to_owned()),
        trunk_label: Some(LABEL.to_owned()),
        control_channel: Some(control),
        ..DvFrame::new(MODE, kind)
    }
}

/// A message from a site's control channel.
pub(super) fn control_frame() -> DvFrame {
    DvFrame {
        crc_verified: Some(true),
        ..marked(DvFrameKind::Control, true)
    }
}

/// Voice opening or closing on a working channel; the grant that sent it there names it.
pub(super) fn working_frame(kind: DvFrameKind) -> DvFrame {
    marked(kind, false)
}
