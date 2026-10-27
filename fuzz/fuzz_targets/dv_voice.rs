#![no_main]

use libfuzzer_sys::fuzz_target;
use num_complex::Complex;
use sdrmm_channels::{ChannelCtx, ChannelOutputs};
use sdrmm_wire::ChannelSettings;

const MODES: [&str; 7] = ["dmr", "dpmr", "dstar", "m17", "nxdn", "p25", "ysf"];
const MAX_SAMPLES: usize = 1 << 18;

fn sample(byte: u8) -> f32 {
    (f32::from(byte) - 127.5) / 127.5
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, tail)) = data.split_first() else {
        return;
    };
    let type_id = MODES[usize::from(mode) % MODES.len()];
    let Some(descriptor) = sdrmm_channels::descriptor(type_id) else {
        return;
    };
    let Some(settings) = ChannelSettings::default_for(type_id) else {
        return;
    };
    let ctx = ChannelCtx {
        input_rate: descriptor.input_rate_hz,
    };
    let Ok(mut channel) = sdrmm_channels::create(ctx, &settings) else {
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
