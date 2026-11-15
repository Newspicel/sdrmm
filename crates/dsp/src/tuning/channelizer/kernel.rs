use super::planes::Planes;

/// Sums each of the `BINS` polyphase branches of `taps` against the window into `partial`.
pub(super) fn fir_fixed<const BINS: usize>(
    taps: &[f32],
    window_re: &[f32],
    window_im: &[f32],
    partial: &mut Planes,
) {
    let mut re = [0.0f32; BINS];
    let mut im = [0.0f32; BINS];
    let (taps, _) = taps.as_chunks::<BINS>();
    let (window_re, _) = window_re.as_chunks::<BINS>();
    let (window_im, _) = window_im.as_chunks::<BINS>();
    for ((tap, sample_re), sample_im) in taps.iter().zip(window_re).zip(window_im) {
        for slot in 0..BINS {
            re[slot] += tap[slot] * sample_re[slot];
            im[slot] += tap[slot] * sample_im[slot];
        }
    }
    partial.re.copy_from_slice(&re);
    partial.im.copy_from_slice(&im);
}

/// [`fir_fixed`] for any bin count.
pub(super) fn fir_any(
    bins: usize,
    taps: &[f32],
    window_re: &[f32],
    window_im: &[f32],
    partial: &mut Planes,
) {
    partial.re.fill(0.0);
    partial.im.fill(0.0);
    for ((tap, re), im) in taps
        .chunks_exact(bins)
        .zip(window_re.chunks_exact(bins))
        .zip(window_im.chunks_exact(bins))
    {
        for (((acc_re, acc_im), &tap), (&re, &im)) in partial
            .re
            .iter_mut()
            .zip(partial.im.iter_mut())
            .zip(tap)
            .zip(re.iter().zip(im))
        {
            *acc_re += tap * re;
            *acc_im += tap * im;
        }
    }
}

/// Turns the branch sums into the wanted bins through one twiddle row per branch.
pub(super) fn rotate_fixed<const BINS: usize, const PADDED: usize>(
    partial: &Planes,
    table: &Planes,
    sums: &mut Planes,
) {
    let mut sum_re = [0.0f32; PADDED];
    let mut sum_im = [0.0f32; PADDED];
    let (rows_re, _) = table.re.as_chunks::<PADDED>();
    let (rows_im, _) = table.im.as_chunks::<PADDED>();
    for slot in 0..BINS {
        let (a, b) = (partial.re[slot], partial.im[slot]);
        let (c, d) = (&rows_re[slot], &rows_im[slot]);
        for lane in 0..PADDED {
            sum_re[lane] += a * c[lane] - b * d[lane];
            sum_im[lane] += a * d[lane] + b * c[lane];
        }
    }
    sums.re.copy_from_slice(&sum_re);
    sums.im.copy_from_slice(&sum_im);
}

/// [`rotate_fixed`] for any bin and lane count.
pub(super) fn rotate(partial: &Planes, table: &Planes, padded: usize, sums: &mut Planes) {
    sums.re.fill(0.0);
    sums.im.fill(0.0);
    for (((&a, &b), c), d) in partial
        .re
        .iter()
        .zip(&partial.im)
        .zip(table.re.chunks_exact(padded))
        .zip(table.im.chunks_exact(padded))
    {
        for (((sum_re, sum_im), &c), &d) in sums.re.iter_mut().zip(sums.im.iter_mut()).zip(c).zip(d)
        {
            *sum_re += a * c - b * d;
            *sum_im += a * d + b * c;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{super::planes::twiddle_table, *};

    fn ramp(len: usize, step: usize, modulus: usize) -> Vec<f32> {
        (0..len)
            .map(|k| ((k * step) % modulus) as f32 / modulus as f32 - 0.5)
            .collect()
    }

    fn assert_planes_close(a: &Planes, b: &Planes, tolerance: f32) {
        for (a, b) in a.re.iter().chain(&a.im).zip(b.re.iter().chain(&b.im)) {
            assert!((a - b).abs() < tolerance, "{a} vs {b}");
        }
    }

    #[test]
    fn the_fixed_and_general_filters_agree() {
        let (taps, re, im) = (ramp(192, 37, 101), ramp(192, 13, 29), ramp(192, 7, 31));
        let mut fixed = Planes::zeroed(12);
        let mut general = Planes::zeroed(12);
        fir_fixed::<12>(&taps, &re, &im, &mut fixed);
        fir_any(12, &taps, &re, &im, &mut general);
        assert_planes_close(&fixed, &general, 1e-5);
    }

    #[test]
    fn the_fixed_and_general_rotations_agree() {
        let partial = Planes {
            re: (0..12).map(|k| k as f32 * 0.1 - 0.5).collect(),
            im: (0..12).map(|k| 0.3 - k as f32 * 0.05).collect(),
        };
        let table = twiddle_table(12, &[-5, -2, 0, 1, 3, 4, 5, -1, -3, 2]).swap_remove(7);
        let mut fixed = Planes::zeroed(12);
        let mut general = Planes::zeroed(12);
        rotate_fixed::<12, 12>(&partial, &table, &mut fixed);
        rotate(&partial, &table, 12, &mut general);
        assert_planes_close(&fixed, &general, 1e-5);
    }

    #[test]
    fn ten_bins_match_the_general_path() {
        let (taps, re, im) = (ramp(160, 37, 101), ramp(160, 13, 29), ramp(160, 7, 31));
        let mut fixed = Planes::zeroed(10);
        let mut general = Planes::zeroed(10);
        fir_fixed::<10>(&taps, &re, &im, &mut fixed);
        fir_any(10, &taps, &re, &im, &mut general);
        let table = twiddle_table(10, &[-4, -3, -2, -1, 0, 1, 2, 3, 4]).swap_remove(3);
        let mut rotated = Planes::zeroed(12);
        let mut reference = Planes::zeroed(12);
        rotate_fixed::<10, 12>(&fixed, &table, &mut rotated);
        rotate(&general, &table, 12, &mut reference);
        assert_planes_close(&rotated, &reference, 1e-4);
    }
}
