use super::{
    Parakeet,
    matmul::{HalfMatrix, matmul},
    vocab::Vocab,
};
use crate::math::neural::f32_to_f16;

const RATE: f64 = 16_000.0;

/// The second of sweep and tone `scripts/speech-fixture.py` fed the reference model.
fn fixture_audio() -> Vec<f32> {
    (0..RATE as usize)
        .map(|n| {
            let t = n as f64 / RATE;
            let sweep = 0.3 * (2.0 * std::f64::consts::PI * (200.0 + 150.0 * t) * t).sin();
            let tone = 0.1 * (2.0 * std::f64::consts::PI * 1234.0 * t).sin();
            (sweep + tone) as f32
        })
        .collect()
}

#[test]
fn a_tiny_model_matches_the_transformers_reference() {
    let model = Parakeet::load(include_bytes!("fixtures/tiny.sdrmmnn").to_vec()).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/expected.json")).unwrap();
    let pcm = fixture_audio();
    let (encoded, frames) = model.encode(&pcm, 2).unwrap();
    assert_eq!(frames as u64, expected["frames"].as_u64().unwrap());
    let want: Vec<f32> = expected["encoded"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap() as f32)
        .collect();
    assert_eq!(encoded.len(), want.len());
    let worst = encoded
        .iter()
        .zip(&want)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    assert!(worst < 0.05, "encoder drifts {worst} from the reference");
    let tokens: Vec<u64> = model
        .tokens(&pcm, 1)
        .unwrap()
        .into_iter()
        .map(|t| t as u64)
        .collect();
    let want_tokens: Vec<u64> = expected["tokens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert_eq!(tokens, want_tokens);
}

#[test]
fn threads_do_not_change_the_text() {
    let model = Parakeet::load(include_bytes!("fixtures/tiny.sdrmmnn").to_vec()).unwrap();
    let pcm = fixture_audio();
    assert_eq!(
        model.tokens(&pcm, 1).unwrap(),
        model.tokens(&pcm, 5).unwrap()
    );
}

#[test]
fn silence_too_short_to_frame_is_no_text() {
    let model = Parakeet::load(include_bytes!("fixtures/tiny.sdrmmnn").to_vec()).unwrap();
    assert_eq!(model.transcribe(&[0.0; 100], 1).unwrap(), "");
}

#[test]
fn a_file_that_is_not_a_speech_model_is_refused() {
    assert!(Parakeet::load(b"SDRMMNN1".to_vec()).is_err());
}

#[test]
fn an_8_khz_tone_keeps_its_level_and_place_at_16_khz() {
    let tone: Vec<f32> = (0..800)
        .map(|n| (2.0 * std::f32::consts::PI * 500.0 * n as f32 / 8_000.0).sin())
        .collect();
    let wide = super::to_model_rate(&tone, 8_000).unwrap();
    assert_eq!(wide.len(), 1_600);
    for (n, &got) in wide.iter().enumerate().take(1_400).skip(200) {
        let want = (2.0 * std::f32::consts::PI * 500.0 * n as f32 / 16_000.0).sin();
        assert!((got - want).abs() < 0.05, "sample {n}: {got} vs {want}");
    }
    assert_eq!(super::to_model_rate(&wide, 16_000).unwrap(), wide);
    assert_eq!(super::to_model_rate(&tone, 48_000).unwrap().len(), 266);
    assert!(super::to_model_rate(&tone, 11_025).is_err());
}

#[test]
fn half_matmul_matches_the_textbook_product() {
    let (depth, cols) = (300, 70);
    let weights: Vec<f32> = (0..depth * cols)
        .map(|i| ((i * 37 % 101) as f32 - 50.0) / 64.0)
        .collect();
    let mut bytes = vec![0u8; 3];
    for &w in &weights {
        bytes.extend(f32_to_f16(w).to_le_bytes());
    }
    let matrix = HalfMatrix { at: 3, depth, cols };
    for rows in [1, 4, 7] {
        let input: Vec<f32> = (0..rows * depth)
            .map(|i| ((i * 13 % 29) as f32 - 14.0) / 8.0)
            .collect();
        for threads in [1, 3] {
            let mut out = vec![0.0; rows * cols];
            matmul(&bytes, &matrix, &input, rows, &mut out, threads);
            for r in 0..rows {
                for c in 0..cols {
                    let want: f32 = (0..depth)
                        .map(|k| input[r * depth + k] * weights[k * cols + c])
                        .sum();
                    assert!((out[r * cols + c] - want).abs() < 1e-3, "row {r} col {c}");
                }
            }
        }
    }
}

#[test]
fn pieces_join_into_words_and_bytes_into_letters() {
    let vocab = Vocab::parse("\n\u{2581}Fahr\nzeug\n\u{2581}3\n<0xC3>\n<0x9F>\n.");
    assert_eq!(vocab.decode(&[1, 2, 3, 0, 6]), "Fahrzeug 3.");
    assert_eq!(vocab.decode(&[1, 4, 5]), "Fahr\u{df}");
}
