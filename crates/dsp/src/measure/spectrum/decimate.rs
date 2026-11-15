use super::POWER_EPSILON;
use crate::math::fastmath::{fast_db_to_power, fast_power_db};

/// Shrinks a dB spectrum to `out.len()` bins. A group holding a signal keeps its peak so a
/// carrier survives; a group of noise keeps its mean power so the floor stays where it was.
pub fn decimate_signal(db: &[f32], signal_db: f32, out: &mut [f32]) {
    let bins = out.len();
    assert!(bins > 0, "need at least one output bin");
    if db.is_empty() {
        out.fill(f32::NEG_INFINITY);
        return;
    }
    let len = db.len();
    if bins >= len {
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = db[(i * len / bins).min(len - 1)];
        }
        return;
    }
    for (i, slot) in out.iter_mut().enumerate() {
        let start = i * len / bins;
        let end = ((i + 1) * len / bins).max(start + 1).min(len);
        *slot = group_level(&db[start..end], signal_db);
    }
}

fn group_level(group: &[f32], signal_db: f32) -> f32 {
    let mut peak = f32::NEG_INFINITY;
    let mut power = 0.0f32;
    for &v in group {
        peak = peak.max(v);
        power += fast_db_to_power(v);
    }
    if peak >= signal_db {
        peak
    } else {
        fast_power_db(power / group.len() as f32 + POWER_EPSILON)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        super::{SpanFloor, floor::tests::exponential_power_db},
        *,
    };

    #[test]
    fn decimation_keeps_a_carrier_peak() {
        let mut db = vec![-100.0f32; 100];
        db[37] = -3.0;
        let mut out = vec![0.0f32; 10];
        decimate_signal(&db, -90.0, &mut out);
        assert_eq!(out[3], -3.0);
    }

    #[test]
    fn decimation_keeps_the_noise_floor_where_it_was() {
        let noise = exponential_power_db(4_096, -80.0, 0xD1CE);
        let floor = SpanFloor::default().read(&noise).unwrap();
        let mut out = vec![0.0f32; 256];
        decimate_signal(&noise, floor + 10.0, &mut out);
        let mean = out.iter().sum::<f32>() / out.len() as f32;
        assert!((mean - -80.0).abs() < 1.0, "decimated floor read {mean}");
    }
}
