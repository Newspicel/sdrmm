use sdrmm_wire::{
    ChannelParams, DvChannelDefinition, DvFrame, DvFrameKind, TrunkKind, TrunkScheme, TrunkSpec,
    VoiceKey,
};

use super::{bits_value, cipher_keys};
use crate::{ChannelError, TrunkGrant, Trunking, dv::P25Params};

const PROTOCOL: &str = "p25";
const LABEL: &str = "P25 trunking";

const IDEN_UP_VU: u8 = 0x34;
const IDEN_UP: u8 = 0x3D;

const CHANNEL_NUMBER_BITS: u16 = 12;
const SPACING_UNIT_HZ: u64 = 125;
const BASE_UNIT_HZ: u64 = 5;
const OFFSET_UNIT_HZ: i64 = 250_000;

pub(crate) fn trunking() -> Trunking {
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
        slots: 1,
        max_channel: u16::MAX,
        schemes: vec![TrunkScheme {
            id: PROTOCOL.to_owned(),
            label: LABEL.to_owned(),
            kind: TrunkKind::Grants,
        }],
        vocoder: crate::dv::chip::VOCODER_CHIPS,
    }
}

fn watch(_ignore_crc: bool) -> Result<ChannelParams, ChannelError> {
    ChannelParams::of(&P25Params::default()).map_err(ChannelError::InvalidSettings)
}

fn follow(
    slot: Option<u8>,
    _ignore_crc: bool,
    keys: &[VoiceKey],
    vocoder: Option<&str>,
) -> Result<ChannelParams, ChannelError> {
    if let Some(slot) = slot {
        return Err(ChannelError::InvalidSettings(format!(
            "P25 Phase 1 carries one call per channel, not slot {slot}"
        )));
    }
    cipher_keys(keys)?;
    ChannelParams::of(&P25Params {
        keys: keys.to_vec(),
        vocoder: vocoder.map(str::to_owned),
    })
    .map_err(ChannelError::InvalidSettings)
}

fn grants(frame: &DvFrame) -> Vec<TrunkGrant> {
    if frame.kind != DvFrameKind::Control {
        return Vec::new();
    }
    match (frame.channel, frame.destination) {
        (Some(logical_channel), Some(destination)) => vec![TrunkGrant {
            logical_channel,
            slot: None,
            destination,
            source: frame.source,
            group_call: frame.group_call,
        }],
        _ => Vec::new(),
    }
}

/// Names a trunking block as the control channel of a P25 trunked site.
pub(super) fn mark(frame: &mut DvFrame) {
    frame.trunk_protocol = Some(PROTOCOL.to_owned());
    frame.trunk_label = Some(LABEL.to_owned());
    frame.control_channel = Some(true);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Band {
    base_hz: u64,
    spacing_hz: u64,
    tx_offset_hz: i64,
}

/// The channel identifiers a site broadcasts: a channel number's top four bits pick one, the
/// rest count channels up from its base frequency.
#[derive(Debug, Default)]
pub(super) struct BandPlan {
    bands: [Option<Band>; 16],
}

impl BandPlan {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    /// Takes in a channel identifier update, when `payload` is one.
    pub(super) fn learn(&mut self, payload: &[bool; 96]) {
        let opcode = bits_value(&payload[2..8]) as u8;
        let standard = bits_value(&payload[8..16]) == 0;
        let iden = bits_value(&payload[16..20]) as usize;
        let spacing_hz = u64::from(bits_value(&payload[38..48])) * SPACING_UNIT_HZ;
        let base_hz = u64::from(bits_value(&payload[48..80])) * BASE_UNIT_HZ;
        let offset = match opcode {
            IDEN_UP => i64::from(bits_value(&payload[30..38])) * OFFSET_UNIT_HZ,
            IDEN_UP_VU => i64::from(bits_value(&payload[25..38])) * spacing_hz as i64,
            _ => return,
        };
        let positive = if opcode == IDEN_UP {
            payload[29]
        } else {
            payload[24]
        };
        if !standard || spacing_hz == 0 || base_hz == 0 {
            return;
        }
        self.bands[iden] = Some(Band {
            base_hz,
            spacing_hz,
            tx_offset_hz: if positive { offset } else { -offset },
        });
    }

    /// Places the channel a grant names, once the site has broadcast its identifier.
    pub(super) fn place(&self, frame: &mut DvFrame) {
        let (Some(channel), Some(_)) = (frame.channel, frame.destination) else {
            return;
        };
        let Some(band) = self.bands[usize::from(channel >> CHANNEL_NUMBER_BITS)] else {
            return;
        };
        let number = u64::from(channel & ((1 << CHANNEL_NUMBER_BITS) - 1));
        let rx_hz = band.base_hz + number * band.spacing_hz;
        frame.channel_definition = Some(DvChannelDefinition {
            channel,
            tx_hz: rx_hz.saturating_add_signed(band.tx_offset_hz),
            rx_hz,
            color_code: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(value: u64, width: usize) -> Vec<bool> {
        (0..width).rev().map(|bit| value >> bit & 1 != 0).collect()
    }

    /// An IDEN_UP for 851.00625 MHz in 6.25 kHz steps, mobiles 45 MHz below.
    fn iden_up(iden: u64) -> [bool; 96] {
        let mut payload = bits(u64::from(IDEN_UP), 8);
        payload.extend(bits(0, 8));
        payload.extend(bits(iden, 4));
        payload.extend(bits(0x32, 9));
        payload.extend(bits(0, 1));
        payload.extend(bits(180, 8));
        payload.extend(bits(50, 10));
        payload.extend(bits(851_006_250 / 5, 32));
        payload.extend(bits(0, 16));
        payload.try_into().expect("96 bits")
    }

    #[test]
    fn a_grant_is_placed_by_the_sites_channel_identifier() {
        let mut plan = BandPlan::default();
        plan.learn(&iden_up(1));
        let mut grant = DvFrame {
            channel: Some(1 << 12 | 10),
            destination: Some(0x1201),
            ..DvFrame::new("P25", DvFrameKind::Control)
        };
        plan.place(&mut grant);

        let definition = grant.channel_definition.clone().expect("placed");
        assert_eq!(definition.rx_hz, 851_068_750);
        assert_eq!(definition.tx_hz, 806_068_750);
        assert_eq!(
            crate::dv::announced_channels(&grant),
            vec![crate::TrunkPlacement {
                logical_channel: 1 << 12 | 10,
                freq_hz: 851_068_750,
            }]
        );
    }

    #[test]
    fn a_grant_on_an_unknown_identifier_is_left_unplaced() {
        let mut plan = BandPlan::default();
        plan.learn(&iden_up(1));
        let mut grant = DvFrame {
            channel: Some(2 << 12 | 10),
            destination: Some(0x1201),
            ..DvFrame::new("P25", DvFrameKind::Control)
        };
        plan.place(&mut grant);
        assert!(grant.channel_definition.is_none());
    }

    #[test]
    fn a_grant_carries_no_timeslot() {
        let grant = DvFrame {
            channel: Some(0x1234),
            destination: Some(0x1201),
            source: Some(0xABCDEF),
            group_call: Some(true),
            ..DvFrame::new("P25", DvFrameKind::Control)
        };
        assert_eq!(
            grants(&grant),
            vec![TrunkGrant {
                logical_channel: 0x1234,
                slot: None,
                destination: 0x1201,
                source: Some(0xABCDEF),
                group_call: Some(true),
            }]
        );
        assert!(follow(None, false, &[], None).is_ok());
        assert!(follow(Some(1), false, &[], None).is_err());
    }
}
