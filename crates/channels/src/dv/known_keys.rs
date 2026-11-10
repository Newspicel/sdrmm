use std::path::{Path, PathBuf};

use serde_json::Value;

const VOCODER_RATE_HZ: usize = 8_000;
const FRAME_SECONDS: f64 = 0.02;
/// How closely the decoded loudness contour has to follow the expected audio's. The full-rate
/// vocoder tracks the reference decoder closely; the half-rate one parts from it in pauses, where
/// the reference keeps a noise floor.
pub(crate) const MIN_FULL_RATE_CORRELATION: f64 = 0.9;
pub(crate) const MIN_HALF_RATE_CORRELATION: f64 = 0.6;
const TONE_TOLERANCE_HZ: f64 = 10.0;

pub(crate) struct Sample {
    pub(crate) name: String,
    pub(crate) key_id: Option<u16>,
    pub(crate) key: String,
    pub(crate) frames: Vec<Frame>,
    expected: Vec<f32>,
}

pub(crate) struct Frame {
    pub(crate) bytes: Vec<u8>,
    pub(crate) algorithm: Option<u8>,
    pub(crate) key_id: Option<u16>,
    pub(crate) mi: Option<u64>,
    pub(crate) tag: Option<String>,
}

/// Every sample directory whose name starts with `prefix`, or none without the checkout.
pub(crate) fn samples(prefix: &str) -> Vec<Sample> {
    let Ok(root) = std::env::var("KNOWN_KEY_SAMPLES") else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .expect("sample checkout")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix))
        })
        .collect();
    dirs.sort();
    dirs.iter().map(|dir| load(dir)).collect()
}

fn load(dir: &Path) -> Sample {
    let key_text = std::fs::read_to_string(dir.join("key.txt")).expect("key.txt");
    let field = |name: &str| {
        key_text
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name}=")))
            .map(str::trim)
    };
    let json: Value =
        serde_json::from_slice(&std::fs::read(dir.join("encrypted.mbe")).expect("encrypted.mbe"))
            .expect("mbe json");
    let frames = json["frames"]
        .as_array()
        .expect("frames")
        .iter()
        .map(frame)
        .collect();
    Sample {
        name: dir
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned(),
        key_id: field("key_id")
            .map(|id| u16::from_str_radix(id.trim_start_matches("0x"), 16).expect("key id")),
        key: field("key_hex").expect("key_hex").to_owned(),
        frames,
        expected: wav_first_channel(&dir.join("expected.wav")),
    }
}

fn frame(value: &Value) -> Frame {
    let hex = value["hex"].as_str().expect("hex");
    Frame {
        bytes: (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex byte"))
            .collect(),
        algorithm: value["encryption_algorithm"].as_u64().map(|id| id as u8),
        key_id: value["encryption_key_id"].as_u64().map(|id| id as u16),
        mi: value["encryption_mi"]
            .as_str()
            .map(|mi| u64::from_str_radix(&mi[..mi.len().min(16)], 16).expect("mi")),
        tag: value["tag"].as_str().map(str::to_owned),
    }
}

fn wav_first_channel(path: &Path) -> Vec<f32> {
    let wav = std::fs::read(path).expect("expected.wav");
    let channels = usize::from(u16::from_le_bytes([wav[22], wav[23]]));
    let mut at = 12;
    while at + 8 <= wav.len() {
        let size = u32::from_le_bytes(wav[at + 4..at + 8].try_into().expect("chunk")) as usize;
        if &wav[at..at + 4] == b"data" {
            return wav[at + 8..at + 8 + size]
                .as_chunks::<2>()
                .0
                .iter()
                .step_by(channels)
                .map(|sample| f32::from(i16::from_le_bytes(*sample)) / 32_768.0)
                .collect();
        }
        at += 8 + size;
    }
    panic!("{path:?} has no data chunk");
}

/// Loudness per 20 ms frame in decibels.
fn contour(pcm: &[f32], rate: usize) -> Vec<f64> {
    let frame = (rate as f64 * FRAME_SECONDS) as usize;
    pcm.chunks_exact(frame)
        .map(|chunk| {
            let power = chunk.iter().map(|&s| f64::from(s * s)).sum::<f64>() / frame as f64;
            10.0 * (power + 1e-9).log10()
        })
        .collect()
}

/// Correlation of the decoded audio's loudness contour with the expected audio's, over the
/// expected audio's last `frames` frames. Speech decoded with the right key follows it; a wrong
/// keystream gives noise that does not.
pub(crate) fn correlation(sample: &Sample, decoded: &[f32], decoded_rate: usize) -> f64 {
    let ours = contour(decoded, decoded_rate);
    let theirs = contour(&sample.expected, VOCODER_RATE_HZ);
    let ours = &ours[ours.len().saturating_sub(theirs.len())..];
    let theirs = &theirs[theirs.len().saturating_sub(ours.len())..];
    let mean = |values: &[f64]| values.iter().sum::<f64>() / values.len() as f64;
    let (mean_ours, mean_theirs) = (mean(ours), mean(theirs));
    let (mut cross, mut own, mut other) = (0.0, 0.0, 0.0);
    for (a, b) in ours.iter().zip(theirs) {
        cross += (a - mean_ours) * (b - mean_theirs);
        own += (a - mean_ours).powi(2);
        other += (b - mean_theirs).powi(2);
    }
    cross / (own * other).sqrt()
}

fn dominant_hz(pcm: &[f32], rate: usize) -> f64 {
    let (frequency, _) = crate::testutil::dominant_tone(pcm, rate as f64);
    frequency
}

/// Whether the decoded audio carries the expected audio's tone: the NXDN samples are the
/// conformance vectors' 1031 Hz tone, which has no loudness contour to follow.
pub(crate) fn tone_matches(sample: &Sample, decoded: &[f32], decoded_rate: usize) -> (f64, bool) {
    let ours = dominant_hz(decoded, decoded_rate);
    let theirs = dominant_hz(&sample.expected, VOCODER_RATE_HZ);
    (ours, (ours - theirs).abs() <= TONE_TOLERANCE_HZ)
}
