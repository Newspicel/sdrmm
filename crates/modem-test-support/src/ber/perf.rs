use std::path::Path;

use num_complex::Complex;
use sdrmm_dsp::{RealDecimator, design_rrc};
use serde::{Deserialize, Serialize};

pub const REGRESSION_FRACTION: f64 = 0.10;

pub use sdrmm_test_support::{
    CountingAlloc, TIMED_BATCHES, assert_no_alloc, measure_allocs, measure_throughput,
};

pub const CALIBRATION_BENCH: &str = "__calibration";

pub const BASELINE_DOC: &str = "docs/src/development/building.md";

#[must_use]
pub fn calibrate() -> f64 {
    const LEN: usize = 8_192;
    let buffer: Vec<Complex<f32>> = (0..LEN)
        .map(|index| {
            let phase = index as f32 * 0.000_7;
            Complex::new(phase.cos(), phase.sin())
        })
        .collect();
    measure_throughput(256, LEN as u64, || {
        let mut acc = [Complex::new(0.0f32, 0.0); 4];
        for (index, value) in buffer.iter().enumerate() {
            acc[index & 3] += value * value.conj() + *value;
        }
        std::hint::black_box(acc);
    })
}

fn calibration_of(rows: &[PerfBaseline]) -> Option<f64> {
    rows.iter()
        .find(|row| row.bench == CALIBRATION_BENCH)
        .map(|row| row.msamples_per_s)
        .filter(|value| *value > 0.0)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PerfBaseline {
    pub bench: String,
    pub msamples_per_s: f64,
    pub realtime_factor: f64,
    pub config: String,
    pub host: String,
}

#[must_use]
pub fn host_id() -> String {
    format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)
}

#[must_use]
pub fn rows_for_host(rows: &[PerfBaseline]) -> Vec<PerfBaseline> {
    let host = host_id();
    rows.iter()
        .filter(|row| row.host == host)
        .cloned()
        .collect()
}

#[must_use]
pub fn missing_host_baseline(what: &str, record_command: &str) -> String {
    format!(
        "no committed {what} baseline for {host}, so the gate cannot run.\n\
         Record one on a quiet machine of this host and commit it:\n  {record_command}\n\
         Procedure: {BASELINE_DOC}, \"Performance baselines\".",
        host = host_id()
    )
}

#[must_use]
pub fn committed_for_host(
    rows: &[PerfBaseline],
    what: &str,
    record_command: &str,
) -> Vec<PerfBaseline> {
    let selected = rows_for_host(rows);
    assert!(
        !selected.is_empty(),
        "{}",
        missing_host_baseline(what, record_command)
    );
    selected
}

pub fn save_baselines(path: &Path, baselines: &[PerfBaseline]) -> std::io::Result<()> {
    let host = host_id();
    let mut rows: Vec<PerfBaseline> = load_baselines(path)
        .unwrap_or_default()
        .into_iter()
        .filter(|row| row.host != host)
        .collect();
    rows.extend(
        baselines
            .iter()
            .filter(|row| row.bench != CALIBRATION_BENCH)
            .cloned(),
    );
    rows.push(PerfBaseline {
        bench: CALIBRATION_BENCH.into(),
        msamples_per_s: calibrate(),
        realtime_factor: 0.0,
        config: "fixed reference kernel, scales the gate to the host's current speed".into(),
        host,
    });
    let mut json = serde_json::to_string_pretty(&rows).map_err(std::io::Error::other)?;
    json.push('\n');
    std::fs::write(path, json)
}

