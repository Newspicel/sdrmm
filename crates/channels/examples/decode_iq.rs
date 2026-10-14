use std::{
    env,
    error::Error,
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
};

use num_complex::Complex;
use sdrmm_channels::{ChannelCtx, ChannelOutputs, channel_filter, create, input_rate};
use sdrmm_dsp::Ddc;
use sdrmm_wire::{ChannelParams, ChannelSettings};

const USAGE: &str = "usage: decode_iq <cf32_le file> <rate> <center Hz> <channel Hz> <type> \
                     [params json] [start s] [length s]";

struct Args {
    path: String,
    rate: f64,
    center: f64,
    settings: ChannelSettings,
    start: f64,
    length: f64,
}

fn args() -> Result<Args, Box<dyn Error>> {
    let args: Vec<_> = env::args().collect();
    if !(6..=9).contains(&args.len()) {
        return Err(USAGE.into());
    }
    let mut settings = ChannelSettings::default_for(&args[5]).ok_or("unknown channel type")?;
    if let Some(json) = args.get(6).filter(|json| !json.is_empty()) {
        let tagged = serde_json::json!({ "type": args[5], "settings": serde_json::from_str::<serde_json::Value>(json)? });
        settings.params = serde_json::from_value::<ChannelParams>(tagged)?;
    }
    settings.frequency_hz = args[4].parse()?;
    Ok(Args {
        path: args[1].clone(),
        rate: args[2].parse()?,
        center: args[3].parse()?,
        settings,
        start: args.get(7).map_or(Ok(0.0), |arg| arg.parse())?,
        length: args.get(8).map_or(Ok(f64::INFINITY), |arg| arg.parse())?,
    })
}

fn read_samples(
    reader: &mut impl Read,
    bytes: &mut [u8],
    samples: &mut Vec<Complex<f32>>,
) -> Result<usize, Box<dyn Error>> {
    let mut count = 0;
    while count < bytes.len() {
        let read = reader.read(&mut bytes[count..])?;
        if read == 0 {
            break;
        }
        count += read;
    }
    samples.clear();
    for sample in bytes[..count - count % 8].as_chunks::<8>().0 {
        samples.push(Complex::new(
            f32::from_le_bytes(sample[..4].try_into()?),
            f32::from_le_bytes(sample[4..].try_into()?),
        ));
    }
    Ok(samples.len())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = args()?;
    let channel_rate = input_rate(&args.settings.params);
    let mut decoder = create(
        ChannelCtx {
            input_rate: channel_rate,
        },
        &args.settings,
    )?;
    let mut filter = channel_filter(&args.settings.params)?;
    let mut ddc = Ddc::new(
        args.rate,
        channel_rate,
        args.settings.frequency_hz - args.center,
    )?;
    let mut reader = BufReader::new(File::open(&args.path)?);
    reader.seek(SeekFrom::Start((args.start * args.rate) as u64 * 8))?;
    let mut remaining = (args.length * args.rate).min(u64::MAX as f64) as u64;
    let mut position = args.start;
    let (mut tuned, mut filtered, mut samples) = (Vec::new(), Vec::new(), Vec::new());
    let mut out = ChannelOutputs::default();
    let mut bytes = vec![0u8; 1 << 20];
    while remaining > 0 && read_samples(&mut reader, &mut bytes, &mut samples)? > 0 {
        samples.truncate(remaining.min(samples.len() as u64) as usize);
        remaining -= samples.len() as u64;
        position += samples.len() as f64 / args.rate;
        ddc.process(&samples, &mut tuned);
        filter.process(&tuned, &mut filtered);
        out.reset();
        decoder.process(&filtered, &mut out);
        for event in &out.events {
            println!(
                "{}",
                serde_json::json!({ "at_s": position, "event": event })
            );
        }
    }
    Ok(())
}
