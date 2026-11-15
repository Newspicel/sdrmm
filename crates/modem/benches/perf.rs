use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use num_complex::Complex;
use sdrmm_dsp::{SymbolSync, design_rrc, math::rng::Rng};
use sdrmm_modem::{
    cpm::{CpmDemod, CpmMod, CpmParams, Mapping, RealDetector, TIMING_BW_BURST},
    pulse::{self, Norm},
};
use sdrmm_modem_test_support::ber::perf::{shaped_bpsk_iq, test_dibits};

const SEED: u32 = 0x5eed;

const CPM_SPS: f64 = 10.0;

const CPM_SYMBOLS: usize = 2_400;

const AFSK_RATE_HZ: f64 = 12_000.0;

fn symbol_sync_8sps(c: &mut Criterion) {
    let iq = shaped_bpsk_iq(4_096, 8.0, 0x0dd5);
    let mut sync = SymbolSync::new(8.0, 0.01);
    bench_block(c, "symbol_sync_8sps", "process", iq.len(), |symbols| {
        sync.process(black_box(&iq), symbols);
    });
}

fn design_rrc_canary(c: &mut Criterion) {
    let taps = design_rrc(CPM_SPS, 0.2, 8);
    let mut group = c.benchmark_group("design_rrc");
    group.throughput(Throughput::Elements(taps.len() as u64));
    group.bench_function("sps10_alpha02_span8", |b| {
        b.iter(|| black_box(design_rrc(black_box(CPM_SPS), 0.2, 8)));
    });
    group.finish();
}

fn cpm_demod_m4_48k(c: &mut Criterion) {
    let shape = pulse::root_raised_cosine(CPM_SPS, 0.2, 8, Norm::Area);
    let params = CpmParams::from_deviation(
        Mapping::new(vec![1.0, 3.0, -1.0, -3.0]),
        1_944.0,
        4_800.0,
        shape.clone(),
        CPM_SPS,
    );
    let iq = cpm_iq(&params, &test_dibits(CPM_SYMBOLS, SEED));
    bench_cpm(c, "cpm_demod_m4_48k", &params, &shape, &iq);
}

fn gmsk_bt05_demod(c: &mut Criterion) {
    let shape = pulse::gaussian_freq(CPM_SPS, 0.5, 3, Norm::Area);
    let params = CpmParams::from_h(Mapping::natural(2), 0.5, shape.clone(), CPM_SPS);
    let iq = cpm_iq(&params, &test_bits(CPM_SYMBOLS, SEED));
    bench_cpm(c, "gmsk_bt05_demod", &params, &shape, &iq);
}

fn msk_demod(c: &mut Criterion) {
    let shape = pulse::rect(CPM_SPS, Norm::Area);
    let params = CpmParams::from_h(Mapping::natural(2), 0.5, shape.clone(), CPM_SPS);
    let iq = cpm_iq(&params, &test_bits(CPM_SYMBOLS, SEED));
    bench_cpm(c, "msk_demod", &params, &shape, &iq);
}

fn afsk_filterbank_12k(c: &mut Criterion) {
    let params = CpmParams::from_deviation(
        Mapping::new(vec![1.0, -1.0]),
        500.0,
        1_200.0,
        pulse::rect(CPM_SPS, Norm::Area),
        CPM_SPS,
    );
    let baseband = cpm_iq(&params, &test_bits(CPM_SYMBOLS, SEED));
    let mut carrier = sdrmm_dsp::Nco::new(1_700.0, AFSK_RATE_HZ as f32);
    let audio: Vec<f32> = baseband
        .iter()
        .map(|&s| (s * carrier.next_sample()).re)
        .collect();
    let detector = RealDetector::ToneFilterbank {
        plus_hz: 2_200.0,
        minus_hz: 1_200.0,
    };
    let rx = pulse::rect(5.0, Norm::Area);
    let mut demod = CpmDemod::real(&params, &rx, TIMING_BW_BURST, AFSK_RATE_HZ, detector);
    bench_block(
        c,
        "afsk_filterbank_12k",
        "process_real",
        audio.len(),
        |soft| {
            demod.process_real(black_box(&audio), soft);
        },
    );
}

