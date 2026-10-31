use std::path::Path;

use anyhow::{Context, Result};
use num_complex::Complex;
use sdrmm_channels::synth::dv;
use sdrmm_wire::ChannelSettings;

const MAX_SEED_BYTES: usize = 32 * 1024;
const SEED_SAMPLES: usize = 1_024;
const SEED_TONE: f32 = 0.05;

const DV_MODES: [&str; 7] = ["dmr", "dpmr", "dstar", "m17", "nxdn", "p25", "ysf"];

const CODEPLUGS: [&str; 2] = ["anytone-d890uv-v100.img", "radtel-rt4d-handbuilt.img"];

const BITSTREAMS: [(u8, &str); 9] = [
    (0, "tone_48k_mono.mp2"),
    (0, "tone_32k_stereo.mp2"),
    (1, "tone.aac"),
    (3, "tone.ac3"),
    (4, "tone.eac3"),
    (5, "pattern.m2v"),
    (6, "pattern.h264"),
    (7, "pattern.hevc"),
    (8, "tone_48k_mono.mp2"),
];

pub fn run(root: &Path) -> Result<()> {
    let corpus = root.join("fuzz/corpus");
    dv_voice(&corpus.join("dv_voice"))?;
    channel_chain(&corpus.join("channel_chain"))?;
    channel_settings(&corpus.join("channel_settings"))?;
    cps_image(root, &corpus.join("cps_image"))?;
    media_bitstream(root, &corpus.join("media_bitstream"))
}

fn cps_image(root: &Path, out: &Path) -> Result<()> {
    make(out)?;
    for name in CODEPLUGS {
        let source = root.join("fixtures/cps").join(name);
        let mut seed = vec![16];
        seed.extend(read(&source)?);
        write(&out.join(name), &seed)?;
    }
    Ok(())
}

fn media_bitstream(root: &Path, out: &Path) -> Result<()> {
    make(out)?;
    for (selector, name) in BITSTREAMS {
        let source = root.join("fixtures/broadcast_audio").join(name);
        let mut seed = vec![selector];
        seed.extend(read(&source)?);
        write(&out.join(format!("{selector}-{name}")), &seed)?;
    }
    Ok(())
}

fn dv_voice(out: &Path) -> Result<()> {
    make(out)?;
    for (mode, type_id) in DV_MODES.iter().enumerate() {
        let rate = input_rate(type_id)?;
        for (label, iq) in transmissions(type_id, rate) {
            let mut seed = vec![u8::try_from(mode)?];
            seed.extend(quantised(&iq));
            write(&out.join(format!("{type_id}-{label}")), &seed)?;
        }
    }
    Ok(())
}

fn channel_chain(out: &Path) -> Result<()> {
    make(out)?;
    let tone = quantised(&tone(SEED_SAMPLES, SEED_TONE));
    for descriptor in sdrmm_channels::descriptor_table() {
        let mut seed = descriptor.type_id.clone().into_bytes();
        seed.push(0);
        seed.extend_from_slice(&tone);
        write(&out.join(&descriptor.type_id), &seed)?;
    }
    Ok(())
}

fn channel_settings(out: &Path) -> Result<()> {
    make(out)?;
    let tone = quantised(&tone(SEED_SAMPLES, SEED_TONE));
    for descriptor in sdrmm_channels::descriptor_table() {
        let Some(settings) = ChannelSettings::default_for(&descriptor.type_id) else {
            continue;
        };
        let mut seed = serde_json::to_vec(&settings)
            .with_context(|| format!("serialize {} settings", descriptor.type_id))?;
        seed.push(0);
        seed.push(0);
        seed.extend_from_slice(&tone);
        write(&out.join(&descriptor.type_id), &seed)?;
    }
    Ok(())
}

fn transmissions(type_id: &str, rate: f64) -> Vec<(&'static str, Vec<Complex<f32>>)> {
    match type_id {
        "dmr" => vec![
            (
                "voice",
                dv::dmr::transmission(&dv::dmr::Call::default(), rate),
            ),
            (
                "simplex",
                dv::dmr::simplex_transmission(&dv::dmr::Call::default(), rate),
            ),
        ],
        "dpmr" => vec![(
            "voice",
            dv::dpmr::transmission(&dv::dpmr::Call::default(), rate),
        )],
        "dstar" => vec![
            (
                "voice",
                dv::dstar::transmission(&dv::dstar::Call::default(), rate),
            ),
            (
                "repeated",
                dv::dstar::repeated_transmission(&dv::dstar::Call::default(), 2, rate),
            ),
        ],
        "m17" => vec![("voice", dv::m17::transmission("ALL", "DL1ABC", rate))],
        "nxdn" => vec![(
            "voice",
            dv::nxdn::transmission(&dv::nxdn::Shape::default(), 1, true, rate),
        )],
        "p25" => vec![
            ("voice", dv::p25::transmission(0x293, rate)),
            ("trunking", dv::p25::trunking(0x293, rate)),
        ],
        _ => vec![(
            "voice",
            dv::ysf::transmission_with_callsigns(
                &dv::ysf::Fich::default(),
                &dv::ysf::Call::default(),
                rate,
            ),
        )],
    }
}

fn input_rate(type_id: &str) -> Result<f64> {
    sdrmm_channels::descriptor(type_id)
        .map(|descriptor| descriptor.input_rate_hz)
        .with_context(|| format!("no channel describes itself as `{type_id}`"))
}

fn tone(samples: usize, radians_per_sample: f32) -> Vec<Complex<f32>> {
    (0..samples)
        .map(|at| Complex::from_polar(0.7, radians_per_sample * at as f32))
        .collect()
}

fn quantised(iq: &[Complex<f32>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(iq.len() * 2);
    for sample in iq.iter().take(MAX_SEED_BYTES / 2) {
        out.push(code(sample.re));
        out.push(code(sample.im));
    }
    out
}

fn code(value: f32) -> u8 {
    (value * 127.5 + 127.5).round().clamp(0.0, 255.0) as u8
}

fn make(out: &Path) -> Result<()> {
    std::fs::create_dir_all(out).with_context(|| format!("create {}", out.display()))
}

fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("read {}", path.display()))
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}
