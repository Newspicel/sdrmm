use sdrmm_wire::{ChannelParams, DvFrame, TrunkKind, TrunkScheme, TrunkSpec, VoiceKey};

use super::cipher_keys;
use crate::{ChannelError, TrunkGrant, Trunking, dv::DmrParams, dv::DmrSlots};

/// A trunking protocol DMR signalling announces.
#[derive(Clone, Copy)]
pub(super) struct DmrTrunk {
    pub(super) id: &'static str,
    label: &'static str,
    kind: TrunkKind,
}

impl DmrTrunk {
    pub(super) fn mark(self, frame: &mut DvFrame) {
        frame.trunk_protocol = Some(self.id.to_owned());
        frame.trunk_label = Some(self.label.to_owned());
    }
}

pub(super) const CAPACITY_PLUS: DmrTrunk = DmrTrunk {
    id: "capacity_plus",
    label: "Capacity Plus",
    kind: TrunkKind::Distributed,
};
pub(super) const HYTERA_XPT: DmrTrunk = DmrTrunk {
    id: "hytera_xpt",
    label: "Hytera XPT",
    kind: TrunkKind::Distributed,
};
pub(super) const TIER_THREE: DmrTrunk = DmrTrunk {
    id: "tier_three",
    label: "Tier III",
    kind: TrunkKind::Grants,
};

/// Tier III numbers its logical channels in 12 bits.
const MAX_LOGICAL_CHANNEL: u16 = 4095;

pub(super) fn trunking() -> Trunking {
    Trunking {
        watch,
        follow,
        spec,
        grants,
        placements: crate::dv::announced_channels,
    }
}

fn spec() -> TrunkSpec {
    TrunkSpec {
        slots: 2,
        max_channel: MAX_LOGICAL_CHANNEL,
        schemes: [CAPACITY_PLUS, HYTERA_XPT, TIER_THREE]
            .into_iter()
            .map(|trunk| TrunkScheme {
                id: trunk.id.to_owned(),
                label: trunk.label.to_owned(),
                kind: trunk.kind,
            })
            .collect(),
        vocoder: crate::dv::chip::VOCODER_CHIPS,
    }
}

fn watch(ignore_crc: bool) -> Result<ChannelParams, ChannelError> {
    ChannelParams::of(&DmrParams {
        slots: DmrSlots::Both,
        ignore_crc,
        ..DmrParams::default()
    })
    .map_err(ChannelError::InvalidSettings)
}

pub(super) fn follow(
    slot: Option<u8>,
    ignore_crc: bool,
    keys: &[VoiceKey],
    vocoder: Option<&str>,
) -> Result<ChannelParams, ChannelError> {
    cipher_keys(keys)?;
    let slots = match slot {
        Some(1) => DmrSlots::One,
        Some(2) => DmrSlots::Two,
        other => {
            return Err(ChannelError::InvalidSettings(format!(
                "DMR has slots 1 and 2, not {other:?}"
            )));
        }
    };
    ChannelParams::of(&DmrParams {
        slots,
        ignore_crc,
        keys: keys.to_vec(),
        vocoder: vocoder.map(str::to_owned),
    })
    .map_err(ChannelError::InvalidSettings)
}

/// A Capacity Max channel update names every busy timeslot; a Tier III grant names one call.
fn grants(frame: &DvFrame) -> Vec<TrunkGrant> {
    let updates: Vec<TrunkGrant> = frame
        .slot_activity
        .iter()
        .filter_map(|activity| {
            Some(TrunkGrant {
                logical_channel: activity.logical_channel?,
                slot: Some(activity.slot),
                destination: activity.destination?,
                source: None,
                group_call: None,
            })
        })
        .collect();
    if !updates.is_empty() {
        return updates;
    }
    match (frame.channel, frame.slot, frame.destination) {
        (Some(logical_channel), Some(slot), Some(destination)) => vec![TrunkGrant {
            logical_channel,
            slot: Some(slot),
            destination,
            source: frame.source,
            group_call: frame.group_call,
        }],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{DvFrameKind, DvSlotActivity};

    use super::*;

    #[test]
    fn a_grant_names_its_timeslot() {
        let frame = DvFrame {
            channel: Some(17),
            slot: Some(2),
            destination: Some(91),
            source: Some(4001),
            ..DvFrame::new("DMR", DvFrameKind::Control)
        };
        assert_eq!(
            grants(&frame),
            vec![TrunkGrant {
                logical_channel: 17,
                slot: Some(2),
                destination: 91,
                source: Some(4001),
                group_call: None,
            }]
        );
    }

    #[test]
    fn a_channel_update_grants_every_busy_timeslot() {
        let busy = |slot, destination| DvSlotActivity {
            slot,
            activity: "group voice".to_owned(),
            destination_hash: None,
            destination: Some(destination),
            logical_channel: Some(5),
        };
        let frame = DvFrame {
            channel: Some(5),
            slot_activity: vec![busy(1, 10), busy(2, 20)],
            ..DvFrame::new("DMR", DvFrameKind::Control)
        };
        let granted: Vec<(Option<u8>, u32)> = grants(&frame)
            .into_iter()
            .map(|grant| (grant.slot, grant.destination))
            .collect();
        assert_eq!(granted, vec![(Some(1), 10), (Some(2), 20)]);
    }

    #[test]
    fn a_grant_without_a_timeslot_is_no_dmr_grant() {
        let frame = DvFrame {
            channel: Some(17),
            destination: Some(91),
            ..DvFrame::new("DMR", DvFrameKind::Control)
        };
        assert!(grants(&frame).is_empty());
        assert!(follow(None, false, &[], None).is_err());
    }
}
