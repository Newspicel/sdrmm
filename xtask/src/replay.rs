use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use clap::Args;
use num_complex::Complex;
use sdrmm_channels::{ChannelCtx, ChannelOutputs};
use sdrmm_dsp::Ddc;
use sdrmm_wire::{ChannelParams, ChannelSettings};

use crate::excerpt::Source;

const BLOCK: usize = 65_536;

#[derive(Args)]
pub struct Replay {
    pub input: PathBuf,
    #[arg(long)]
    pub params: String,
    #[arg(long, default_value_t = 0.0)]
    pub offset: f64,
    #[arg(long)]
    pub input_rate: Option<f64>,
    #[arg(long)]
    pub center: Option<f64>,
    #[arg(long)]
    pub quiet_tail: Option<f64>,
    #[arg(long)]
    pub speed: Option<f64>,
    #[arg(long, default_value_t = 0.0)]
    pub settle: f64,
    #[arg(long)]
    pub images: Option<PathBuf>,
}

pub fn run(args: &Replay) -> Result<()> {
    let params: ChannelParams =
        serde_json::from_str(&args.params).context("read the channel parameters as JSON")?;
    let mut source = Source::open(&args.input, args.input_rate, args.center)?;
    let device_rate = source.rate;

    let type_id = params.type_id().to_owned();
    ensure!(
        sdrmm_channels::descriptors()
            .iter()
            .any(|d| d.type_id == type_id),
        "no channel called {type_id}"
    );
    let input_rate = sdrmm_channels::input_rate(&params);
    let settings = ChannelSettings {
        frequency_hz: source.center + args.offset,
        squelch: sdrmm_wire::Squelch::Off,
        params,
        blanker: Default::default(),
    };
    let mut ddc =
        Ddc::new(device_rate, input_rate, args.offset).map_err(|err| anyhow::anyhow!("{err}"))?;
    let mut filter = sdrmm_channels::channel_filter(&settings.params)?;
    let mut channel = sdrmm_channels::create(ChannelCtx { input_rate }, &settings)?;

    let mut block = vec![Complex::default(); BLOCK];
    let mut tuned = Vec::new();
    let mut filtered = Vec::new();
    let mut out = ChannelOutputs::default();
    let mut read = 0u64;
    let mut events = 0usize;
    let mut audio = 0usize;
    let tail = (args.quiet_tail.unwrap_or(0.0) * device_rate).round() as u64;
    let mut tail_left = tail;
    let started = Instant::now();
    let mut saved = 0usize;
    loop {
        let got = source.read(&mut block)?;
        let got = if got > 0 {
            read += got as u64;
            got
        } else if tail_left > 0 {
            let n = BLOCK.min(tail_left as usize);
            block[..n].fill(Complex::default());
            tail_left -= n as u64;
            n
        } else {
            break;
        };
        ddc.process(&block[..got], &mut tuned);
        filter.process(&tuned, &mut filtered);
        out.reset();
        channel.process(&filtered, &mut out);
        audio += out.audio_pcm.len();
        events += report(&out, read as f64 / device_rate);
        save_images(&out, args.images.as_deref(), &mut saved)?;
        if let Some(speed) = args.speed {
            pace(started, read as f64 / device_rate / speed);
        }
    }
    let settle_until = Instant::now() + Duration::from_secs_f64(args.settle);
    while Instant::now() < settle_until {
        std::thread::sleep(Duration::from_millis(100));
        out.reset();
        channel.process(&[], &mut out);
        events += report(&out, read as f64 / device_rate);
        save_images(&out, args.images.as_deref(), &mut saved)?;
    }
    println!(
        "{events} events, {audio} audio samples over {:.3} s of {type_id} at {device_rate} Hz",
        read as f64 / device_rate
    );
    Ok(())
}

fn report(out: &ChannelOutputs, at_s: f64) -> usize {
    for event in &out.events {
        println!("{at_s:9.4} s  {event:?}");
    }
    for image in &out.images {
        println!(
            "{at_s:9.4} s  image {}x{}",
            image.picture.width, image.picture.height
        );
    }
    out.events.len()
}

fn pace(started: Instant, due_s: f64) {
    let due = Duration::from_secs_f64(due_s);
    if let Some(wait) = due.checked_sub(started.elapsed()) {
        std::thread::sleep(wait);
    }
}

fn save_images(out: &ChannelOutputs, dir: Option<&Path>, saved: &mut usize) -> Result<()> {
    let Some(dir) = dir else {
        return Ok(());
    };
    for image in &out.images {
        *saved += 1;
        let path = dir.join(format!("{}-{saved}.png", image.source));
        let picture = &image.picture;
        let (color, data) = if picture.rgb.is_empty() {
            (png::ColorType::Grayscale, &picture.luma)
        } else {
            (png::ColorType::Rgb, &picture.rgb)
        };
        let file =
            std::fs::File::create(&path).with_context(|| format!("create {}", path.display()))?;
        let mut encoder = png::Encoder::new(
            std::io::BufWriter::new(file),
            u32::from(picture.width),
            u32::from(picture.height),
        );
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().context("write the png header")?;
        writer
            .write_image_data(data)
            .context("write the png data")?;
        writer.finish().context("finish the png")?;
        println!("saved {}", path.display());
    }
    Ok(())
}
