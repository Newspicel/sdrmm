#![allow(clippy::expect_used)]

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use num_complex::Complex;
use sdrmm_channels::{
    AdsbParams, Dvbs2Rate, DvbtChannel, DvbtParams, WfmChannel, WfmParams,
    synth::{
        adsb::{me_identification, squitter, transmission},
        datv::{Ldpc, LdpcFrame},
        dvbt as dvbt_synth, rds as rds_synth,
    },
};
use sdrmm_decoder::NativeChannel;
use sdrmm_decoder::{Channel, ChannelCtx, ChannelOutputs};
use sdrmm_modem::synth::add_noise;
use sdrmm_modem_test_support::ber::rng::Rng;
use sdrmm_wire::{ChannelParams, ChannelSettings, Squelch};

const EB_N0_DB: f64 = 2.0;
const WORDS: usize = 4;

fn noisy_words(code: &Ldpc, rate: f64, seed: u64) -> Vec<Vec<f32>> {
    let mut rng = Rng::new(seed);
    let variance = 1.0 / (2.0 * rate * 10f64.powf(EB_N0_DB / 10.0));
    let sigma = variance.sqrt();
    let information = Dvbs2Rate::R1_2.information(LdpcFrame::Normal);
    (0..WORDS)
        .map(|_| {
            let message: Vec<bool> = (0..information).map(|_| rng.next_u64() & 1 == 1).collect();
            let mut codeword = Vec::new();
            code.encode(&message, &mut codeword);
            codeword
                .iter()
                .map(|&bit| {
                    let symbol = if bit { -1.0 } else { 1.0 };
                    (2.0 * (symbol + sigma * rng.normal()) / variance) as f32
                })
                .collect()
        })
        .collect()
}

fn ldpc_normal_half(c: &mut Criterion) {
    let mut code = Ldpc::new(Dvbs2Rate::R1_2, LdpcFrame::Normal).expect("normal 1/2");
    let words = noisy_words(&code, 0.5, 0x1d9c);
    let mut out = Vec::new();
    for word in &words {
        out.clear();
        code.decode(word, &mut out).expect("a converging word");
    }
    let mut group = c.benchmark_group("ldpc_normal_1_2");
    group.throughput(Throughput::Elements((WORDS * words[0].len()) as u64));
    group.bench_function("decode", |b| {
        b.iter(|| {
            for word in &words {
                out.clear();
                black_box(code.decode(black_box(word), &mut out));
            }
        });
    });
    group.finish();
}

const ADSB_RATE: f64 = 2_400_000.0;
const ADSB_SAMPLES: usize = 2_400_000;
const ADSB_BLOCK: usize = 65_536;

fn adsb_sky() -> Vec<Complex<f32>> {
    let frames: Vec<Vec<u8>> = (0..200)
        .map(|k| squitter(0x40_0000 + k, me_identification("SDRMM")))
        .collect();
    let mut iq = transmission(&frames, 5_000.0, 0.5, ADSB_RATE);
    iq.resize(ADSB_SAMPLES, Complex::default());
    add_noise(&mut iq, 0xad5b, 0.05);
    iq
}

fn adsb_decode(c: &mut Criterion) {
    let settings = ChannelSettings {
        frequency_hz: 0.0,
        squelch: Squelch::Off,
        params: ChannelParams::of(&AdsbParams::default()).expect("adsb settings"),
        blanker: Default::default(),
        if_filter: Default::default(),
    };
    let ctx = ChannelCtx {
        input_rate: ADSB_RATE,
    };
    let iq = adsb_sky();
    let mut out = ChannelOutputs::default();
    let mut group = c.benchmark_group("adsb");
    group.throughput(Throughput::Elements(iq.len() as u64));
    group.sample_size(20);
    group.bench_function("one_second", |b| {
        b.iter(|| {
            let mut channel =
                sdrmm_channels::AdsbChannel::new(ctx, settings.clone()).expect("adsb channel");
            let mut decoded = 0;
            for block in iq.chunks(ADSB_BLOCK) {
                out.reset();
                channel.process(black_box(block), &mut out);
                decoded += out.events.len();
            }
            black_box(decoded)
        });
    });
    group.finish();
}

fn wfm_stereo_rds(c: &mut Criterion) {
    const RATE: f64 = 240_000.0;
    let station = rds_synth::Station {
        pi: 0xD3C2,
        ps: "SDRMM   ".to_owned(),
        radiotext: "benchmark".to_owned(),
        pty: 10,
        tp: true,
        ta: false,
        music: true,
        alt_freqs_hz: vec![89_800_000.0],
    };
    let iq = rds_synth::transmission(&station, 0.25, Some(1_000.0), RATE);
    let settings = ChannelSettings {
        frequency_hz: 0.0,
        squelch: Squelch::Off,
        params: ChannelParams::of(&WfmParams::default()).expect("wfm settings"),
        blanker: Default::default(),
        if_filter: Default::default(),
    };
    let ctx = ChannelCtx { input_rate: RATE };
    let mut channel = WfmChannel::new(ctx, settings).expect("wfm channel");
    let mut out = ChannelOutputs::default();
    let mut group = c.benchmark_group("wfm_stereo_rds");
    group.throughput(Throughput::Elements(iq.len() as u64));
    group.bench_function("quarter_second_240k", |b| {
        b.iter(|| {
            let mut audio = 0;
            for block in iq.chunks(4_800) {
                out.reset();
                channel.process(black_box(block), &mut out);
                audio += out.audio_pcm.len();
            }
            black_box(audio)
        });
    });
    group.finish();
}

fn dvbt_8k(c: &mut Criterion) {
    let mut params = dvbt_synth::defaults();
    params.fft = 8192;
    params.guard = 1024;
    let iq = dvbt_synth::waveform(params, 80);
    let dvbt_params = DvbtParams::default();
    let settings = ChannelSettings {
        frequency_hz: 0.0,
        squelch: Squelch::Off,
        params: ChannelParams::of(&dvbt_params).expect("dvbt settings"),
        blanker: Default::default(),
        if_filter: Default::default(),
    };
    let ctx = ChannelCtx {
        input_rate: dvbt_params.sample_rate_hz(),
    };
    let mut out = ChannelOutputs::default();
    let mut group = c.benchmark_group("dvbt_8k");
    group.throughput(Throughput::Elements(iq.len() as u64));
    group.sample_size(20);
    group.bench_function("80_symbols", |b| {
        b.iter(|| {
            let mut channel = DvbtChannel::new(ctx, settings.clone()).expect("dvbt channel");
            let mut events = 0;
            for block in iq.chunks(8_192) {
                out.reset();
                channel.process(black_box(block), &mut out);
                events += out.events.len();
            }
            black_box(events)
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    ldpc_normal_half,
    adsb_decode,
    wfm_stereo_rds,
    dvbt_8k
);
criterion_main!(benches);
