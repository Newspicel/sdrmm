use std::{f64::consts::TAU, path::Path};

use super::{FULL_SCALE, G726, log, subta, subtb};

const ALAW_EVEN_BITS: u16 = 0x55;

fn expand_alaw(code: u16) -> u32 {
    let s = (code ^ 128) & 255;
    let (s, sign) = if s >= 128 { (s - 128, 4_096) } else { (s, 0) };
    let exp = u32::from(s / 16);
    let mant = u32::from(s % 16);
    let ss = if exp == 0 {
        (mant << 1) + 1 + sign
    } else {
        (1 << (exp - 1)) * ((mant << 1) + 33) + sign
    };
    let ssq = (ss & 4_095) << 1;
    if ss / 4_096 == 0 {
        ssq
    } else {
        (16_384 - ssq) & 16_383
    }
}

fn compress_alaw(sr: u32) -> u16 {
    let negative = sr >> 15 != 0;
    let im = if negative { (65_536 - sr) & 32_767 } else { sr };
    let im = if sr == 32_768 { 2 } else { im };
    let halved = if negative {
        ((im + 1) >> 1) - 1
    } else {
        im >> 1
    };
    let mut imag = halved.min(4_095);
    let mut segment = 7;
    for step in 1..=7 {
        imag <<= 1;
        if imag >= 4_096 {
            break;
        }
        segment = 7 - step;
    }
    let mantissa = ((imag & 4_095) >> 8) as u16;
    let code = mantissa + (segment << 4) + if negative { 128 } else { 0 };
    code ^ 128
}

fn sync_alaw(code: u32, sp: u16, dlnx: u32, dsx: u32) -> u16 {
    let im = if code >> 3 == 0 { code + 8 } else { code & 7 };
    let id = match dlnx {
        3_972.. => 9,
        2_048.. => 7,
        400.. => 15,
        349.. => 14,
        300.. => 13,
        246.. => 12,
        178.. => 11,
        80.. => 10,
        _ => 9,
    };
    let id = if dsx != 0 { 15 - id } else { id };
    let id = if id == 8 { 7 } else { id };
    let mut sign = (sp & 128) >> 7;
    let mut mask = sp & 127;
    let towards_positive = match id.cmp(&im) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => return sp,
    };
    match (towards_positive, sign == 1, mask) {
        (true, true, 0) => sign = 0,
        (true, true, _) | (false, false, 1..) => mask -= 1,
        (true, false, ..127) | (false, true, ..127) => mask += 1,
        (false, false, 0) => sign = 1,
        _ => {}
    }
    mask + (sign << 7)
}

impl G726 {
    fn decode_alaw(&mut self, code: u8) -> u16 {
        let prediction = self.predict();
        let sr = self.update(u32::from(code), prediction);
        let sp = compress_alaw(sr);
        let (dlx, dsx) = log(subta(expand_alaw(sp), prediction.se));
        sync_alaw(u32::from(code), sp, subtb(dlx, prediction.y), dsx) ^ ALAW_EVEN_BITS
    }

    fn encode_alaw(&mut self, code: u16) -> u8 {
        self.encode_uniform(expand_alaw(code ^ ALAW_EVEN_BITS))
    }
}

