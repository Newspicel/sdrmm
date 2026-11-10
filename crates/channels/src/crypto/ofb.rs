/// Fills `out` with output feedback keystream: the block cipher run on its own output from `iv`.
pub(crate) fn ofb<const N: usize>(encrypt: impl Fn(&mut [u8; N]), iv: [u8; N], out: &mut [u8]) {
    let mut block = iv;
    for chunk in out.chunks_mut(N) {
        encrypt(&mut block);
        chunk.copy_from_slice(&block[..chunk.len()]);
    }
}

/// One step of the 64-bit message indicator LFSR of P25 and NXDN (x^64 + x^62 + x^46 + x^38 +
/// x^27 + x^15 + 1) clocked 64 times: the next superframe's MI when its own was lost, and the low
/// half of a 128-bit AES IV.
pub(crate) fn next_message_indicator(mi: u64) -> u64 {
    (0..64).fold(mi, |lfsr, _| {
        let bit = (lfsr >> 63 ^ lfsr >> 61 ^ lfsr >> 45 ^ lfsr >> 37 ^ lfsr >> 26 ^ lfsr >> 14) & 1;
        lfsr << 1 | bit
    })
}

/// The 128-bit AES IV a 64-bit message indicator stands for: the MI and the LFSR's next 64 bits.
pub(crate) fn aes_iv(mi: u64) -> [u8; 16] {
    (u128::from(mi) << 64 | u128::from(next_message_indicator(mi))).to_be_bytes()
}
