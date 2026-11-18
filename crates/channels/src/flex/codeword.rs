use sdrmm_dsp::{ShortBch, coding::fec::chase};

/// BCH(31,21) plus an even parity bit, sent LSB first.
const BCH: ShortBch<31> = ShortBch::new(0x769, 10);

pub const WORDS: usize = 88;
pub const PHASE_BITS: usize = WORDS * 32;
const CHASE_BITS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Word {
    Hard { data: u32, errors: u32 },
    Soft { data: u32, errors: u32 },
    Lost,
}

impl Word {
    pub fn data(self) -> Option<u32> {
        match self {
            Self::Hard { data, .. } | Self::Soft { data, .. } => Some(data),
            Self::Lost => None,
        }
    }

    pub fn errors(self) -> u32 {
        match self {
            Self::Hard { errors, .. } | Self::Soft { errors, .. } => errors,
            Self::Lost => 0,
        }
    }

    pub fn is_soft(self) -> bool {
        matches!(self, Self::Soft { .. })
    }
}

#[derive(Clone, Copy)]
pub struct Received {
    raw: u32,
    reliability: [f32; 32],
}

fn reverse31(word: u32) -> u32 {
    word.reverse_bits() >> 1
}

fn reverse21(word: u32) -> u32 {
    word.reverse_bits() >> 11
}

fn to_pocsag(raw: u32) -> u32 {
    reverse31(raw) << 1 | raw >> 31
}

fn from_pocsag(pocsag: u32) -> u32 {
    reverse31(pocsag >> 1) | (pocsag & 1) << 31
}

fn repaired(raw: u32) -> Option<u32> {
    BCH.decode_extended(to_pocsag(raw))
        .map(|(corrected, _)| from_pocsag(corrected))
}

fn data_of(raw: u32) -> u32 {
    reverse21(to_pocsag(raw) >> 11)
}

#[cfg(any(test, feature = "synth"))]
pub(crate) fn encode_word(data: u32) -> u32 {
    from_pocsag(BCH.encode_extended(reverse21(data & 0x1F_FFFF)))
}

pub fn decode_word(raw: u32) -> Option<(u32, u32)> {
    let (corrected, errors) = BCH.decode_extended(to_pocsag(raw))?;
    Some((reverse21(corrected >> 11), errors))
}

pub fn deinterleave(bits: &[f32]) -> Option<[Received; WORDS]> {
    if bits.len() != PHASE_BITS {
        return None;
    }
    let mut words = [Received {
        raw: 0,
        reliability: [0.0; 32],
    }; WORDS];
    let mut at = 0;
    for block in 0..11 {
        for bit in 0..32 {
            for word in &mut words[block * 8..block * 8 + 8] {
                let soft = bits[at];
                word.raw |= u32::from(soft > 0.0) << bit;
                word.reliability[bit] = soft.abs();
                at += 1;
            }
        }
    }
    Some(words)
}

pub fn decode(received: &Received) -> Word {
    match decode_word(received.raw) {
        Some((data, errors)) => Word::Hard { data, errors },
        None => chase_word(received),
    }
}

fn chase_word(received: &Received) -> Word {
    let raw = u64::from(received.raw);
    let repair = |word: u64| repaired(word as u32).map(u64::from);
    chase(raw, &received.reliability, CHASE_BITS, repair).map_or(Word::Lost, |candidate| {
        let candidate = candidate as u32;
        Word::Soft {
            data: data_of(candidate),
            errors: (candidate ^ received.raw).count_ones(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codeword(data: u32) -> u32 {
        from_pocsag(BCH.encode_extended(reverse21(data)))
    }

    fn received(raw: u32, weak: &[usize]) -> Received {
        let mut reliability = [1.0; 32];
        for &bit in weak {
            reliability[bit] = 0.05;
        }
        Received { raw, reliability }
    }

    #[test]
    fn raw_words_round_trip_through_pocsag_order() {
        for raw in [0u32, 1, 0x8000_0000, 0x1234_5678, u32::MAX] {
            assert_eq!(from_pocsag(to_pocsag(raw)), raw);
        }
    }

    #[test]
    fn a_clean_word_decodes_hard() {
        let word = codeword(0x1A_2B3C);
        assert_eq!(
            decode(&received(word, &[])),
            Word::Hard {
                data: 0x1A_2B3C,
                errors: 0
            }
        );
    }

    #[test]
    fn three_errors_on_weak_bits_are_repaired_softly() {
        let word = codeword(0x0F_00F0);
        let broken = word ^ (1 << 3 | 1 << 17 | 1 << 29);
        assert_eq!(decode_word(broken), None);
        assert_eq!(
            decode(&received(broken, &[3, 17, 29])),
            Word::Soft {
                data: 0x0F_00F0,
                errors: 3
            }
        );
    }

    #[test]
    fn deinterleaving_spreads_each_word_over_the_block() {
        let mut bits = vec![-1.0; PHASE_BITS];
        bits[0] = 0.5;
        bits[8] = 0.25;
        let words = deinterleave(&bits).expect("full phase");
        assert_eq!(words[0].raw, 0b11);
        assert_eq!(words[0].reliability[1], 0.25);
        assert_eq!(words[1].raw, 0);
        assert!(deinterleave(&bits[1..]).is_none());
    }
}
