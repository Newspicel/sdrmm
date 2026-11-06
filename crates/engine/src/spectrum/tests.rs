use super::*;
use crate::runtime::spectrum_size;

const RATES: [f64; 7] = [
    48_000.0,
    2_400_000.0,
    10_000_000.0,
    20_000_000.0,
    61_440_000.0,
    122_880_000.0,
    245_760_000.0,
];

#[test]
fn each_frame_averages_what_fits_its_period_within_the_budget() {
    for rate in RATES {
        let size = spectrum_size(rate);
        let cadence = Cadence::new(rate, size, CPU_BUDGET);
        let transforms_per_second = rate / cadence.hop as f64;
        assert!(
            transforms_per_second * size as f64 <= CPU_BUDGET * 1.001,
            "{rate}: {transforms_per_second} transforms/s of {size}"
        );
        if cadence.averages > 1 {
            let period = (cadence.hop as f64 * f64::from(cadence.averages)) / rate;
            let slack = cadence.hop as f64 / rate / 2.0;
            assert!(
                (period - 1.0 / TARGET_FPS).abs() <= slack + 1e-9,
                "{rate}: {period} s"
            );
        }
    }
}

#[test]
fn the_cpu_overlaps_every_sample_below_the_widest_rates() {
    for rate in &RATES[..5] {
        let size = spectrum_size(*rate);
        let cadence = Cadence::new(*rate, size, CPU_BUDGET);
        assert!(cadence.hop <= size / 2, "{rate}: hop {}", cadence.hop);
    }
    let widest = Cadence::new(245_760_000.0, 262_144, CPU_BUDGET);
    assert_eq!(widest.averages, 16, "keeps what the CPU always afforded");
}

#[test]
fn a_budget_for_every_sample_overlaps_the_widest_rate() {
    let cadence = Cadence::new(245_760_000.0, 262_144, 600e6);
    assert!(cadence.hop <= 262_144 / 2, "hop {}", cadence.hop);
    assert_eq!(cadence.averages, MAX_AVERAGES);
}

#[test]
fn slow_rates_keep_the_frame_rate_with_a_quarter_window_hop() {
    assert_eq!(
        Cadence::new(48_000.0, 4_096, CPU_BUDGET),
        Cadence {
            hop: 1_024,
            averages: 2
        }
    );
    assert_eq!(Cadence::new(8_000.0, 4_096, CPU_BUDGET).averages, 1);
}

#[test]
fn gpu_selection_only_targets_expensive_workloads() {
    assert!(!should_use_gpu(4096, 1));
    assert!(should_use_gpu(GPU_MIN_FFT_SIZE, 1));
    assert!(should_use_gpu(262_144, 1));
    assert!(!should_use_gpu(4096, 4));
    assert!(!should_use_gpu(16_384, 4));
    assert!(!should_use_gpu(12_345, 4));
}

#[cfg(feature = "gpu-fft")]
fn dithered_tone(len: usize) -> Vec<Complex<f32>> {
    let mut noise = 0x1234_5678_u32;
    (0..len)
        .map(|index| {
            noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let phase = std::f32::consts::TAU * 117.3 * index as f32 / 1_024.0;
            let dither = (noise as f32 / u32::MAX as f32 - 0.5) * 0.02;
            Complex::from_polar(0.75, phase) + Complex::new(dither, -0.5 * dither)
        })
        .collect()
}

#[cfg(feature = "gpu-fft")]
fn reference_frames(
    samples: &[Complex<f32>],
    size: usize,
    cadence: Cadence,
    origin: u64,
) -> Vec<(u64, Vec<f32>)> {
    let mut analyzer = sdrmm_dsp::SpectrumAnalyzer::with_window(display_window(size));
    let transforms = (samples.len() - size) / cadence.hop + 1;
    let starts: Vec<_> = (0..transforms).map(|j| j * cadence.hop).collect();
    starts
        .chunks(cadence.averages as usize)
        .map(|frame| {
            let mut average = sdrmm_dsp::PowerAverage::new(size);
            for &start in frame {
                analyzer.accumulate(&samples[start..start + size], &mut average);
            }
            let mut db = vec![0.0; size];
            average.take_db(&mut db);
            let last = frame[frame.len() - 1];
            (origin + (last + size) as u64, db)
        })
        .collect()
}

#[cfg(feature = "gpu-fft")]
#[test]
#[ignore = "CI runs this explicitly with a guaranteed headless adapter"]
fn gpu_matches_cpu_when_a_hardware_adapter_is_available() {
    let context = match gpu::Context::new_for_test().map(Arc::new) {
        Ok(context) => context,
        Err(error) if std::env::var_os("SDRMM_REQUIRE_GPU_FFT_TEST").is_some() => {
            panic!("required GPU FFT test adapter is unavailable: {error}")
        }
        Err(_) => return,
    };
    let size = 1_024;
    let cadence = Cadence {
        hop: 300,
        averages: 7,
    };
    let tuning = Tuning {
        center_hz: 100e6,
        span_hz: 2_400_000.0,
    };
    let origin = 5_000;
    let samples = dithered_tone(300 * 7 * 4 + size + 500);
    let expected = reference_frames(&samples, size, cadence, origin);
    assert_eq!(expected.len(), 5, "four whole frames and a partial one");

    let mut stream = gpu::spectrum::Stream::with_batch(context, size, 3).expect("GPU stream");
    let actual = std::cell::RefCell::new(Vec::new());
    let mut publish = |frame: SpectrumFrame, db: &[f32]| {
        assert_eq!(frame.center_hz, tuning.center_hz);
        actual.borrow_mut().push((frame.timestamp, db.to_vec()));
    };
    for (block, chunk) in samples.chunks(777).enumerate() {
        let index = origin + (block * 777) as u64;
        stream
            .push(chunk, index, tuning, cadence, &mut publish)
            .expect("push");
    }
    stream.restart();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while actual.borrow().len() < expected.len() {
        assert!(std::time::Instant::now() < deadline, "GPU frames missing");
        stream.idle(&mut publish).expect("GPU spectrum");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    let actual = actual.into_inner();
    assert_eq!(actual.len(), expected.len());
    for ((stamp, gpu), (want_stamp, cpu)) in actual.iter().zip(&expected) {
        assert_eq!(stamp, want_stamp);
        for (bin, (&cpu, &gpu)) in cpu.iter().zip(gpu).enumerate() {
            assert!(
                (cpu - gpu).abs() < 0.1,
                "frame {stamp} bin {bin}: CPU {cpu} dB, GPU {gpu} dB"
            );
        }
    }
}
