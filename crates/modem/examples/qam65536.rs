//! Demodulates one of the 65536-QAM test files from github.com/ibelinp/65536qam and counts the
//! errors against its answer key.
//!
//!     cargo run --release -p sdrmm-modem --example qam65536 -- <file.wav> <symbols.csv> [passes]
//!
//! The receiver is blind. Every payload point is sent once per frame, so the key lines the
//! output up by looking decided points up in it.

use std::{collections::HashMap, env, error::Error, fs, time::Instant};

use num_complex::Complex;
use sdrmm_modem::{
    constellation::tables,
    linear::{DEFAULT_EQUALISER_TAPS, DenseQamDemod},
    pulse::{self, Norm},
};

const PREAMBLE: usize = 64;
const LEVEL_SCALE: f32 = 209.023_92;
const SYMBOL_RATE: f64 = 12_000.0;
const ROLL_OFF: f64 = 0.35;
const PROBE: usize = 2_000;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn chunk_u32(raw: &[u8], at: usize) -> Result<u32> {
    let bytes = raw.get(at..at + 4).ok_or("truncated WAV")?;
    Ok(u32::from_le_bytes(bytes.try_into()?))
}

/// Sample rate and I/Q of a 16-bit stereo WAV.
fn read_wav(path: &str) -> Result<(u32, Vec<Complex<f32>>)> {
    let raw = fs::read(path)?;
    let (mut at, mut rate) = (12, 0);
    while at + 8 <= raw.len() {
        let size = chunk_u32(&raw, at + 4)? as usize;
        match &raw[at..at + 4] {
            b"fmt " => rate = chunk_u32(&raw, at + 12)?,
            b"data" => {
                let data = raw.get(at + 8..at + 8 + size).ok_or("truncated data")?;
                let sample = |b: &[u8]| f32::from(i16::from_le_bytes([b[0], b[1]]));
                let iq = data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| Complex::new(sample(&c[..2]), sample(&c[2..])))
                    .collect();
                return Ok((rate, iq));
            }
            _ => {}
        }
        at += 8 + size + (size & 1);
    }
    Err("no data chunk".into())
}

/// The sent I and Q levels, -255 to 255.
fn read_key(path: &str) -> Result<Vec<(i32, i32)>> {
    fs::read_to_string(path)?
        .lines()
        .skip(1)
        .map(|line| {
            let fields: Vec<&str> = line.split(',').collect();
            let level =
                |k: usize| -> Result<i32> { Ok(fields.get(k).ok_or("short row")?.parse()?) };
            Ok((level(1)?, level(2)?))
        })
        .collect()
}

fn level(v: f32) -> i32 {
    let index = f32::midpoint(v * LEVEL_SCALE, 255.0)
        .round()
        .clamp(0.0, 255.0) as i32;
    2 * index - 255
}

fn decide(z: Complex<f32>) -> (i32, i32) {
    (level(z.re), level(z.im))
}

/// The quarter turn and frame offset under which most of `probe` lands on the key.
fn align(probe: &[Complex<f32>], key: &[(i32, i32)]) -> (Complex<f32>, usize) {
    let index: HashMap<(i32, i32), usize> = key
        .iter()
        .enumerate()
        .skip(PREAMBLE)
        .map(|(k, &p)| (p, k))
        .collect();
    let turns =
        [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)].map(|(re, im)| Complex::new(re, im));
    let mut votes: HashMap<(usize, usize), usize> = HashMap::new();
    for (turn, &r) in turns.iter().enumerate() {
        for (n, &z) in probe.iter().enumerate() {
            if let Some(&k) = index.get(&decide(z * r)) {
                *votes
                    .entry((turn, (k + key.len() - n % key.len()) % key.len()))
                    .or_default() += 1;
            }
        }
    }
    let ((turn, offset), _) = votes
        .into_iter()
        .max_by_key(|&(_, count)| count)
        .unwrap_or(((0, 0), 0));
    (turns[turn], offset)
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let (wav, csv) = (args.get(1).ok_or("no WAV")?, args.get(2).ok_or("no key")?);
    let passes: usize = args.get(3).map_or(Ok(1), |s| s.parse())?;
    let (rate, iq) = read_wav(wav)?;
    let key = read_key(csv)?;
    let sps = f64::from(rate) / SYMBOL_RATE;
    let filter = pulse::root_raised_cosine(sps, ROLL_OFF, 16, Norm::Energy);
    let table = tables::qam_square(65_536)?;
    let mut demod = DenseQamDemod::new(table, &filter, sps, DEFAULT_EQUALISER_TAPS)?;
    let mut out = Vec::new();
    let started = Instant::now();
    for _ in 0..passes {
        for chunk in iq.chunks(8192) {
            demod.process(chunk, &mut out);
        }
    }
    let seconds = started.elapsed().as_secs_f64();
    println!(
        "{} symbols, {:.0} per second, {:?}",
        out.len(),
        out.len() as f64 / seconds,
        demod.state()
    );
    let tail = &out[out.len() / 2..];
    let (turn, offset) = align(&tail[tail.len() - PROBE..], &key);
    let base = (offset + key.len() - (tail.len() - PROBE) % key.len()) % key.len();
    let (mut wrong, mut signal, mut error) = (0usize, 0.0f64, 0.0f64);
    for (n, &z) in tail.iter().enumerate() {
        let (i, q) = key[(base + n) % key.len()];
        let want = Complex::new(i as f32, q as f32) / LEVEL_SCALE;
        let got = z * turn;
        wrong += usize::from(decide(got) != (i, q));
        signal += f64::from(want.norm_sqr());
        error += f64::from((got - want).norm_sqr());
    }
    println!(
        "second half: {wrong} of {} symbols wrong, MER {:.1} dB",
        tail.len(),
        10.0 * (signal / error).log10()
    );
    Ok(())
}