fn mfsk4_filterbank_48k(c: &mut Criterion) {
    use sdrmm_modem::orthogonal::MfskDemod;
    use sdrmm_modem_test_support::ber::catalog::orthogonal;
    let symbols = orthogonal::filler(4, 1_200);
    let iq = orthogonal::modulate(4, &symbols);
    let demod = MfskDemod::new(orthogonal::params(4));
    bench_block(c, "mfsk4_filterbank_48k", "demodulate", iq.len(), |out| {
        demod.demodulate(black_box(&iq), 0, symbols.len(), out);
    });
}

fn ppm2_matched_8m(c: &mut Criterion) {
    use sdrmm_modem::ppm::SlotDetector;
    use sdrmm_modem_test_support::ber::catalog::ppm;
    let symbols = ppm::filler(2, 2_048);
    let iq = ppm::modulate(2, &symbols);
    let demod = ppm::demod(2, symbols.len(), SlotDetector::MatchedFilter);
    bench_block(c, "ppm2_matched_8m", "demodulate", iq.len(), |out| {
        demod.demodulate(black_box(&iq), 0, symbols.len(), out);
    });
}

fn ofdm64_20m(c: &mut Criterion) {
    use sdrmm_modem::{
        constellation::tables,
        ofdm::{OfdmDemod, OfdmMod, OfdmParams},
    };
    use sdrmm_modem_test_support::ber::catalog::ofdm::{LEAD, SEARCH, SYMBOLS};
    let params = OfdmParams::wifi_like();
    let table = match tables::qam_square(4) {
        Ok(t) => t,
        Err(why) => panic!("QPSK: {why}"),
    };
    let mut rng = Rng::mixed(0x0f06);
    let points: Vec<Complex<f32>> = (0..params.data_subcarriers() * SYMBOLS)
        .map(|_| table.points()[(rng.bits() % table.len() as u64) as usize])
        .collect();
    let mut wave = vec![Complex::new(0.0, 0.0); LEAD];
    OfdmMod::new(params.clone()).frame(&points, &mut wave);

    let mut demod = OfdmDemod::new(params.clone());
    assert!(
        demod.acquire(&wave, SEARCH).is_some(),
        "the bench frame must acquire, or the symbol path measures a stale channel estimate"
    );
    let symbol_samples = SYMBOLS * params.symbol_samples();
    bench_block(
        c,
        "ofdm64_symbols_20m",
        "demodulate",
        symbol_samples,
        |out| {
            demod.demodulate(black_box(&wave), SYMBOLS, out);
        },
    );
    bench_block(
        c,
        "ofdm64_frame_20m",
        "acquire_and_demodulate",
        wave.len(),
        |out| {
            assert!(demod.acquire(black_box(&wave), SEARCH).is_some());
            demod.demodulate(&wave, SYMBOLS, out);
        },
    );
}

/// Times `run` filling a cleared buffer, counted in `elements` input samples per call.
fn bench_block<T>(
    c: &mut Criterion,
    group: &str,
    function: &str,
    elements: usize,
    mut run: impl FnMut(&mut Vec<T>),
) {
    let mut out: Vec<T> = Vec::with_capacity(elements);
    let mut group = c.benchmark_group(group);
    group.throughput(Throughput::Elements(elements as u64));
    group.bench_function(function, |b| {
        b.iter(|| {
            out.clear();
            run(&mut out);
            black_box(out.len())
        });
    });
    group.finish();
}

fn bench_cpm(c: &mut Criterion, group: &str, params: &CpmParams, rx: &[f32], iq: &[Complex<f32>]) {
    let mut demod = CpmDemod::new(params, rx, TIMING_BW_BURST);
    bench_block(c, group, "process", iq.len(), |soft| {
        demod.process(black_box(iq), soft);
    });
}

fn cpm_iq(params: &CpmParams, symbols: &[u8]) -> Vec<Complex<f32>> {
    let mut modulator = CpmMod::new(params.clone());
    let mut iq = Vec::new();
    modulator.modulate(symbols, &mut iq);
    modulator.flush(&mut iq);
    iq
}

fn test_bits(len: usize, seed: u32) -> Vec<u8> {
    test_dibits(len, seed).into_iter().map(|d| d & 1).collect()
}

criterion_group!(
    benches,
    symbol_sync_8sps,
    design_rrc_canary,
    cpm_demod_m4_48k,
    gmsk_bt05_demod,
    msk_demod,
    afsk_filterbank_12k,
    mfsk4_filterbank_48k,
    ppm2_matched_8m,
    ofdm64_20m
);
criterion_main!(benches);
