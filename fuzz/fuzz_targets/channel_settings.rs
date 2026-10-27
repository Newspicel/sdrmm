#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use num_complex::Complex;
use sdrmm_channels::{ChannelCtx, ChannelOutputs};
use sdrmm_wire::{
    AisChannel, ChannelDescriptor, ChannelParams, ChannelSettings, DatvStandard, DectBand,
    DectSides, DectSpan, LoraBandwidth, LoraSpreadingFactor,
};

const MAX_SAMPLES: usize = 1 << 16;

const SPREADING: [LoraSpreadingFactor; 7] = [
    LoraSpreadingFactor::All,
    LoraSpreadingFactor::Sf7,
    LoraSpreadingFactor::Sf8,
    LoraSpreadingFactor::Sf9,
    LoraSpreadingFactor::Sf10,
    LoraSpreadingFactor::Sf11,
    LoraSpreadingFactor::Sf12,
];

static DESCRIPTORS: OnceLock<Vec<ChannelDescriptor>> = OnceLock::new();

fn pick<T: Copy>(choices: &[T], knob: u8) -> T {
    choices[usize::from(knob) % choices.len()]
}

fn steer(params: &mut ChannelParams, knob: u8) {
    match params {
        ChannelParams::Dect(dect) => {
            dect.span = pick(&[DectSpan::Carrier, DectSpan::Band], knob);
            dect.band = pick(&[DectBand::Eu, DectBand::Us], knob >> 1);
            dect.sides = pick(&[DectSides::Both, DectSides::Rfp, DectSides::Pp], knob >> 2);
        }
        ChannelParams::Lora(lora) => {
            lora.bandwidth = pick(&LoraBandwidth::ALL, knob);
            lora.spreading_factor = pick(&SPREADING, knob >> 4);
        }
        ChannelParams::Datv(datv) => {
            datv.standard = pick(&[DatvStandard::DvbS, DatvStandard::DvbS2], knob);
        }
        ChannelParams::Ais(ais) => {
            ais.ais_channel = pick(&[AisChannel::A, AisChannel::B], knob);
        }
        _ => {}
    }
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
    let tail = data.get(split + 1..).unwrap_or_default();
    steer(
        &mut settings.params,
        tail.first().copied().unwrap_or_default(),
    );
    let descriptors = DESCRIPTORS.get_or_init(sdrmm_channels::descriptors);
    let Some(descriptor) = descriptors
        .iter()
        .find(|descriptor| descriptor.type_id == settings.params.type_id())
    else {
        return;
    };
    let ctx = ChannelCtx {
        input_rate: descriptor.input_rate_hz,
    };
    let Ok(mut channel) = sdrmm_channels::create(ctx, &settings) else {
        return;
    };
    let _ = sdrmm_channels::channel_filter(&settings.params);
    let _ = sdrmm_channels::occupied_band(&settings.params);
    let mut out = ChannelOutputs::default();
    let iq = samples(tail.get(1..).unwrap_or_default());
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
