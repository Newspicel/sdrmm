#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use num_complex::Complex;
use sdrmm_channels::{ChannelCtx, ChannelOutputs};
use sdrmm_wire::{ChannelDescriptor, DecoderFamily};

const MAX_SAMPLES: usize = 1 << 18;

static VOICES: OnceLock<Vec<ChannelDescriptor>> = OnceLock::new();

fn voices() -> Vec<ChannelDescriptor> {
    let mut voices: Vec<ChannelDescriptor> = sdrmm_channels::descriptors()
        .into_iter()
        .filter(|descriptor| descriptor.family == DecoderFamily::DIGITAL_VOICE)
        .collect();
    voices.sort_unstable_by(|left, right| left.type_id.cmp(&right.type_id));
    voices
}

fn sample(byte: u8) -> f32 {
    (f32::from(byte) - 127.5) / 127.5
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, tail)) = data.split_first() else {
        return;
    };
    let voices = VOICES.get_or_init(voices);
    let Some(descriptor) = voices.get(usize::from(mode) % voices.len().max(1)) else {
        return;
    };
    let type_id = &descriptor.type_id;
    let Some(settings) = &descriptor.defaults else {
        return;
    };
    let ctx = ChannelCtx {
        input_rate: descriptor.input_rate_hz,
    };
    let Ok(mut channel) = sdrmm_channels::create(ctx, settings) else {
        return;
    };
    let iq: Vec<Complex<f32>> = tail
        .as_chunks::<2>()
        .0
        .iter()
        .take(MAX_SAMPLES)
        .map(|pair| Complex::new(sample(pair[0]), sample(pair[1])))
        .collect();
    let mut out = ChannelOutputs::default();
    for block in iq.chunks(1_024) {
        out.reset();
        channel.process(block, &mut out);
        assert!(
            out.audio_pcm.iter().all(|sample| sample.is_finite()),
            "the {type_id} vocoder produced a non-finite sample"
        );
        assert!(
            out.audio_pcm.iter().all(|sample| sample.abs() <= 1.0),
            "the {type_id} vocoder produced a sample outside full scale"
        );
    }
});
