use sdrmm_wire::GsmLogicalChannel;

use super::{burst::HYPERFRAME, coding::SCH_INFO};

const CONTROL_MULTIFRAME: u32 = 51;
const TRAFFIC_MULTIFRAME: u32 = 26;
const BLOCK_FRAMES: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SyncInfo {
    pub(crate) bsic: u8,
    pub(crate) frame_number: u32,
}

const BCC_BITS: [usize; 3] = [2, 3, 4];
const NCC_BITS: [usize; 3] = [5, 6, 7];
const T1_BITS: [usize; 11] = [23, 8, 9, 10, 11, 12, 13, 14, 15, 0, 1];
const T2_BITS: [usize; 5] = [18, 19, 20, 21, 22];
const T3P_BITS: [usize; 3] = [24, 16, 17];

fn field(bits: &[bool; SCH_INFO], positions: &[usize]) -> u32 {
    positions
        .iter()
        .enumerate()
        .map(|(weight, &at)| u32::from(bits[at]) << weight)
        .sum()
}

pub(crate) fn parse_sync(bits: &[bool; SCH_INFO]) -> Option<SyncInfo> {
    let t1 = field(bits, &T1_BITS);
    let t2 = field(bits, &T2_BITS);
    let t3 = 10 * field(bits, &T3P_BITS) + 1;
    if t2 >= TRAFFIC_MULTIFRAME || t3 >= CONTROL_MULTIFRAME {
        return None;
    }
    let difference = (t3 + TRAFFIC_MULTIFRAME - t2) % TRAFFIC_MULTIFRAME;
    let frame_number =
        CONTROL_MULTIFRAME * difference + t3 + CONTROL_MULTIFRAME * TRAFFIC_MULTIFRAME * t1;
    (frame_number < HYPERFRAME).then_some(SyncInfo {
        bsic: (field(bits, &NCC_BITS) << 3 | field(bits, &BCC_BITS)) as u8,
        frame_number,
    })
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn sync_bits(info: SyncInfo) -> [bool; SCH_INFO] {
    let t1 = info.frame_number / (CONTROL_MULTIFRAME * TRAFFIC_MULTIFRAME);
    let t2 = info.frame_number % TRAFFIC_MULTIFRAME;
    let t3p = (info.frame_number % CONTROL_MULTIFRAME - 1) / 10;
    let mut bits = [false; SCH_INFO];
    let mut put = |value: u32, positions: &[usize]| {
        for (weight, &at) in positions.iter().enumerate() {
            bits[at] = value >> weight & 1 == 1;
        }
    };
    put(u32::from(info.bsic & 7), &BCC_BITS);
    put(u32::from(info.bsic >> 3), &NCC_BITS);
    put(t1, &T1_BITS);
    put(t2, &T2_BITS);
    put(t3p, &T3P_BITS);
    bits
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BlockSlot {
    pub(crate) channel: GsmLogicalChannel,
    pub(crate) subchannel: Option<u8>,
    pub(crate) index: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlSlot {
    Frequency,
    Sync,
    Block(BlockSlot),
    Idle,
}

pub(crate) fn control_slot(frame_number: u32, combined: bool) -> ControlSlot {
    let position = frame_number % CONTROL_MULTIFRAME;
    let odd = frame_number / CONTROL_MULTIFRAME % 2 == 1;
    if position == 50 {
        return ControlSlot::Idle;
    }
    match position % 10 {
        0 => return ControlSlot::Frequency,
        1 => return ControlSlot::Sync,
        _ => {}
    }
    let index = ((position - 2) % 10 % 4) as u8;
    let group = position / 10;
    let second = (position % 10) >= 6;
    let block = |channel, subchannel| {
        ControlSlot::Block(BlockSlot {
            channel,
            subchannel,
            index,
        })
    };
    match (group, second) {
        (0, false) => block(GsmLogicalChannel::Bcch, None),
        (0, true) | (1, _) => block(GsmLogicalChannel::Ccch, None),
        (2 | 3, _) if combined => block(
            GsmLogicalChannel::Sdcch,
            Some(((group - 2) * 2) as u8 + u8::from(second)),
        ),
        (4, _) if combined => block(
            GsmLogicalChannel::Sacch,
            Some(u8::from(second) + if odd { 2 } else { 0 }),
        ),
        _ => block(GsmLogicalChannel::Ccch, None),
    }
}

pub(crate) fn sdcch8_slot(frame_number: u32) -> Option<BlockSlot> {
    let position = frame_number % CONTROL_MULTIFRAME;
    let odd = frame_number / CONTROL_MULTIFRAME % 2 == 1;
    let index = (position % BLOCK_FRAMES) as u8;
    match position {
        0..32 => Some(BlockSlot {
            channel: GsmLogicalChannel::Sdcch,
            subchannel: Some((position / BLOCK_FRAMES) as u8),
            index,
        }),
        32..48 => Some(BlockSlot {
            channel: GsmLogicalChannel::Sacch,
            subchannel: Some(((position - 32) / BLOCK_FRAMES) as u8 + if odd { 4 } else { 0 }),
            index,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(channel: GsmLogicalChannel, subchannel: Option<u8>, index: u8) -> ControlSlot {
        ControlSlot::Block(BlockSlot {
            channel,
            subchannel,
            index,
        })
    }

    #[test]
    fn the_control_multiframe_follows_the_standard_layout() {
        assert_eq!(control_slot(0, false), ControlSlot::Frequency);
        assert_eq!(control_slot(41, false), ControlSlot::Sync);
        assert_eq!(control_slot(50, false), ControlSlot::Idle);
        assert_eq!(
            control_slot(2, false),
            block(GsmLogicalChannel::Bcch, None, 0)
        );
        assert_eq!(
            control_slot(5, false),
            block(GsmLogicalChannel::Bcch, None, 3)
        );
        assert_eq!(
            control_slot(6, false),
            block(GsmLogicalChannel::Ccch, None, 0)
        );
        assert_eq!(
            control_slot(19, false),
            block(GsmLogicalChannel::Ccch, None, 3)
        );
        assert_eq!(
            control_slot(46, false),
            block(GsmLogicalChannel::Ccch, None, 0)
        );
        assert_eq!(
            control_slot(26, true),
            block(GsmLogicalChannel::Sdcch, Some(1), 0)
        );
        assert_eq!(
            control_slot(37, true),
            block(GsmLogicalChannel::Sdcch, Some(3), 1)
        );
        assert_eq!(
            control_slot(42, true),
            block(GsmLogicalChannel::Sacch, Some(0), 0)
        );
        assert_eq!(
            control_slot(51 + 49, true),
            block(GsmLogicalChannel::Sacch, Some(3), 3)
        );
    }

    #[test]
    fn sdcch8_carries_eight_dedicated_and_eight_associated_subchannels() {
        let slot = sdcch8_slot(862_261).unwrap();
        assert_eq!(slot.channel, GsmLogicalChannel::Sdcch);
        assert_eq!(slot.subchannel, Some(1));
        let slot = sdcch8_slot(862_242).unwrap();
        assert_eq!(slot.channel, GsmLogicalChannel::Sacch);
        assert_eq!(slot.subchannel, Some(1));
        assert_eq!(sdcch8_slot(48), None);
    }

    #[test]
    fn sync_bits_round_trip_the_frame_number_and_bsic() {
        for frame_number in [1, 11, 860_891, 2_715_647 - 9] {
            let info = SyncInfo {
                bsic: 0o60,
                frame_number,
            };
            assert_eq!(parse_sync(&sync_bits(info)), Some(info), "{frame_number}");
        }
    }
}
