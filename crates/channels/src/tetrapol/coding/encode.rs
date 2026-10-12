use sdrmm_wire::TetrapolBand;

use super::{
    BLOCK_BYTES, DATA_CRC, DATA_FIELD_BITS, DataBlock, FIRST_CODED, FIRST_STEPS, FRAME_BITS,
    PAYLOAD_BITS, PRECODED, SECOND_STEPS, SYNC, SYNC_BITS, VOICE_CRC, branch, interleaved_position,
    scrambling_bit, tail_start,
};

pub(super) fn encode_pairs(bits: &[u8], start: usize, out: &mut [u8]) {
    let mut state = start;
    for (step, &bit) in bits.iter().enumerate() {
        let (o0, o1, next) = branch(state, usize::from(bit));
        out[2 * step] = o0;
        out[2 * step + 1] = o1;
        state = next;
    }
}

pub(super) fn remainder_bits(bits: &[u8], generator: u32, out: &mut [u8]) {
    let degree = 31 - generator.leading_zeros();
    let mask = (1u32 << degree) - 1;
    let mut register = 0u32;
    for &bit in bits
        .iter()
        .chain(std::iter::repeat_n(&0u8, degree as usize))
    {
        let top = (register >> (degree - 1)) & 1;
        register = ((register << 1) | u32::from(bit)) & mask;
        if top == 1 {
            register ^= generator & mask;
        }
    }
    for (k, slot) in out.iter_mut().enumerate() {
        *slot = ((register >> (degree as usize - 1 - k)) & 1) as u8;
    }
}

pub(crate) fn encode_data(block: &DataBlock, band: TetrapolBand) -> [u8; PAYLOAD_BITS] {
    let mut field = [0u8; DATA_FIELD_BITS];
    field[0] = 1;
    field[1] = block.flag & 1;
    field[2] = block.flag >> 1 & 1;
    for k in 0..BLOCK_BYTES * 8 {
        field[3 + k] = block.octets[k / 8] >> (k % 8) & 1;
    }
    field[67] = block.asb[0];
    field[68] = block.asb[1];
    let (message, crc) = field.split_at_mut(DATA_FIELD_BITS - 5);
    remainder_bits(message, DATA_CRC, crc);
    let mut second = [0u8; SECOND_STEPS];
    second[..DATA_FIELD_BITS - FIRST_STEPS].copy_from_slice(&field[FIRST_STEPS..]);
    let mut first = [0u8; FIRST_STEPS];
    first.copy_from_slice(&field[..FIRST_STEPS]);
    interleave(&first, &second, band)
}

pub(crate) fn encode_voice(
    payload: &[u8; 22],
    unprotected: &[u8; 100],
    band: TetrapolBand,
) -> [u8; PAYLOAD_BITS] {
    let mut first = [0u8; FIRST_STEPS];
    first[1..23].copy_from_slice(&payload[..22]);
    first[0] = 0;
    let (message, crc) = first.split_at_mut(23);
    remainder_bits(message, VOICE_CRC, crc);
    for bit in crc.iter_mut() {
        *bit ^= 1;
    }
    let mut coded = [0u8; PAYLOAD_BITS];
    encode_pairs(&first, tail_start(&first), &mut coded[..FIRST_CODED]);
    coded[FIRST_CODED..].copy_from_slice(unprotected);
    scatter(&coded, band)
}

fn interleave(
    first: &[u8; FIRST_STEPS],
    second: &[u8; SECOND_STEPS],
    band: TetrapolBand,
) -> [u8; PAYLOAD_BITS] {
    let mut coded = [0u8; PAYLOAD_BITS];
    encode_pairs(first, tail_start(first), &mut coded[..FIRST_CODED]);
    encode_pairs(second, 0, &mut coded[FIRST_CODED..]);
    scatter(&coded, band)
}

fn scatter(coded: &[u8; PAYLOAD_BITS], band: TetrapolBand) -> [u8; PAYLOAD_BITS] {
    let mut e = [0u8; PAYLOAD_BITS];
    for (j, &bit) in coded.iter().enumerate() {
        e[interleaved_position(band, j)] = bit;
    }
    e
}

pub(crate) fn frame_bits(e: &[u8; PAYLOAD_BITS], band: TetrapolBand, code: u8) -> [u8; FRAME_BITS] {
    let mut f = [0u8; FRAME_BITS];
    f[..SYNC_BITS].copy_from_slice(&SYNC);
    let mut precoded = [0u8; PAYLOAD_BITS];
    for j in 0..PAYLOAD_BITS {
        precoded[j] = match band {
            TetrapolBand::Vhf => e[j],
            TetrapolBand::Uhf if j == 0 => e[0] ^ SYNC[SYNC_BITS - 1],
            TetrapolBand::Uhf if PRECODED[j] => e[j] ^ precoded[j - 2],
            TetrapolBand::Uhf => e[j] ^ precoded[j - 1],
        };
        f[SYNC_BITS + j] = precoded[j] ^ scrambling_bit(code, j);
    }
    f
}
