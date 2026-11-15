use num_complex::Complex;

use super::subcarrier::SubcarrierMap;

pub fn interpolate(map: &SubcarrierMap, known: &[bool], h: &mut [Complex<f32>]) {
    let occupied = map.occupied();
    let Some(first) = occupied.iter().position(|c| known[c.bin]) else {
        return;
    };
    let Some(last) = occupied.iter().rposition(|c| known[c.bin]) else {
        return;
    };
    for i in 0..first {
        h[occupied[i].bin] = h[occupied[first].bin];
    }
    for i in last + 1..occupied.len() {
        h[occupied[i].bin] = h[occupied[last].bin];
    }
    let mut lo = first;
    while lo < last {
        let hi = (lo + 1..=last)
            .find(|&j| known[occupied[j].bin])
            .unwrap_or(last);
        let (a, b) = (occupied[lo], occupied[hi]);
        let span = f64::from(b.offset - a.offset);
        for c in &occupied[lo + 1..hi] {
            let t = (f64::from(c.offset - a.offset) / span) as f32;
            h[c.bin] = h[a.bin] * (1.0 - t) + h[b.bin] * t;
        }
        lo = hi;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ofdm::OfdmParams;

    fn map() -> SubcarrierMap {
        OfdmParams::wifi_like().map().clone()
    }

    #[test]
    fn interpolation_reproduces_a_linear_channel_exactly() {
        let map = map();
        let truth = |offset: i32| Complex::new(1.0 + 0.01 * offset as f32, 0.02 * offset as f32);
        let mut h = vec![Complex::new(0.0, 0.0); map.fft()];
        let mut known = vec![false; map.fft()];
        for c in map.occupied().iter().filter(|c| c.offset % 4 == 0) {
            h[c.bin] = truth(c.offset);
            known[c.bin] = true;
        }
        interpolate(&map, &known, &mut h);
        for c in map.occupied().iter().filter(|c| c.offset.abs() <= 24) {
            let want = truth(c.offset);
            assert!(
                (h[c.bin] - want).norm() < 1e-5,
                "bin {} ({}): {:?} vs {want:?}",
                c.bin,
                c.offset,
                h[c.bin]
            );
        }
    }

    #[test]
    fn interpolation_extends_flat_past_the_outermost_anchor() {
        let map = map();
        let mut h = vec![Complex::new(0.0, 0.0); map.fft()];
        let mut known = vec![false; map.fft()];
        for c in map.occupied().iter().filter(|c| c.offset.abs() <= 4) {
            h[c.bin] = Complex::new(c.offset as f32, 0.0);
            known[c.bin] = true;
        }
        interpolate(&map, &known, &mut h);
        let edge = |offset: i32| {
            h[map
                .occupied()
                .iter()
                .find(|c| c.offset == offset)
                .unwrap()
                .bin]
        };
        assert!((edge(26) - edge(4)).norm() < 1e-6);
        assert!((edge(-26) - edge(-4)).norm() < 1e-6);
        let mut empty = vec![Complex::new(0.0, 0.0); map.fft()];
        interpolate(&map, &vec![false; map.fft()], &mut empty);
        assert!(empty.iter().all(|v| *v == Complex::new(0.0, 0.0)));
    }
}
