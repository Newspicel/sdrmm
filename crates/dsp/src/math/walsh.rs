//! The fast Walsh-Hadamard transform.

/// Transforms `values` in place, unnormalized, so applying it twice scales by the length.
/// The length must be a power of two; any other length is left untouched.
///
/// Over GF(2^m), the distribution of a sum of independent symbols is the inverse transform of
/// the product of their transforms, which is what message passing on non-binary codes uses.
pub fn walsh_hadamard(values: &mut [f32]) {
    let len = values.len();
    if !len.is_power_of_two() {
        return;
    }
    let mut half = 1;
    while half < len {
        for block in values.chunks_exact_mut(2 * half) {
            let (low, high) = block.split_at_mut(half);
            for (a, b) in low.iter_mut().zip(high) {
                let (sum, difference) = (*a + *b, *a - *b);
                *a = sum;
                *b = difference;
            }
        }
        half *= 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twice_is_the_identity_scaled_by_the_length() {
        let original: Vec<f32> = (0..64).map(|i| (i as f32 * 0.37).sin()).collect();
        let mut values = original.clone();
        walsh_hadamard(&mut values);
        walsh_hadamard(&mut values);
        for (twice, once) in values.iter().zip(&original) {
            assert!((twice - 64.0 * once).abs() < 1e-3);
        }
    }

    #[test]
    fn products_of_transforms_add_symbols_without_carry() {
        let mut a = [0.0f32; 8];
        let mut b = [0.0f32; 8];
        a[5] = 1.0;
        b[3] = 1.0;
        walsh_hadamard(&mut a);
        walsh_hadamard(&mut b);
        let mut sum: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x * y).collect();
        walsh_hadamard(&mut sum);
        let peak = sum
            .iter()
            .enumerate()
            .max_by(|x, y| x.1.total_cmp(y.1))
            .map(|(index, _)| index);
        assert_eq!(peak, Some(5 ^ 3));
    }
}
