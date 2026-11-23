use std::{
    collections::HashMap,
    env,
    error::Error,
    fs::File,
    io::{BufReader, Read},
    time::{Duration, Instant},
};

use num_complex::Complex;
use sdrmm_channels::dvbt2::receiver::Receiver;
use sdrmm_dsp::FracResampler;

const PACKET: usize = 188;
const CHUNK: usize = 1 << 16;
const USAGE: &str = "usage: dvbt2_iq <cf32_le file> <expected ts> <rate> [snr dB] [offset Hz] [clock ppm] \
                     [echo samples] [echo gain]";

/// Channel impairments applied to the recording before the receiver sees it.
struct Impairments {
    snr_db: f64,
    offset: f64,
    ppm: f64,
    echo: usize,
    echo_gain: f32,
}

struct Args {
    iq: String,
    expected: String,
    rate: f64,
    impairments: Impairments,
}

fn arg(args: &[String], index: usize, default: f64) -> Result<f64, Box<dyn Error>> {
    Ok(args.get(index).map_or(Ok(default), |value| value.parse())?)
}

fn args() -> Result<Args, Box<dyn Error>> {
    let args: Vec<_> = env::args().collect();
    if !(4..=9).contains(&args.len()) {
        return Err(USAGE.into());
    }
    let rate: f64 = args[3].parse()?;
    Ok(Args {
        iq: args[1].clone(),
        expected: args[2].clone(),
        rate,
        impairments: Impairments {
            snr_db: arg(&args, 4, f64::INFINITY)?,
            offset: arg(&args, 5, 0.0)? / rate,
            ppm: arg(&args, 6, 0.0)?,
            echo: arg(&args, 7, 0.0)? as usize,
            echo_gain: arg(&args, 8, 0.0)? as f32,
        },
    })
}

/// Seeded Gaussian noise, so every run of a setting sees the same channel.
struct Noise(u64);

impl Noise {
    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }

    fn gaussian(&mut self, sigma: f64) -> Complex<f32> {
        let radius = (-2.0 * self.uniform().ln()).sqrt() * sigma;
        let angle = std::f64::consts::TAU * self.uniform();
        Complex::new((radius * angle.cos()) as f32, (radius * angle.sin()) as f32)
    }
}

struct Channel {
    impairments: Impairments,
    noise: Noise,
    sigma: Option<f64>,
    clock: Option<FracResampler>,
    history: Vec<Complex<f32>>,
    index: u64,
    trimmed: Vec<Complex<f32>>,
}

impl Channel {
    fn new(impairments: Impairments) -> Result<Self, Box<dyn Error>> {
        let clock = if impairments.ppm == 0.0 {
            None
        } else {
            let mut clock = FracResampler::trimmable(0.9);
            clock
                .set_ratio(1.0 + impairments.ppm * 1e-6)
                .map_err(|_| "clock ratio")?;
            Some(clock)
        };
        Ok(Self {
            history: vec![Complex::default(); impairments.echo],
            impairments,
            noise: Noise(0x9e37_79b9_7f4a_7c15),
            sigma: None,
            clock,
            index: 0,
            trimmed: Vec::with_capacity(2 * CHUNK),
        })
    }

    fn apply(&mut self, samples: &mut Vec<Complex<f32>>) {
        self.echo(samples);
        if let Some(clock) = &mut self.clock {
            clock.process(samples, &mut self.trimmed);
            std::mem::swap(samples, &mut self.trimmed);
        }
        let power = samples.iter().map(|p| f64::from(p.norm_sqr())).sum::<f64>()
            / samples.len().max(1) as f64;
        let snr = self.impairments.snr_db;
        let sigma = *self
            .sigma
            .get_or_insert_with(|| (power / 10f64.powf(snr / 10.0) / 2.0).sqrt());
        for sample in samples.iter_mut() {
            let turn = (self.impairments.offset * self.index as f64).fract();
            let rotation = Complex::from_polar(1.0, (std::f64::consts::TAU * turn) as f32);
            *sample *= rotation;
            if sigma > 0.0 {
                *sample += self.noise.gaussian(sigma);
            }
            self.index += 1;
        }
    }

