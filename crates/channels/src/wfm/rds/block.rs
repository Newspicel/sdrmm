use sdrmm_dsp::BurstCode;

/// The (26,16) block code, repairing bursts up to five bits.
static CODE: BurstCode<1024> = BurstCode::new(0x5B9, 26, 10, 5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RdsOffset {
    A,
    B,
    C,
    CPrime,
    D,
}

impl RdsOffset {
    const fn word(self) -> u32 {
        match self {
            Self::A => 0x0FC,
            Self::B => 0x198,
            Self::C => 0x168,
            Self::CPrime => 0x350,
            Self::D => 0x1B4,
        }
    }
}

pub(crate) fn check_block(block: u32, offset: RdsOffset) -> Option<u16> {
    CODE.check(block, offset.word()).map(|data| data as u16)
}

pub(crate) fn correct_block(block: u32, offset: RdsOffset) -> Option<(u16, u32)> {
    CODE.correct(block, offset.word())
        .map(|(data, flips)| (data as u16, flips))
}

/// The codeword [`correct_block`] repairs `block` to.
pub(crate) fn corrected_word(block: u32, offset: RdsOffset) -> Option<u32> {
    correct_block(block, offset).map(|(data, _)| CODE.encode(u32::from(data), offset.word()))
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn encode_block(data: u16, offset: RdsOffset) -> u32 {
    CODE.encode(u32::from(data), offset.word())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_round_trips_under_each_offset_and_fails_under_the_others() {
        let offsets = [
            RdsOffset::A,
            RdsOffset::B,
            RdsOffset::C,
            RdsOffset::CPrime,
            RdsOffset::D,
        ];
        for offset in offsets {
            let block = encode_block(0xACDC, offset);
            assert_eq!(check_block(block, offset), Some(0xACDC));
            assert_eq!(correct_block(block ^ 0b1_0110, offset), Some((0xACDC, 3)));
            for other in offsets.into_iter().filter(|&other| other != offset) {
                assert_eq!(check_block(block, other), None);
            }
        }
    }
}
