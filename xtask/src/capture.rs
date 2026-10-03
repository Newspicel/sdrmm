use std::{
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, ensure};
use clap::Args;
use sdrmm_device::{DeviceDriver, RxSink, Sample};
use sdrmm_device_kiwisdr::KiwiSdrDriver;
use sdrmm_device_spyserver::SpyServerDriver;
use sdrmm_wire::{Agc, AgcSetting, Capabilities, DeviceSettings, GainValue};

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Driver {
    Kiwisdr,
    Spyserver,
}

#[derive(Args)]
pub struct NetCapture {
    pub host: String,
    #[arg(long, value_enum, default_value_t = Driver::Kiwisdr)]
    pub driver: Driver,
    #[arg(long)]
    pub rate: Option<f64>,
    #[arg(long)]
    pub center: f64,
    #[arg(long)]
    pub seconds: f64,
    #[arg(long)]
    pub out: PathBuf,
    #[arg(long)]
    pub gain: Option<f64>,
}

fn settings(args: &NetCapture, caps: &Capabilities) -> DeviceSettings {
    tuning(
        args,
        caps.gains.first().map(|stage| stage.name.as_str()),
        caps.agc != Agc::None,
    )
}

fn tuning(args: &NetCapture, gain_stage: Option<&str>, has_agc: bool) -> DeviceSettings {
    let gains = args
        .gain
        .zip(gain_stage)
        .map(|(value_db, stage)| {
            vec![GainValue {
                stage: stage.to_owned(),
                value_db,
            }]
        })
        .unwrap_or_default();
    DeviceSettings {
        center_hz: Some(args.center),
        sample_rate: args.rate,
        agc: has_agc.then(|| AgcSetting::switched(args.gain.is_none())),
        gains,
        ..DeviceSettings::default()
    }
}

fn driver(kind: Driver) -> Box<dyn DeviceDriver> {
    match kind {
        Driver::Kiwisdr => Box::new(KiwiSdrDriver::new()),
        Driver::Spyserver => Box::new(SpyServerDriver::new()),
    }
}

pub fn run(args: &NetCapture) -> Result<()> {
    ensure!(args.seconds > 0.0, "--seconds must be positive");
    let driver = driver(args.driver);
    let info = driver
        .resolve(&args.host)
        .with_context(|| format!("{} is not a {} address", args.host, driver.id()))?;
    let mut device = driver.open(&info).map_err(|e| anyhow!("open: {e}"))?;
    device
        .apply(&settings(args, &device.capabilities().clone()))
        .map_err(|e| anyhow!("tune: {e}"))?;
    let rate = device.settings().sample_rate.context("no sample rate")?;
    let mut writer = sdrmm_recorder::SigmfWriter::create(&args.out, rate, args.center, &info.label)
        .with_context(|| format!("create {}", args.out.display()))?;

    let (tx, rx) = mpsc::channel::<Result<(u64, Vec<Sample>), String>>();
    let fatal = tx.clone();
    device
        .rx_start(vec![RxSink::with_fatal_handler(
            move |samples, index| {
                let _ = tx.send(Ok((index, samples.to_vec())));
            },
            move |e| {
                let _ = fatal.send(Err(e.to_string()));
            },
        )])
        .map_err(|e| anyhow!("stream: {e}"))?;

    let want = (args.seconds * rate).round() as u64;
    let deadline = Instant::now() + Duration::from_secs_f64(args.seconds * 2.0 + 10.0);
    let mut written = 0u64;
    let mut gaps = 0u64;
    let mut failure = String::from("no samples for 5 s");
    while written < want && Instant::now() < deadline {
        let (index, block) = match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(delivered)) => delivered,
            Ok(Err(reason)) => {
                failure = reason;
                break;
            }
            Err(_) => break,
        };
        let missing = index.saturating_sub(written).min(want - written);
        if missing > 0 {
            gaps += missing;
            writer.write_block(&vec![Sample::default(); missing as usize])?;
            written += missing;
        }
        let take = block.len().min((want - written) as usize);
        writer.write_block(&block[..take])?;
        written += take as u64;
    }
    device.rx_stop();
    writer.finalize()?;
    ensure!(
        written == want,
        "stream ended after {written} of {want} samples: {failure}"
    );
    println!(
        "{written} samples at {rate} Hz, centre {} Hz, {gaps} lost samples zero-filled",
        args.center
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(gain: Option<f64>) -> NetCapture {
        NetCapture {
            host: "kiwi.local".into(),
            driver: Driver::Spyserver,
            rate: Some(312_500.0),
            center: 162e6,
            seconds: 1.0,
            out: PathBuf::from("capture"),
            gain,
        }
    }

    #[test]
    fn a_gain_lands_on_the_radios_own_stage_and_turns_agc_off() {
        let tuned = tuning(&args(Some(16.0)), Some("tuner"), true);
        assert_eq!(
            tuned.gains,
            vec![GainValue {
                stage: "tuner".into(),
                value_db: 16.0
            }]
        );
        assert_eq!(tuned.agc, Some(AgcSetting::switched(false)));
        assert_eq!(tuned.sample_rate, Some(312_500.0));
    }

    #[test]
    fn a_radio_without_agc_is_never_sent_one() {
        let tuned = tuning(&args(None), None, false);
        assert!(tuned.gains.is_empty());
        assert_eq!(tuned.agc, None);
    }
}
