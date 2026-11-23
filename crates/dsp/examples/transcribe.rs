//! Transcribes a 16 kHz mono 16-bit WAV: `transcribe <model.sdrmmnn> <clip.wav> [threads]`.

use std::time::Instant;

use sdrmm_dsp::audio::speech::{Parakeet, SAMPLE_RATE_HZ};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, model, clip, rest @ ..] = args.as_slice() else {
        return Err("usage: transcribe <model.sdrmmnn> <clip.wav> [threads]".into());
    };
    let threads = rest.first().map_or(Ok(4), |t| t.parse())?;
    let started = Instant::now();
    let parakeet = Parakeet::load(std::fs::read(model)?)?;
    println!("loaded in {:.2?}", started.elapsed());
    let pcm = wav_pcm(&std::fs::read(clip)?)?;
    let started = Instant::now();
    let text = parakeet.transcribe(&pcm, threads)?;
    let took = started.elapsed();
    println!(
        "{:.1} s of audio in {took:.2?} on {threads} threads: {text}",
        pcm.len() as f64 / f64::from(SAMPLE_RATE_HZ)
    );
    Ok(())
}

fn wav_pcm(bytes: &[u8]) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into()?) as usize;
        if id == b"data" {
            let data = bytes.get(at + 8..at + 8 + len).ok_or("truncated wav")?;
            return Ok(data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / 32768.0)
                .collect());
        }
        at += 8 + len + (len & 1);
    }
    Err("no data chunk".into())
}
