use num_complex::Complex;

/// `len` complex samples of uniform noise in `[-amp, amp]` on each rail, repeatable by `seed`.
#[must_use]
pub fn complex_noise(seed: u32, amp: f32, len: usize) -> Vec<Complex<f32>> {
    let mut state = seed | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * amp
    };
    (0..len).map(|_| Complex::new(next(), next())).collect()
}