pub fn load_baselines(path: &Path) -> std::io::Result<Vec<PerfBaseline>> {
    serde_json::from_str(&std::fs::read_to_string(path)?).map_err(std::io::Error::other)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PerfChange {
    pub bench: String,
    pub committed_msamples_per_s: f64,
    pub measured_msamples_per_s: f64,
    pub change_fraction: f64,
}

pub fn compare_perf(
    measured: &[PerfBaseline],
    committed: &[PerfBaseline],
    regression_fraction: f64,
) -> Result<Vec<PerfChange>, Vec<PerfChange>> {
    let mut changes = Vec::new();
    let mut regressions = Vec::new();
    let scale = calibration_of(committed).map_or(1.0, |recorded| calibrate() / recorded);
    for reference in committed {
        if reference.bench == CALIBRATION_BENCH {
            continue;
        }
        let expected = reference.msamples_per_s * scale;
        let found = measured
            .iter()
            .find(|m| m.bench == reference.bench && m.config == reference.config);
        let change = match found {
            Some(m) if expected > 0.0 => PerfChange {
                bench: reference.bench.clone(),
                committed_msamples_per_s: expected,
                measured_msamples_per_s: m.msamples_per_s,
                change_fraction: (m.msamples_per_s - expected) / expected,
            },
            _ => PerfChange {
                bench: reference.bench.clone(),
                committed_msamples_per_s: expected,
                measured_msamples_per_s: 0.0,
                change_fraction: -1.0,
            },
        };
        if change.change_fraction < -regression_fraction {
            regressions.push(change.clone());
        }
        changes.push(change);
    }
    if regressions.is_empty() {
        Ok(changes)
    } else {
        Err(regressions)
    }
}

struct XorShift32(u32);

impl XorShift32 {
    fn seeded(seed: u32) -> Self {
        Self(seed | 1)
    }

    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
}

#[must_use]
pub fn test_dibits(len: usize, seed: u32) -> Vec<u8> {
    let mut rng = XorShift32::seeded(seed);
    (0..len).map(|_| (rng.next() & 3) as u8).collect()
}

#[must_use]
pub fn shaped_bpsk_iq(symbols: usize, sps: f64, seed: u32) -> Vec<Complex<f32>> {
    assert!(
        sps >= 2.0 && (sps - sps.round()).abs() < 1e-9,
        "need a whole number of samples per symbol, at least two"
    );
    let step = sps as usize;
    let taps = design_rrc(sps, 0.35, 8);
    let mut impulses = vec![0.0f32; symbols * step + taps.len()];
    let mut rng = XorShift32::seeded(seed);
    for i in 0..symbols {
        let sign = if rng.next() & 1 == 0 { 1.0 } else { -1.0 };
        impulses[i * step] = sign * sps as f32;
    }
    let mut shaped = Vec::new();
    RealDecimator::new(&taps, 1).process(&impulses, &mut shaped);
    shaped.iter().map(|&re| Complex::new(re, 0.0)).collect()
}

#[cfg(test)]
mod tests {
    use std::hint::black_box;

    use sdrmm_dsp::SymbolSync;

    use super::*;

    #[global_allocator]
    static ALLOC: CountingAlloc = CountingAlloc::new();

    #[test]
    fn counts_a_known_vec_allocation() {
        let allocs = measure_allocs(|| {
            black_box(Vec::<u8>::with_capacity(4096));
        });
        assert_eq!(allocs, 1, "one with_capacity is exactly one allocation");
    }

    #[test]
    fn pure_arithmetic_reads_zero() {
        let mut acc = 0.0f32;
        let allocs = measure_allocs(|| {
            for i in 0..10_000 {
                acc += (i as f32).sqrt();
            }
        });
        black_box(acc);
        assert_eq!(allocs, 0);
    }

    fn baseline(bench: &str, msamples_per_s: f64) -> PerfBaseline {
        PerfBaseline {
            bench: bench.into(),
            msamples_per_s,
            realtime_factor: msamples_per_s * 1e6 / 48_000.0,
            config: "test configuration".into(),
            host: host_id(),
        }
    }

    #[test]
    fn baselines_round_trip_through_json() {
        let path = std::env::temp_dir().join(format!(
            "sdrmm-modem-perf-roundtrip-{}.json",
            std::process::id()
        ));
        let committed = vec![
            baseline("cpm_demod_m4_48k", 21.5),
            baseline("symbol_sync_8sps", 84.25),
        ];
        save_baselines(&path, &committed).unwrap();
        let loaded = load_baselines(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let (calibration, rows): (Vec<PerfBaseline>, Vec<PerfBaseline>) = loaded
            .into_iter()
            .partition(|row| row.bench == CALIBRATION_BENCH);
        assert_eq!(rows, committed);
        assert_eq!(calibration.len(), 1);
        assert!(calibration[0].msamples_per_s > 0.0);
    }

    #[test]
    fn a_doctored_regression_fails_the_gate() {
        let committed = vec![baseline("cpm_demod_m4_48k", 100.0)];
        let measured = vec![baseline("cpm_demod_m4_48k", 85.0)];
        let regressions = compare_perf(&measured, &committed, REGRESSION_FRACTION).unwrap_err();
        assert_eq!(regressions.len(), 1);
        assert!((regressions[0].change_fraction + 0.15).abs() < 1e-12);
    }

    #[test]
    fn an_improvement_passes_and_is_reported() {
        let committed = vec![baseline("cpm_demod_m4_48k", 100.0)];
        let measured = vec![baseline("cpm_demod_m4_48k", 130.0)];
        let changes = compare_perf(&measured, &committed, REGRESSION_FRACTION).unwrap();
        assert_eq!(changes.len(), 1);
        assert!((changes[0].change_fraction - 0.30).abs() < 1e-12);
    }

    #[test]
    fn a_wobble_inside_the_tolerance_passes() {
        let committed = vec![baseline("cpm_demod_m4_48k", 100.0)];
        let measured = vec![baseline("cpm_demod_m4_48k", 91.0)];
        assert!(compare_perf(&measured, &committed, REGRESSION_FRACTION).is_ok());
    }

    fn calibration_row(msamples_per_s: f64) -> PerfBaseline {
        PerfBaseline {
            bench: CALIBRATION_BENCH.into(),
            msamples_per_s,
            realtime_factor: 0.0,
            config: "reference kernel".into(),
            host: host_id(),
        }
    }

    #[test]
    fn a_host_calibration_scales_the_expectation() {
        let committed = vec![
            baseline("cpm_demod_m4_48k", 100.0),
            calibration_row(2.0 * calibrate()),
        ];
        let halved = vec![baseline("cpm_demod_m4_48k", 55.0)];
        assert!(compare_perf(&halved, &committed, REGRESSION_FRACTION).is_ok());
        let slower = vec![baseline("cpm_demod_m4_48k", 40.0)];
        assert_eq!(
            compare_perf(&slower, &committed, REGRESSION_FRACTION)
                .unwrap_err()
                .len(),
            1
        );
    }

    #[test]
    fn a_foreign_host_row_is_never_selected() {
        let mut foreign = baseline("cpm_demod_m4_48k", 100.0);
        foreign.host = format!("{}-elsewhere", host_id());
        let rows = vec![foreign, baseline("symbol_sync_8sps", 400.0)];
        let selected = rows_for_host(&rows);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].bench, "symbol_sync_8sps");
    }

    #[test]
    fn recording_a_baseline_keeps_the_rows_of_other_hosts() {
        let path = std::env::temp_dir().join(format!(
            "sdrmm-modem-perf-hosts-{}.json",
            std::process::id()
        ));
        let mut foreign = baseline("symbol_sync_8sps", 11.0);
        foreign.host = "sparc-plan9".into();
        save_baselines(&path, &[foreign]).unwrap();
        save_baselines(&path, &[baseline("symbol_sync_8sps", 400.0)]).unwrap();
        let loaded = load_baselines(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let kept: Vec<&PerfBaseline> = loaded
            .iter()
            .filter(|row| row.host == "sparc-plan9" && row.bench == "symbol_sync_8sps")
            .collect();
        assert_eq!(kept.len(), 1, "{loaded:#?}");
        assert!((kept[0].msamples_per_s - 11.0).abs() < 1e-9);
        assert_eq!(rows_for_host(&loaded).len(), 2);
    }

    #[test]
    #[should_panic(expected = "so the gate cannot run")]
    fn a_baseline_from_another_host_alone_cannot_stand_in_for_this_one() {
        let mut foreign = baseline("symbol_sync_8sps", 400.0);
        foreign.host = "sparc-plan9".into();
        let _ = committed_for_host(&[foreign], "modem throughput", RECORD_COMMAND);
    }

    #[test]
    fn a_baseline_for_this_host_is_the_one_the_gate_reads() {
        let mut foreign = baseline("symbol_sync_8sps", 11.0);
        foreign.host = "sparc-plan9".into();
        let mine = baseline("symbol_sync_8sps", 400.0);
        let selected = committed_for_host(&[foreign, mine], "modem throughput", RECORD_COMMAND);
        assert_eq!(selected.len(), 1);
        assert!((selected[0].msamples_per_s - 400.0).abs() < 1e-9);
    }

    #[test]
    fn the_missing_baseline_message_names_the_host_and_the_command() {
        let message = missing_host_baseline("modem throughput", RECORD_COMMAND);
        assert!(message.contains(&host_id()), "{message}");
        assert!(message.contains(RECORD_COMMAND), "{message}");
        assert!(message.contains(BASELINE_DOC), "{message}");
    }

    #[test]
    fn a_vanished_bench_is_a_full_regression() {
        let committed = vec![baseline("cpm_demod_m4_48k", 100.0)];
        let regressions = compare_perf(&[], &committed, REGRESSION_FRACTION).unwrap_err();
        assert_eq!(regressions[0].change_fraction, -1.0);
    }

    #[test]
    fn throughput_helper_measures_something_finite() {
        let mut acc = 0u64;
        let msamples_per_s = measure_throughput(8, 1_024, || {
            for i in 0..1_024u64 {
                acc = acc.wrapping_add(i * i);
            }
        });
        black_box(acc);
        assert!(
            msamples_per_s.is_finite() && msamples_per_s > 0.0,
            "measured {msamples_per_s} MS/s"
        );
    }

    #[test]
    fn symbol_sync_steady_state_allocates_nothing() {
        let iq = shaped_bpsk_iq(4_096, 8.0, 0x0dd5);
        let mut sync = SymbolSync::new(8.0, 0.01);
        let mut symbols = Vec::with_capacity(iq.len());
        sync.process(&iq, &mut symbols);
        symbols.clear();
        sync.process(&iq, &mut symbols);
        symbols.clear();
        assert_no_alloc("SymbolSync::process", || sync.process(&iq, &mut symbols));
        assert!(
            !symbols.is_empty(),
            "the measured call recovered no symbols"
        );
    }

    const SYMBOL_SYNC_RATE_HZ: f64 = 8.0 * 4_800.0;

    const RETIRED: [&str; 1] = ["fsk4_dmr_48k"];

    const RECORD_COMMAND: &str = "cargo test -p sdrmm-modem-test-support --release -- \
         --ignored --exact ber::perf::tests::write_perf_baseline";

    fn measured_baselines() -> Vec<PerfBaseline> {
        let bpsk = shaped_bpsk_iq(4_096, 8.0, 0x0dd5);
        let mut sync = SymbolSync::new(8.0, 0.01);
        let mut symbols = Vec::with_capacity(bpsk.len());
        sync.process(&bpsk, &mut symbols);
        let sync_msps = measure_throughput(1_500, bpsk.len() as u64, || {
            symbols.clear();
            sync.process(&bpsk, &mut symbols);
        });

        vec![PerfBaseline {
            bench: "symbol_sync_8sps".into(),
            msamples_per_s: sync_msps,
            realtime_factor: sync_msps * 1e6 / SYMBOL_SYNC_RATE_HZ,
            config: "8 S/sym, loop_bw 0.01, RRC α=0.35 shaped BPSK".into(),
            host: host_id(),
        }]
    }

    fn live_committed(committed: &[PerfBaseline]) -> Vec<PerfBaseline> {
        committed
            .iter()
            .filter(|b| !RETIRED.contains(&b.bench.as_str()))
            .cloned()
            .collect()
    }

    fn gated_benches(committed: &[PerfBaseline]) -> Vec<String> {
        let mut benches: Vec<String> = live_committed(committed)
            .into_iter()
            .filter(|b| b.bench != CALIBRATION_BENCH)
            .map(|b| b.bench)
            .collect();
        benches.sort();
        benches.dedup();
        benches
    }

    fn committed_baseline_path() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../modem/baselines/perf_phase0.json")
    }

    #[test]
    fn the_committed_file_keeps_its_history_and_gates_the_rest() {
        let committed = load_baselines(&committed_baseline_path()).unwrap();
        assert!(
            committed.iter().any(|b| b.bench == "fsk4_dmr_48k"),
            "the pre-migration reference row was removed from the committed baseline"
        );
        assert_eq!(gated_benches(&committed), ["symbol_sync_8sps"]);
    }

    #[test]
    #[ignore = "rewrites the committed baseline; run explicitly in release on the reference host"]
    fn write_perf_baseline() {
        if cfg!(debug_assertions) {
            panic!("a debug-profile number must never become the committed baseline");
        }
        let path = committed_baseline_path();
        let mut rows: Vec<PerfBaseline> = rows_for_host(&load_baselines(&path).unwrap_or_default())
            .into_iter()
            .filter(|b| RETIRED.contains(&b.bench.as_str()))
            .collect();
        rows.extend(measured_baselines());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        save_baselines(&path, &rows).unwrap();
    }

    #[test]
    #[ignore = "nightly perf gate; run in release: cargo test -p sdrmm-modem-test-support --release compare_perf_baseline -- --ignored"]
    fn compare_perf_baseline() {
        let committed = live_committed(&rows_for_host(
            &load_baselines(&committed_baseline_path()).unwrap(),
        ));
        assert!(
            !gated_benches(&committed).is_empty(),
            "{}",
            missing_host_baseline("modem throughput", RECORD_COMMAND)
        );
        if cfg!(debug_assertions) {
            eprintln!("skipping the comparison: throughput is only comparable in release");
            return;
        }
        match compare_perf(&measured_baselines(), &committed, REGRESSION_FRACTION) {
            Ok(changes) => {
                for c in changes {
                    eprintln!(
                        "{}: {:+.1}% vs baseline ({:.1} -> {:.1} MS/s)",
                        c.bench,
                        100.0 * c.change_fraction,
                        c.committed_msamples_per_s,
                        c.measured_msamples_per_s
                    );
                }
            }
            Err(regressions) => panic!(
                "throughput regressions past {:.0}%: {regressions:#?}",
                100.0 * REGRESSION_FRACTION
            ),
        }
    }
}
