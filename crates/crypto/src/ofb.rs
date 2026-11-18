/// Fills `out` with output feedback keystream: the block cipher run on its own output from `iv`.
pub fn ofb<const N: usize>(encrypt: impl Fn(&mut [u8; N]), iv: [u8; N], out: &mut [u8]) {
    let mut block = iv;
    for chunk in out.chunks_mut(N) {
        encrypt(&mut block);
        chunk.copy_from_slice(&block[..chunk.len()]);
    }
}
