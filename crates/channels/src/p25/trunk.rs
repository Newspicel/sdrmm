use sdrmm_wire::{
    ChannelParams, DvChannelDefinition, DvFrame, DvFrameKind, TrunkKind, TrunkScheme, TrunkSpec,
};

use super::P25Params;
use super::phase2::{P25p2Params, P25p2Slots};
use super::{bits_value, cipher_keys};
use sdrmm_decoder::{ChannelError, TrunkGrant, Trunking};

const PROTOCOL: &str = "p25";
const LABEL: &str = "P25 trunking";

const IDEN_UP_TDMA: u8 = 0x33;
const IDEN_UP_VU: u8 = 0x34;
const IDEN_UP: u8 = 0x3D;

const CHANNEL_NUMBER_BITS: u16 = 12;
const SPACING_UNIT_HZ: u64 = 125;
const BASE_UNIT_HZ: u64 = 5;
const OFFSET_UNIT_HZ: i64 = 250_000;
/// Timeslots per carrier by a TDMA identifier's channel type.
const SLOTS_BY_CHANNEL_TYPE: [u16; 6] = [1, 1, 1, 2, 4, 2];

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
        max_channel: u16::MAX,
        schemes: vec![TrunkScheme {
            id: PROTOCOL.to_owned(),
            label: LABEL.to_owned(),
            kind: TrunkKind::Grants,
        }],
        vocoder: sdrmm_codec::voice::chip::VOCODER_CHIPS,
    }
}

fn watch(_ignore_crc: bool) -> Result<ChannelParams, ChannelError> {
    ChannelParams::of(&P25Params::default()).map_err(ChannelError::InvalidSettings)
}

/// A call without a timeslot is Phase 1; one in a timeslot of a TDMA carrier is Phase 2.
fn follow(slot: Option<u8>, calls: &ChannelParams) -> Result<ChannelParams, ChannelError> {
    let params: P25Params = calls.parse().map_err(ChannelError::InvalidSettings)?;
    let Some(slot) = slot else {
        cipher_keys(&params.keys)?;
        return ChannelParams::of(&params).map_err(ChannelError::InvalidSettings);
    };
    let slots = match slot {
        1 => P25p2Slots::One,
        2 => P25p2Slots::Two,
        other => {
            return Err(ChannelError::InvalidSettings(format!(
                "P25 Phase 2 has slots 1 and 2, not {other}"
            )));
        }
    };
    ChannelParams::of(&P25p2Params {
        slots,
        vocoder: params.vocoder,
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
            slot: frame.slot,
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
    slots: u16,
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
            IDEN_UP_VU | IDEN_UP_TDMA => {
                i64::from(bits_value(&payload[25..38])) * spacing_hz as i64
            }
            _ => return,
        };
        let slots = if opcode == IDEN_UP_TDMA {
            let channel_type = bits_value(&payload[20..24]) as usize;
            SLOTS_BY_CHANNEL_TYPE
                .get(channel_type)
                .copied()
                .unwrap_or(1)
        } else {
            1
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
            slots,
        });
    }

    /// Places the channel a grant names, once the site has broadcast its identifier. On a TDMA
    /// carrier the channel number counts timeslots, so it also names the slot, from 1.
    pub(super) fn place(&self, frame: &mut DvFrame) {
        let (Some(channel), Some(_)) = (frame.channel, frame.destination) else {
            return;
        };
        let Some(band) = self.bands[usize::from(channel >> CHANNEL_NUMBER_BITS)] else {
            return;
        };
        let number = channel & ((1 << CHANNEL_NUMBER_BITS) - 1);
        let rx_hz = band.base_hz + u64::from(number / band.slots) * band.spacing_hz;
        if band.slots > 1 && frame.slot.is_none() {
            frame.slot = u8::try_from(number % band.slots + 1).ok();
        }
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

    /// An `IDEN_UP` for 851.00625 MHz in 6.25 kHz steps, mobiles 45 MHz below.
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
            channel: Some(1 << 12 | 0xA),
            destination: Some(0x1201),
            ..DvFrame::new("P25", DvFrameKind::Control)
        };
        plan.place(&mut grant);

        let definition = grant.channel_definition.clone().expect("placed");
        assert_eq!(definition.rx_hz, 851_068_750);
        assert_eq!(definition.tx_hz, 806_068_750);
        assert_eq!(
            sdrmm_decoder::announced_placements(&grant),
            vec![sdrmm_decoder::TrunkPlacement {
                logical_channel: 1 << 12 | 0xA,
                freq_hz: 851_068_750,
            }]
        );
    }

    /// An `IDEN_UP_TDMA` for two-slot carriers from 769.00625 MHz in 12.5 kHz steps, mobiles
    /// 30 MHz above.
    fn iden_up_tdma(iden: u64) -> [bool; 96] {
        let mut payload = bits(u64::from(IDEN_UP_TDMA), 8);
        payload.extend(bits(0, 8));
        payload.extend(bits(iden, 4));
        payload.extend(bits(3, 4));
        payload.extend(bits(1, 1));
        payload.extend(bits(30_000_000 / 12_500, 13));
        payload.extend(bits(100, 10));
        payload.extend(bits(769_006_250 / 5, 32));
        payload.extend(bits(0, 16));
        payload.try_into().expect("96 bits")
    }

    #[test]
    fn a_tdma_grant_names_its_carrier_and_timeslot() {
        let mut plan = BandPlan::default();
        plan.learn(&iden_up_tdma(2));
        let mut grant = DvFrame {
            channel: Some(2 << 12 | 0x15),
            destination: Some(201),
            ..DvFrame::new("P25", DvFrameKind::Control)
        };
        plan.place(&mut grant);

        let definition = grant.channel_definition.clone().expect("placed");
        assert_eq!(definition.rx_hz, 769_006_250 + 10 * 12_500);
        assert_eq!(definition.tx_hz, definition.rx_hz + 30_000_000);
        assert_eq!(grant.slot, Some(2));
        let granted = grants(&grant);
        assert_eq!(granted[0].slot, Some(2));

        let calls = ChannelParams::bare("p25");
        let followed = follow(granted[0].slot, &calls).expect("phase 2 call");
        assert_eq!(followed.type_id(), "p25p2");
        let params: P25p2Params = followed.parse().expect("phase 2 settings");
        assert_eq!(params.slots, P25p2Slots::Two);
        assert!(follow(Some(3), &calls).is_err());
    }

    #[test]
    fn a_grant_on_an_unknown_identifier_is_left_unplaced() {
        let mut plan = BandPlan::default();
        plan.learn(&iden_up(1));
        let mut grant = DvFrame {
            channel: Some(2 << 12 | 0xA),
            destination: Some(0x1201),
            ..DvFrame::new("P25", DvFrameKind::Control)
        };
        plan.place(&mut grant);
        assert!(grant.channel_definition.is_none());
    }

    #[test]
    fn a_phase_one_grant_carries_no_timeslot() {
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
        let calls = ChannelParams::bare("p25");
        let followed = follow(None, &calls).expect("phase 1 call");
        assert_eq!(followed.type_id(), "p25");
    }
}
