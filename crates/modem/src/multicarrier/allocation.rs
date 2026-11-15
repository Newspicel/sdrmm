/// The signed subcarrier of allocation slot `index`: `allocated` slots centred on DC, DC skipped.
pub(super) fn signed_offset(index: usize, allocated: usize) -> isize {
    let offset = index as isize - (allocated / 2) as isize;
    if offset < 0 { offset } else { offset + 1 }
}

/// The transform bin of allocation slot `index` in a `size`-point transform.
pub(super) fn centred_bin(index: usize, allocated: usize, size: usize) -> usize {
    signed_offset(index, allocated).rem_euclid(size as isize) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_straddle_dc_and_skip_it() {
        let offsets: Vec<isize> = (0..4).map(|k| signed_offset(k, 4)).collect();
        assert_eq!(offsets, [-2, -1, 1, 2]);
        assert_eq!(centred_bin(0, 4, 16), 14);
        assert_eq!(centred_bin(3, 4, 16), 2);
    }
}
