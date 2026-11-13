#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use num_complex::Complex;
use sdrmm_channels::{ChannelCtx, ChannelOutputs};
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, Control, ParamOption, SettingField};
use serde_json::{Map, Value};

const MAX_SAMPLES: usize = 1 << 16;

static DESCRIPTORS: OnceLock<Vec<ChannelDescriptor>> = OnceLock::new();

fn options(control: &Control) -> Option<&[ParamOption]> {
    match control {
        Control::Choice { options } if !options.is_empty() => Some(options),
        Control::Live { fallback } => options(fallback),
        _ => None,
    }
}

/// Sets each choice of the form from one knob byte, so every enum variant is reached even
/// when the JSON names none. Returns how many knobs it used.
fn steer(settings: &mut Value, form: &[SettingField], knobs: &[u8]) -> usize {
    if settings.is_null() {
        *settings = Value::Object(Map::new());
    }
    let Value::Object(values) = settings else {
        return 0;
    };
    let choices = form
        .iter()
        .filter_map(|field| options(&field.control).map(|options| (&field.key, options)));
    let mut used = 0;
    for ((key, options), knob) in choices.zip(knobs) {
        let option = &options[usize::from(*knob) % options.len()];
        values.insert(key.clone(), option.value.clone());
        used += 1;
    }
    used
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
    let Ok(mut settings) = serde_json::from_slice::<ChannelSettings>(&data[..split]) else {
        return;
    };
    let descriptors = DESCRIPTORS.get_or_init(sdrmm_channels::descriptors);
    let Some(descriptor) = descriptors
        .iter()
        .find(|descriptor| descriptor.type_id == settings.params.type_id())
    else {
        return;
    };
    let tail = data.get(split + 1..).unwrap_or_default();
    let used = steer(&mut settings.params.settings, &descriptor.settings, tail);
    let ctx = ChannelCtx {
        input_rate: descriptor.input_rate_hz,
    };
    let Ok(mut channel) = sdrmm_channels::create(ctx, &settings) else {
        return;
    };
    let _ = sdrmm_channels::channel_filter(&settings.params);
    let _ = sdrmm_channels::occupied_band(&settings.params);
    let mut out = ChannelOutputs::default();
    let iq = samples(tail.get(used..).unwrap_or_default());
    for block in iq.chunks(512) {
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
