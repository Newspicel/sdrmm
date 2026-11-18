use num_complex::Complex;
use sdrmm_wire::DvFrame;

use sdrmm_decoder::{AUDIO_RATE, Channel, ChannelOutputs};

const RATE_HZ: f64 = 48_000.0;

pub fn decode(chan: &mut dyn Channel, iq: &[Complex<f32>]) -> Vec<DvFrame> {
    decode_with_audio(chan, iq).0
}

pub fn decode_with_audio(chan: &mut dyn Channel, iq: &[Complex<f32>]) -> (Vec<DvFrame>, Vec<f32>) {
    let mut out = ChannelOutputs::default();
    let mut frames = Vec::new();
    let mut audio = Vec::new();
    let quiet = super::complex_noise(0x1157, 0.01, 4 * RATE_HZ as usize / 10);
    chan.process(&quiet, &mut out);
    out.reset();

    let mut pos = 0;
    for len in [997usize, 1, 4_096, 65, 2_048, 7].iter().cycle() {
        if pos >= iq.len() {
            break;
        }
        let end = (pos + len).min(iq.len());
        out.reset();
        chan.process(&iq[pos..end], &mut out);
        audio.extend_from_slice(&out.audio_pcm);
        for event in out.events.drain(..) {
            match event.read::<DvFrame>() {
                Some(frame) => frames.push(frame),
                None => panic!("unexpected {} event", event.kind),
            }
        }
        pos = end;
    }
    (frames, audio)
}

pub fn assert_tone_audio(audio: &[f32], frames: usize) {
    assert!(
        (audio.len() as isize - (frames * 960) as isize).abs() <= 1,
        "expected {frames} vocoder frames, got {} PCM samples",
        audio.len()
    );
    assert!(audio.iter().all(|sample| sample.is_finite()));
    assert!(
        audio.iter().all(|sample| sample.abs() < 1.0),
        "presentation gain drove the vocoder into full-scale clipping"
    );
    let settled = &audio[3 * 960..];
    let rms = super::rms(settled);
    let (frequency, _) = super::dominant_tone(settled, f64::from(AUDIO_RATE));
    assert!(rms > 0.005, "decoded tone is silent: rms {rms}");
    assert!(
        (frequency - 440.0).abs() < 50.0,
        "decoded tone shifted to {frequency} Hz"
    );
}
