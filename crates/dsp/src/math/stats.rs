/// The upper median: the middle value of an odd count, the higher middle of an even one.
/// Reorders `values` in place.
#[must_use]
pub fn median(values: &mut [f32]) -> Option<f32> {
    let middle = values.len() / 2;
    (!values.is_empty()).then(|| *values.select_nth_unstable_by(middle, f32::total_cmp).1)
}

/// The `fraction` quantile of `values`, rounding the rank down. Reorders `values` in place.
#[must_use]
pub fn quantile(values: &mut [f32], fraction: f32) -> Option<f32> {
    let last = values.len().checked_sub(1)?;
    let rank = (last as f32 * fraction.clamp(0.0, 1.0)) as usize;
    Some(*values.select_nth_unstable_by(rank, f32::total_cmp).1)
}

/// The `fraction` quantile of the finite values in `values`, sorted in `scratch`.
pub fn finite_quantile(values: &[f32], scratch: &mut Vec<f32>, fraction: f32) -> Option<f32> {
    scratch.clear();
    scratch.extend(values.iter().copied().filter(|v| v.is_finite()));
    quantile(scratch, fraction)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_of_an_even_count_is_the_upper_middle() {
        assert_eq!(median(&mut []), None);
        assert_eq!(median(&mut [9.0]), Some(9.0));
        assert_eq!(median(&mut [3.0, -1.0, 7.0, 0.5, 2.0]), Some(2.0));
        assert_eq!(median(&mut [4.0, 1.0, 3.0, 2.0]), Some(3.0));
    }

    #[test]
    fn a_quantile_rounds_its_rank_down() {
        assert_eq!(quantile(&mut [], 0.5), None);
        assert_eq!(quantile(&mut [4.0, 1.0, 3.0, 2.0], 0.5), Some(2.0));
        assert_eq!(quantile(&mut [4.0, 1.0, 3.0, 2.0], 1.0), Some(4.0));
    }

    #[test]
    fn a_finite_quantile_skips_values_that_are_not_finite() {
        let mut scratch = Vec::new();
        let db = [f32::NAN, -10.0, -30.0, f32::NEG_INFINITY, -20.0];
        assert_eq!(finite_quantile(&db, &mut scratch, 0.0), Some(-30.0));
        assert_eq!(finite_quantile(&db, &mut scratch, 1.0), Some(-10.0));
        assert_eq!(finite_quantile(&[f32::NAN], &mut scratch, 0.5), None);
    }
}
