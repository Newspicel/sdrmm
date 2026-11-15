/// Remainder of `word`, read as a polynomial over GF(2) of `code_bits` terms, divided by a
/// generator of degree `check_bits`: the check field of a systematic cyclic code.
pub(super) const fn remainder(word: u32, generator: u32, code_bits: u32, check_bits: u32) -> u32 {
    let mut rem = word & ((1 << code_bits) - 1);
    let mut shift = code_bits;
    while shift > check_bits {
        shift -= 1;
        if rem & (1 << shift) != 0 {
            rem ^= generator << (shift - check_bits);
        }
    }
    rem
}
