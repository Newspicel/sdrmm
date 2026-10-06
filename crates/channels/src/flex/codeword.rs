use sdrmm_dsp::pocsag_bch_decode;

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
    pocsag_bch_decode(to_pocsag(raw)).map(|(corrected, _)| from_pocsag(corrected))
}

fn data_of(raw: u32) -> u32 {
    reverse21(to_pocsag(raw) >> 11)
}

pub fn decode_word(raw: u32) -> Option<(u32, u32)> {
    let (corrected, errors) = pocsag_bch_decode(to_pocsag(raw))?;
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
        None => chase(received),
    }
}

fn least_reliable(reliability: &[f32; 32]) -> [usize; CHASE_BITS] {
    let mut order: [usize; 32] = std::array::from_fn(|bit| bit);
    order.select_nth_unstable_by(CHASE_BITS, |&a, &b| {
        reliability[a].total_cmp(&reliability[b])
    });
    std::array::from_fn(|index| order[index])
}

fn chase(received: &Received) -> Word {
    let weak = least_reliable(&received.reliability);
    let mut best: Option<(f32, u32)> = None;
    for pattern in 1..1u32 << CHASE_BITS {
        let flips = weak
            .iter()
            .enumerate()
            .filter(|(index, _)| pattern >> index & 1 == 1)
            .fold(0u32, |mask, (_, &bit)| mask | 1 << bit);
        let Some(candidate) = repaired(received.raw ^ flips) else {
            continue;
        };
        let distance = distance(received, candidate);
        if best.is_none_or(|(metric, _)| distance < metric) {
            best = Some((distance, candidate));
        }
    }
    best.map_or(Word::Lost, |(_, candidate)| Word::Soft {
        data: data_of(candidate),
        errors: (candidate ^ received.raw).count_ones(),
    })
}

fn distance(received: &Received, candidate: u32) -> f32 {
    let differ = candidate ^ received.raw;
    (0..32)
        .filter(|bit| differ >> bit & 1 == 1)
        .map(|bit| received.reliability[bit])
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdrmm_dsp::pocsag_bch_encode;

    fn codeword(data: u32) -> u32 {
        from_pocsag(pocsag_bch_encode(reverse21(data)))
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
