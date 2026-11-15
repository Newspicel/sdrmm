use crate::math::rng::Rng;

pub(super) fn random_bits(len: usize, seed: u64) -> Vec<bool> {
    let mut rng = Rng::mixed(seed);
    (0..len).map(|_| rng.bits() & 1 == 1).collect()
}

pub(super) fn random_bytes(len: usize, seed: u64) -> Vec<u8> {
    let mut rng = Rng::mixed(seed);
    (0..len).map(|_| rng.bits() as u8).collect()
}

pub(super) fn random_byte_array<const N: usize>(seed: u64) -> [u8; N] {
    let mut rng = Rng::mixed(seed);
    std::array::from_fn(|_| rng.bits() as u8)
}

pub(super) fn flip_msb_first(frame: &mut [u8], bit: usize) {
    frame[bit / 8] ^= 0x80 >> (bit % 8);
}
