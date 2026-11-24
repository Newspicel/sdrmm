//! One MDC-1200 block: 112 bits interleaved over a 16 by 7 grid, holding op, argument, unit
//! and CRC with a rate 1/2 convolutional code.

use sdrmm_dsp::TrellisCode;
use sdrmm_dsp::coding::fec::conv::Soft;
use sdrmm_modem::soft::SoftBit;

pub(crate) const SYNC: u64 = 0x07_092A_446F;
pub(crate) const SYNC_BITS: u32 = 40;
pub(crate) const BLOCK_BITS: usize = 112;
const DATA_BITS: usize = BLOCK_BITS / 2;
const DATA_BYTES: usize = DATA_BITS / 8;
const ROWS: usize = 7;
const COLUMNS: usize = 16;
/// Parity taps of the convolutional code: each parity bit sums the data bits this far back.
const TAPS: [usize; 4] = [0, 2, 5, 6];
const MEMORY: usize = 6;
const STATES: usize = 1 << MEMORY;
/// The code sent as a trellis: the state holds the last six data bits, newest lowest, and each
/// step sends the data bit then its parity. The zero last byte ends it in state 0.
const CODE: TrellisCode<STATES, 2, 1> = trellis();

const fn trellis() -> TrellisCode<STATES, 2, 1> {
    let mut next = [[0u8; 2]; STATES];
    let mut output = [[[0u8; 1]; 2]; STATES];
    let mut state = 0;
    while state < STATES {
        let mut input = 0;
        while input < 2 {
            let register = state << 1 | input;
            let mut parity = 0;
            let mut tap = 0;
            while tap < TAPS.len() {
                parity ^= register >> TAPS[tap] & 1;
                tap += 1;
            }
            next[state][input] = (register % STATES) as u8;
            output[state][input] = [(input << 1 | parity) as u8];
            input += 1;
        }
        state += 1;
    }
    TrellisCode::new(next, output)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    pub(crate) op: u8,
    pub(crate) arg: u8,
    pub(crate) unit: u16,
    pub(crate) repaired: u32,
}

/// The ops whose packet runs on into a second block.
pub(crate) const fn is_double(op: u8) -> bool {
    matches!(op, 0x35 | 0x55)
}

fn deinterleave(soft: &[f32; BLOCK_BITS]) -> [f32; BLOCK_BITS] {
    std::array::from_fn(|n| soft[(n % ROWS) * COLUMNS + n / ROWS])
}

#[cfg(test)]
fn parity_of(data: &[bool; DATA_BITS], n: usize) -> bool {
    TAPS.iter()
        .filter(|&&delay| n >= delay)
        .fold(false, |acc, &delay| acc ^ data[n - delay])
}

/// The data bits of the likeliest path through the code, and how many sent bits it overturns.
fn viterbi(line: &[f32; BLOCK_BITS]) -> ([bool; DATA_BITS], u32) {
    let soft = |value: f32| -> Soft { SoftBit(value).to_fec() };
    let received: [[[Soft; 2]; 1]; DATA_BITS] =
        std::array::from_fn(|n| [[soft(line[n]), soft(line[DATA_BITS + n])]]);
    let mut inputs = [0u8; DATA_BITS];
    let overturned = CODE.decode_soft(&received, 0, &mut inputs);
    (inputs.map(|input| input == 1), u32::from(overturned))
}

fn bytes(bits: &[bool; DATA_BITS]) -> [u8; DATA_BYTES] {
    std::array::from_fn(|i| (0..8).fold(0u8, |byte, j| byte | (u8::from(bits[i * 8 + j]) << j)))
}

fn crc(header: &[u8]) -> u16 {
    !sdrmm_dsp::crc16_ccitt(header)
}

/// The block's header when its CRC holds after soft Viterbi decoding; `soft` holds the sent
/// bits, positive for a one.
pub(crate) fn decode(soft: &[f32; BLOCK_BITS]) -> Option<Block> {
    let (data, repaired) = viterbi(&deinterleave(soft));
    let data = bytes(&data);
    (crc(&data[..4]) == u16::from_le_bytes([data[4], data[5]])).then(|| Block {
        op: data[0],
        arg: data[1],
        unit: u16::from_be_bytes([data[2], data[3]]),
        repaired,
    })
}

/// The 112 transmitted bits of a block, the inverse of [`decode`].
#[cfg(test)]
pub(crate) fn encode(op: u8, arg: u8, unit: u16) -> [bool; BLOCK_BITS] {
    let [high, low] = unit.to_be_bytes();
    let [crc_low, crc_high] = crc(&[op, arg, high, low]).to_le_bytes();
    let header = [op, arg, high, low, crc_low, crc_high, 0];
    let data: [bool; DATA_BITS] = std::array::from_fn(|n| header[n / 8] >> (n % 8) & 1 == 1);
    let line: [bool; BLOCK_BITS] = std::array::from_fn(|n| {
        if n < DATA_BITS {
            data[n]
        } else {
            parity_of(&data, n - DATA_BITS)
        }
    });
    let mut sent = [false; BLOCK_BITS];
    for (n, &bit) in line.iter().enumerate() {
        sent[(n % ROWS) * COLUMNS + n / ROWS] = bit;
    }
    sent
}

/// The sent bits as sure soft values.
#[cfg(test)]
pub(crate) fn sure(bits: &[bool; BLOCK_BITS]) -> [f32; BLOCK_BITS] {
    bits.map(|bit| if bit { 1.0 } else { -1.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_round_trips() {
        let block = decode(&sure(&encode(0x01, 0x80, 0x1234)));
        assert_eq!(
            block,
            Some(Block {
                op: 0x01,
                arg: 0x80,
                unit: 0x1234,
                repaired: 0
            })
        );
    }

    #[test]
    fn the_crc_matches_the_reference_encoder() {
        assert_eq!(crc(&[0x01, 0x80, 0x12, 0x34]), 0x3E2E);
        assert_eq!(crc(&[0x01, 0x00, 0x17, 0x77]), 0x3CE5);
    }

    #[test]
    fn the_code_repairs_any_one_flipped_header_bit() {
        let sent = encode(0x46, 0x03, 0x0815);
        for position in 0..48 {
            let mut bits = sent;
            let at = (position % ROWS) * COLUMNS + position / ROWS;
            bits[at] = !bits[at];
            let block = decode(&sure(&bits));
            assert!(
                matches!(
                    block,
                    Some(Block {
                        op: 0x46,
                        arg: 0x03,
                        unit: 0x0815,
                        repaired: 1
                    })
                ),
                "bit {position}: {block:?}"
            );
        }
    }

    #[test]
    fn a_burst_too_damaged_to_repair_fails_its_crc() {
        let mut bits = encode(0x01, 0x00, 0x1777);
        for bit in bits.iter_mut().take(40) {
            *bit = !*bit;
        }
        assert_eq!(decode(&sure(&bits)), None);
    }

    #[test]
    fn weak_errors_past_the_hard_repair_are_overturned() {
        let sent = encode(0x46, 0x03, 0x0815);
        let mut soft = sure(&sent);
        let burst = (10..14).flat_map(|n| [n, DATA_BITS + n]);
        for n in burst {
            soft[(n % ROWS) * COLUMNS + n / ROWS] *= -0.2;
        }
        let hard = soft.map(f32::signum);
        assert_ne!(decode(&hard).map(|block| block.unit), Some(0x0815));
        let block = decode(&soft).expect("repaired");
        assert_eq!((block.op, block.arg, block.unit), (0x46, 0x03, 0x0815));
        assert_eq!(block.repaired, 8);
    }
}
