use sdrmm_decoder::{ChannelError, TrunkGrant, Trunking};
use sdrmm_wire::{ChannelParams, DvFrame, DvFrameKind, TrunkKind, TrunkScheme, TrunkSpec};

use super::LtrParams;
use super::word::MAX_REPEATER;

const PROTOCOL: &str = "ltr";
const LABEL: &str = "LTR";
const MODE: &str = "LTR";

/// Every LTR repeater carries calls and its own data, so a system is followed by watching all
/// of them; the free repeater a word names is where the next call goes.
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
        max_channel: u16::from(MAX_REPEATER),
        schemes: vec![TrunkScheme {
            id: PROTOCOL.to_owned(),
            label: LABEL.to_owned(),
            kind: TrunkKind::Distributed,
        }],
        vocoder: false,
    }
}

fn watch(_ignore_crc: bool) -> Result<ChannelParams, ChannelError> {
    ChannelParams::of(&LtrParams::default()).map_err(ChannelError::InvalidSettings)
}

fn follow(slot: Option<u8>, calls: &ChannelParams) -> Result<ChannelParams, ChannelError> {
    if let Some(slot) = slot {
        return Err(ChannelError::InvalidSettings(format!(
            "LTR carries one call per repeater, not slot {slot}"
        )));
    }
    let params: LtrParams = calls.parse().map_err(ChannelError::InvalidSettings)?;
    ChannelParams::of(&params).map_err(ChannelError::InvalidSettings)
}

/// No repeater grants another's channel; each is watched for its own calls.
fn grants(_frame: &DvFrame) -> Vec<TrunkGrant> {
    Vec::new()
}

pub(super) fn frame(kind: DvFrameKind) -> DvFrame {
    DvFrame {
        trunk_protocol: Some(PROTOCOL.to_owned()),
        trunk_label: Some(LABEL.to_owned()),
        control_channel: Some(false),
        crc_verified: Some(true),
        ..DvFrame::new(MODE, kind)
    }
}
