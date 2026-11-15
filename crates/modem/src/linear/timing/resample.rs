use num_complex::Complex;
use sdrmm_dsp::farrow;

pub fn resample_at(
    filtered: &[Complex<f32>],
    sps: usize,
    offset: f64,
    out: &mut Vec<Complex<f32>>,
) {
    let mut position = offset;
    while position < 1.0 {
        position += sps as f64;
    }
    while (position as usize) + 2 < filtered.len() {
        match farrow_at(filtered, position) {
            Some(value) => out.push(value),
            None => return,
        }
        position += sps as f64;
    }
}

pub(super) fn farrow_at(filtered: &[Complex<f32>], position: f64) -> Option<Complex<f32>> {
    if position < 1.0 || position.is_nan() {
        return None;
    }
    let base = position as usize;
    let window = filtered.get(base - 1..base + 3)?;
    Some(farrow(window, (position - base as f64) as f32))
}
