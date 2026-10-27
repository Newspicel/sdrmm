#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use num_complex::Complex;
use sdrmm_channels::{ChannelCtx, ChannelOutputs};
use sdrmm_wire::{ChannelDescriptor, ChannelSettings};

const MAX_SAMPLES: usize = 1 << 16;

static BY_TYPE_ID: OnceLock<Vec<&'static ChannelDescriptor>> = OnceLock::new();

fn by_type_id() -> Vec<&'static ChannelDescriptor> {
    let mut sorted: Vec<&'static ChannelDescriptor> =
        sdrmm_channels::descriptor_table().iter().collect();
    sorted.sort_unstable_by(|left, right| left.type_id.cmp(&right.type_id));
    sorted
}

fn pick(name: &str) -> Option<&'static ChannelDescriptor> {
    let sorted = BY_TYPE_ID.get_or_init(by_type_id);
    let at = sorted.partition_point(|descriptor| descriptor.type_id.as_str() < name);
    sorted.get(at).or_else(|| sorted.last()).copied()
}

fn sample(byte: u8) -> f32 {
    (f32::from(byte) - 127.5) / 127.5
}

fn samples(bytes: &[u8]) -> Vec<Complex<f32>> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .take(MAX_SAMPLES)
        .map(|pair| Complex::new(sample(pair[0]), sample(pair[1])))
        .collect()
}

fuzz_target!(|data: &[u8]| {
    let split = data
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(data.len());
    let Ok(name) = str::from_utf8(&data[..split]) else {
        return;
    };
    let Some(descriptor) = pick(name) else {
        return;
    };
    let Some(settings) = ChannelSettings::default_for(&descriptor.type_id) else {
        return;
    };
    let ctx = ChannelCtx {
        input_rate: descriptor.input_rate_hz,
    };
    let Ok(mut channel) = sdrmm_channels::create(ctx, &settings) else {
        return;
    };
    let mut out = ChannelOutputs::default();
    let iq = samples(data.get(split + 1..).unwrap_or_default());
    for block in iq.chunks(256) {
        out.reset();
        channel.process(block, &mut out);
        assert!(
            out.audio_pcm.iter().all(|sample| sample.is_finite()),
            "{} produced a non-finite sample",
            descriptor.type_id
        );
        assert!(
            out.audio_pcm.iter().all(|sample| sample.abs() <= 1.0),
            "{} produced a sample outside full scale",
            descriptor.type_id
        );
    }
});
