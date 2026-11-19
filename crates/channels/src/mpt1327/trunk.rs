//! How a trunk node runs an MPT 1327 site: it watches the control channel and follows each
//! granted call onto its traffic channel as analog FM voice.

use sdrmm_decoder::{ChannelError, TrunkGrant, Trunking};
use sdrmm_wire::{ChannelParams, DvFrame, DvFrameKind, TrunkKind, TrunkScheme, TrunkSpec};

use super::Mpt1327Params;
use super::frame::{LABEL, PROTOCOL};
use super::message::GTC;
use crate::nfm::NfmParams;

/// Channel numbers are ten bits wide.
const MAX_CHANNEL: u16 = 1_023;

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
        max_channel: MAX_CHANNEL,
        schemes: vec![TrunkScheme {
            id: PROTOCOL.to_owned(),
            label: LABEL.to_owned(),
            kind: TrunkKind::Grants,
        }],
        vocoder: false,
    }
}

fn watch(_ignore_crc: bool) -> Result<ChannelParams, ChannelError> {
    ChannelParams::of(&Mpt1327Params::default()).map_err(ChannelError::InvalidSettings)
}

/// A granted call is analog speech, so it is followed by an FM receiver.
fn follow(slot: Option<u8>, _calls: &ChannelParams) -> Result<ChannelParams, ChannelError> {
    if let Some(slot) = slot {
        return Err(ChannelError::InvalidSettings(format!(
            "MPT 1327 carries one call per channel, not slot {slot}"
        )));
    }
    ChannelParams::of(&NfmParams::default()).map_err(ChannelError::InvalidSettings)
}

fn grants(frame: &DvFrame) -> Vec<TrunkGrant> {
    if frame.kind != DvFrameKind::Control
        || frame.opcode.as_deref() != Some(GTC)
        || frame.control_channel == Some(false)
    {
        return Vec::new();
    }
    match (frame.channel, frame.destination) {
        (Some(logical_channel), Some(destination)) => vec![TrunkGrant {
            logical_channel,
            slot: None,
            destination,
            source: frame.source,
            group_call: None,
        }],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mpt1327::frame::{Heard, dv_frame};
    use crate::mpt1327::message::{Address, Message};

    fn grant(data: bool, control: bool) -> DvFrame {
        dv_frame(&Heard {
            message: Message::Gtc {
                called: Address {
                    prefix: 10,
                    ident: 4105,
                },
                calling: Address {
                    prefix: 10,
                    ident: 77,
                },
                channel: 123,
                data,
            },
            info: 0,
            data: &[],
            errors: 0,
            system: None,
            control,
        })
    }

    #[test]
    fn a_voice_grant_on_the_control_channel_is_followed() {
        assert_eq!(
            grants(&grant(false, true)),
            vec![TrunkGrant {
                logical_channel: 123,
                slot: None,
                destination: 10 * 8192 + 4105,
                source: Some(10 * 8192 + 77),
                group_call: None,
            }]
        );
    }

    #[test]
    fn data_grants_and_traffic_channel_moves_are_not() {
        assert!(grants(&grant(true, true)).is_empty());
        assert!(grants(&grant(false, false)).is_empty());
    }

    #[test]
    fn calls_are_followed_by_an_fm_receiver() {
        let calls = ChannelParams::bare("mpt1327");
        let params = follow(None, &calls).expect("follows");
        assert_eq!(params.type_id(), "nfm");
        assert!(follow(Some(1), &calls).is_err());
        assert_eq!(watch(true).expect("watches").type_id(), "mpt1327");
    }
}
