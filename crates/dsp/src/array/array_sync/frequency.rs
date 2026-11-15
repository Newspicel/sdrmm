/// Signed frequency of FFT bin `bin` of `len`, in cycles per sample within [-0.5, 0.5).
#[must_use]
pub(super) fn bin_frequency(bin: usize, len: usize) -> f64 {
    if bin < len.div_ceil(2) {
        bin as f64 / len as f64
    } else {
        (bin as f64 - len as f64) / len as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upper_bins_are_negative_frequencies() {
        assert_eq!(bin_frequency(0, 8), 0.0);
        assert_eq!(bin_frequency(3, 8), 0.375);
        assert_eq!(bin_frequency(4, 8), -0.5);
        assert_eq!(bin_frequency(7, 8), -0.125);
        assert_eq!(bin_frequency(2, 5), 0.4);
        assert_eq!(bin_frequency(3, 5), -0.4);
    }
}