    fn echo(&mut self, samples: &mut [Complex<f32>]) {
        let delay = self.impairments.echo;
        if delay == 0 || self.impairments.echo_gain == 0.0 {
            return;
        }
        let mut joined = std::mem::take(&mut self.history);
        joined.extend_from_slice(samples);
        for (i, sample) in samples.iter_mut().enumerate() {
            *sample += joined[i] * self.impairments.echo_gain;
        }
        self.history = joined.split_off(joined.len() - delay);
    }
}

/// Lines the decoded stream up against the expected one, resyncing past gaps.
#[derive(Default)]
struct Tally {
    good: usize,
    bad: usize,
    missing: usize,
    next: Option<usize>,
}

impl Tally {
    fn add(
        &mut self,
        packet: &[u8; PACKET],
        expected: &[[u8; PACKET]],
        index: &HashMap<[u8; PACKET], Vec<usize>>,
    ) {
        if let Some(next) = self.next
            && expected.get(next) == Some(packet)
        {
            self.good += 1;
            self.next = Some(next + 1);
            return;
        }
        let from = self.next.unwrap_or(0);
        match index
            .get(packet)
            .and_then(|at| at.iter().find(|&&at| at >= from))
        {
            Some(&at) => {
                if self.next.is_some() {
                    self.missing += at - from;
                }
                self.good += 1;
                self.next = Some(at + 1);
            }
            None => self.bad += 1,
        }
    }
}

fn expected_packets(path: &str) -> Result<Vec<[u8; PACKET]>, Box<dyn Error>> {
    let mut bytes = Vec::new();
    File::open(path)?.read_to_end(&mut bytes)?;
    Ok(bytes.as_chunks::<PACKET>().0.to_vec())
}

fn read_chunk(
    reader: &mut impl Read,
    bytes: &mut [u8],
    out: &mut Vec<Complex<f32>>,
) -> Result<bool, Box<dyn Error>> {
    let mut count = 0;
    while count < bytes.len() {
        let read = reader.read(&mut bytes[count..])?;
        if read == 0 {
            break;
        }
        count += read;
    }
    out.clear();
    for pair in bytes[..count - count % 8].as_chunks::<8>().0 {
        let re = f32::from_le_bytes([pair[0], pair[1], pair[2], pair[3]]);
        let im = f32::from_le_bytes([pair[4], pair[5], pair[6], pair[7]]);
        out.push(Complex::new(re, im));
    }
    Ok(!out.is_empty())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = args()?;
    let expected = expected_packets(&args.expected)?;
    let mut index: HashMap<[u8; PACKET], Vec<usize>> = HashMap::new();
    for (at, packet) in expected.iter().enumerate() {
        index.entry(*packet).or_default().push(at);
    }
    let mut reader = BufReader::new(File::open(&args.iq)?);
    let mut bytes = vec![0u8; CHUNK * 8];
    let mut samples = Vec::with_capacity(2 * CHUNK);
    let mut channel = Channel::new(args.impairments)?;
    let mut receiver = Receiver::new(None)?;
    let mut packets = Vec::with_capacity(1 << 16);
    let mut tally = Tally::default();
    let (mut busy, mut total, mut first_packet) = (Duration::ZERO, 0usize, None);
    while read_chunk(&mut reader, &mut bytes, &mut samples)? {
        channel.apply(&mut samples);
        let started = Instant::now();
        receiver.push(&samples, &mut packets);
        busy += started.elapsed();
        total += samples.len();
        if first_packet.is_none() && !packets.is_empty() {
            first_packet = Some(total as f64 / args.rate);
        }
        for packet in packets.drain(..) {
            tally.add(&packet, &expected, &index);
        }
    }
    let seconds = total as f64 / args.rate;
    println!(
        "{{\"seconds\":{seconds:.3},\"realtime\":{:.2},\"lock_s\":{},\"good\":{},\"bad\":{},\"missing\":{},\"frames\":{},\"errors\":{},\"report\":\"{:?}\",\"last_error\":\"{:?}\",\"snr\":{:.1},\"clock_ppm\":{:.2}}}",
        seconds / busy.as_secs_f64(),
        first_packet.map_or("null".to_owned(), |s| format!("{s:.3}")),
        tally.good,
        tally.bad,
        tally.missing,
        receiver.frames,
        receiver.errors,
        receiver.report(),
        receiver.last_error,
        receiver.snr(),
        receiver.clock_error() * 1e6,
    );
    Ok(())
}
