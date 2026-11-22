use std::{thread, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
use sdrmm_decoder::{Channel, ChannelCtx, ChannelFilter, ChannelFront, ChannelOutputs};
use sdrmm_dsp::{Ddc, Decimator, design_lowpass};
use sdrmm_wire::{ChannelParams, ChannelSettings, Squelch};

use super::{convert::Fixture, signals::Signal, usage};

const SETTLE_POLL: Duration = Duration::from_millis(250);
const SETTLE_POLLS: usize = 240;
const IDLE_CPU_SECONDS: f64 = 0.002;

pub struct Decoded {
    pub keys: Vec<String>,
    pub cpu_seconds: f64,
}

pub struct FrontEnd {
    pub settings: ChannelSettings,
    pub input_rate: f64,
}

impl FrontEnd {
    pub fn new(signal: &Signal) -> Result<Self> {
        let settings: serde_json::Value = serde_json::from_str(signal.settings)
            .with_context(|| format!("settings for {}", signal.id))?;
        let params: ChannelParams = serde_json::from_value(
            serde_json::json!({ "type": signal.channel, "settings": settings }),
        )
        .with_context(|| format!("settings for {}", signal.channel))?;
        let input_rate = sdrmm_decoder::descriptors()
            .into_iter()
            .find(|d| d.type_id == signal.channel)
            .with_context(|| format!("no channel called {}", signal.channel))?
            .input_rate_hz;
        Ok(Self {
            settings: ChannelSettings {
                frequency_hz: signal.offset_hz,
                squelch: Squelch::Off,
                params,
                blanker: Default::default(),
                if_filter: Default::default(),
            },
            input_rate,
        })
    }

    pub fn ddc(&self, fixture_rate: f64) -> Result<Ddc> {
        Ddc::new(fixture_rate, self.input_rate, self.settings.frequency_hz)
            .map_err(|e| anyhow!("{e}"))
    }

    /// What a reference hears: the signal's occupied band, not our decoder's own filter, which
    /// may leave room for its AFC.
    pub fn reference_filter(&self) -> ChannelFilter {
        const TAPS: usize = 129;
        let (low, high) = sdrmm_decoder::occupied_band(&self.settings.params);
        let half = low.abs().max(high.abs());
        ChannelFilter::Symmetric(Decimator::new(
            &design_lowpass(TAPS, half / self.input_rate),
            1,
        ))
    }
}

pub fn decode(fixture: &Fixture, signal: &Signal, loops: usize) -> Result<Decoded> {
    let front = FrontEnd::new(signal)?;
    let mut channel_front = ChannelFront::new(
        &front.settings.params,
        fixture.rate,
        front.settings.frequency_hz,
    )?;
    let mut channel = sdrmm_decoder::create(
        ChannelCtx {
            input_rate: front.input_rate,
        },
        &front.settings,
    )?;
    let mut filtered = Vec::new();
    let mut out = ChannelOutputs::default();
    let mut keys = Vec::new();
    let start = usage::own()?;
    fixture.feed(loops, signal.tail_seconds, |block| {
        channel_front.process(block, &mut filtered);
        out.reset();
        channel.process(&filtered, &mut out);
        read(signal, &out, &mut keys);
        Ok(())
    })?;
    settle(channel.as_mut(), &mut out, &mut keys, signal)?;
    Ok(Decoded {
        keys: signal.key.finish(keys),
        cpu_seconds: usage::own()? - start,
    })
}

fn read(signal: &Signal, out: &ChannelOutputs, keys: &mut Vec<String>) {
    for event in &out.events {
        signal.key.read(event, keys);
    }
}

fn settle(
    channel: &mut dyn Channel,
    out: &mut ChannelOutputs,
    keys: &mut Vec<String>,
    signal: &Signal,
) -> Result<()> {
    for _ in 0..SETTLE_POLLS {
        let before = usage::own()?;
        thread::sleep(SETTLE_POLL);
        out.reset();
        channel.process(&[], out);
        read(signal, out, keys);
        if usage::own()? - before < IDLE_CPU_SECONDS {
            return Ok(());
        }
    }
    bail!("{} kept working long after its input ended", signal.name)
}
