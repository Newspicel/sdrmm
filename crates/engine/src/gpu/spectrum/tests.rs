use std::sync::Arc;

use num_complex::Complex;
use sdrmm_dsp::PowerAverage;

use super::{Batch, Context, Transfer};

fn tone(len: usize) -> Vec<Complex<f32>> {
    let mut state = 0x2545_F491u32;
    let mut noise = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as f32 / u32::MAX as f32 - 0.5
    };
    (0..len)
        .map(|n| {
            let phase = 0.37 * n as f32;
            Complex::new(phase.cos(), phase.sin()) * 0.5 + Complex::new(noise(), noise()) * 0.01
        })
        .collect()
}

fn cpu_db(samples: &[Complex<f32>], size: usize, hop: usize, count: usize) -> Vec<f32> {
    let mut cpu = crate::spectrum::cpu_analyzer(size);
    let mut average = PowerAverage::new(size);
    for t in 0..count {
        cpu.accumulate(&samples[t * hop..t * hop + size], &mut average);
    }
    let mut db = vec![0.0; size];
    average.take_db(&mut db);
    db
}

#[test]
#[ignore = "requires a GPU adapter"]
fn staged_and_direct_uploads_match_the_cpu() {
    let context = Arc::new(Context::new_for_test().expect("GPU adapter"));
    let transfers: &[Transfer] = if context.unified() {
        &[Transfer::Staged, Transfer::Direct]
    } else {
        &[Transfer::Staged]
    };
    for &transfer in transfers {
        for (size, chunk) in [(1024, 8), (65_536, 4), (262_144, 2)] {
            let hop = size / 2 + 5;
            let count = chunk - 1;
            let samples = tone((count - 1) * hop + size);
            let mut batch = Batch::with_transfer(context.clone(), size, chunk, transfer).unwrap();
            let mut db = vec![0.0; size];
            for _ in 0..5 {
                batch.add(&samples, hop, count).unwrap();
            }
            batch.take_db(5 * count as u32, &mut db).unwrap();
            let want = cpu_db(&samples, size, hop, count);
            for (bin, (gpu, cpu)) in db.iter().zip(&want).enumerate() {
                assert!(
                    (gpu - cpu).abs() < 0.1,
                    "{transfer:?} {size} bin {bin}: GPU {gpu} dB, CPU {cpu} dB"
                );
            }
        }
    }
}