fn vector(dir: &Path, name: &str) -> Vec<u16> {
    let text = std::fs::read_to_string(dir.join(name)).expect("ITU test vector");
    text.split_whitespace()
        .filter(|line| line.len() == 64)
        .flat_map(|line| {
            line.as_bytes()
                .chunks(2)
                .map(|pair| {
                    u16::from_str_radix(std::str::from_utf8(pair).expect("ascii"), 16).expect("hex")
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn tone(freq: f64, amplitude: f64, len: usize) -> Vec<i16> {
    (0..len)
        .map(|n| (amplitude * (TAU * freq * n as f64 / 8_000.0).sin()) as i16)
        .collect()
}

fn snr_db(reference: &[i16], decoded: &[i16]) -> f64 {
    let signal: f64 = reference.iter().map(|&s| f64::from(s).powi(2)).sum();
    let noise: f64 = reference
        .iter()
        .zip(decoded)
        .map(|(&a, &b)| (f64::from(a) - f64::from(b)).powi(2))
        .sum();
    10.0 * (signal / noise).log10()
}

#[test]
fn a_tone_survives_the_round_trip() {
    let input = tone(1_020.0, 8_000.0, 8_000);
    let mut encoder = G726::default();
    let mut decoder = G726::default();
    let decoded: Vec<i16> = input
        .iter()
        .map(|&sample| decoder.decode(encoder.encode(sample)) << 2)
        .collect();
    let snr = snr_db(&input[800..], &decoded[800..]);
    assert!(snr > 20.0, "round trip SNR {snr:.1} dB");
}

#[test]
fn decoding_is_deterministic_from_the_reset_state() {
    let codes: Vec<u8> = (0..400u32).map(|n| ((n * 7 + n / 3) % 16) as u8).collect();
    let mut first = G726::default();
    let mut second = G726::default();
    let a: Vec<i16> = codes.iter().map(|&code| first.decode(code)).collect();
    let b: Vec<i16> = codes.iter().map(|&code| second.decode(code)).collect();
    assert_eq!(a, b);
    first.reset();
    let again: Vec<i16> = codes.iter().map(|&code| first.decode(code)).collect();
    assert_eq!(a, again);
}

#[test]
fn the_idle_code_decays_to_silence() {
    let mut decoder = G726::default();
    let mut encoder = G726::default();
    let idle = encoder.encode(0);
    let tail: Vec<i16> = (0..800).map(|_| decoder.decode(idle)).collect();
    assert!(
        tail[400..]
            .iter()
            .all(|&s| f32::from(s).abs() < FULL_SCALE * 0.01),
        "idle code did not settle: {:?}",
        &tail[790..]
    );
}

#[test]
fn known_answer_matches_the_decoder_verified_against_itu_vectors() {
    let codes: Vec<u8> = (0..32u8).map(|n| (n * 5 + n / 4 + 3) % 16).collect();
    let mut decoder = G726::default();
    let alaw: Vec<u16> = codes
        .iter()
        .map(|&code| decoder.decode_alaw(code))
        .collect();
    assert_eq!(alaw, KNOWN_ALAW);
}

const KNOWN_ALAW: [u16; 32] = [
    0xD4, 0x50, 0x54, 0xD4, 0x50, 0x57, 0xD7, 0xDD, 0x50, 0xD6, 0xC6, 0x4F, 0xDB, 0xE6, 0x6F, 0xCE,
    0x85, 0x03, 0xE3, 0xB1, 0x09, 0xFE, 0xBD, 0x31, 0xFC, 0xB8, 0x32, 0x7E, 0xA5, 0x3F, 0x6C, 0xBB,
];

fn itu_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("G726_VECTORS").expect("G726_VECTORS"))
}

#[test]
#[ignore = "needs G726_VECTORS=<ITU-T G.726 Appendix II DISK2 directory>"]
fn reproduces_the_itu_reset_vectors_bit_for_bit() {
    let dir = itu_dir();
    for (input, expected) in [
        ("RESET/32/RN32FA.I", "RESET/32/RN32FA.O"),
        ("RESET/32/RV32FA.I", "RESET/32/RV32FA.O"),
        ("INPUT/I32", "RESET/32/RI32FA.O"),
    ] {
        let codes = vector(&dir, input);
        let wanted = vector(&dir, expected);
        let mut decoder = G726::default();
        let got: Vec<u16> = codes
            .iter()
            .map(|&code| decoder.decode_alaw(code as u8))
            .collect();
        let first_miss = got.iter().zip(&wanted).position(|(a, b)| a != b);
        assert_eq!(first_miss, None, "{input} diverged from {expected}");
        assert_eq!(got.len(), wanted.len(), "{input}");
    }
    for (input, expected) in [
        ("INPUT/NRM.A", "RESET/32/RN32FA.I"),
        ("INPUT/OVR.A", "RESET/32/RV32FA.I"),
    ] {
        let pcm = vector(&dir, input);
        let wanted = vector(&dir, expected);
        let mut encoder = G726::default();
        let got: Vec<u16> = pcm
            .iter()
            .map(|&code| u16::from(encoder.encode_alaw(code)))
            .collect();
        let first_miss = got.iter().zip(&wanted).position(|(a, b)| a != b);
        assert_eq!(first_miss, None, "{input} diverged from {expected}");
    }
}
