use std::{ops::Range, sync::LazyLock};

use num_complex::Complex;
use sdrmm_dsp::{
    airtime::{PowerFrames, QuietFloor, band_degrees},
    fastmath::fast_power_db,
};
use sdrmm_modem::wifi::channel::{self as plan, Channel, WifiBand as PlanBand};
use sdrmm_wire::{
    AIRTIME_REPORT_MS, ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent,
    DecoderFamily, WifiBand, WifiChannelLoad, WifiOccupancyParams, WifiOccupancyReport,
};

use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx,
    airtime::{
        self, FLOOR_RISE_DB, REPORT_FRAMES, WARMUP_FRAMES, bins_between, fft_len, gated,
        threshold, usable_half_hz,
    },
    check_rate,
};

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "wifi_occupancy".to_owned(),
    name: "Wi-Fi occupancy".to_owned(),
    summary: "Busy time of each Wi-Fi channel, energy only".to_owned(),
    family: DecoderFamily::Utility,
    bandwidth_hz: 2.0 * usable_half_hz(WifiOccupancyParams::default().span),
    input_rate_hz: WifiOccupancyParams::default().span.sample_rate_hz(),
    has_audio: false,
    decoder_kind: Some("wifi_occupancy".to_owned()),
    ..ChannelDescriptor::default()
});

fn params(settings: &ChannelSettings) -> Result<WifiOccupancyParams, ChannelError> {
    match &settings.params {
        ChannelParams::WifiOccupancy(params) => {
            airtime::check_margin(params.margin_db)?;
            Ok(*params)
        }
        other => Err(ChannelError::InvalidSettings(format!(
            "Wi-Fi occupancy channel got {} params",
            other.type_id()
        ))),
    }
}

pub(crate) fn input_rate(params: &WifiOccupancyParams) -> f64 {
    params.span.sample_rate_hz()
}

pub(crate) fn occupied_band(params: &WifiOccupancyParams) -> (f64, f64) {
    airtime::occupied_band(params.span)
}

pub(crate) fn channel_filter(_: &WifiOccupancyParams) -> ChannelFilter {
    airtime::channel_filter()
}

struct Lane {
    channel: Channel,
    bins: Range<usize>,
    floor: QuietFloor,
    threshold: f32,
    evaluated: u64,
    busy: u64,
    busy_power: f64,
    peak: f32,
}

impl Lane {
    fn new(channel: Channel, frequency_hz: f64, len: usize) -> Self {
        let half = plan::OCCUPIED_HZ / 2.0;
        let offset = channel.centre_hz - frequency_hz;
        let bins = bins_between(offset - half, offset + half, len);
        Self {
            channel,
            floor: QuietFloor::new(band_degrees(bins.len()), FLOOR_RISE_DB),
            bins,
            threshold: f32::INFINITY,
            evaluated: 0,
            busy: 0,
            busy_power: 0.0,
            peak: 0.0,
        }
    }

    fn frame(&mut self, power: &[f32]) {
        let band: f32 = power[self.bins.clone()].iter().sum();
        self.floor.observe(fast_power_db(band));
        if self.threshold.is_finite() {
            self.evaluated += 1;
            if band > self.threshold {
                self.busy += 1;
                self.busy_power += f64::from(band);
                self.peak = self.peak.max(band);
            }
        }
    }

    fn settle(&mut self, margin_db: f32) {
        self.threshold = threshold(self.floor.settle(), margin_db);
    }

    fn load(&mut self) -> WifiChannelLoad {
        let load = WifiChannelLoad {
            band: match self.channel.band {
                PlanBand::Ghz2_4 => WifiBand::Ghz2_4,
                PlanBand::Ghz5 => WifiBand::Ghz5,
                PlanBand::Ghz6 => WifiBand::Ghz6,
            },
            number: self.channel.number,
            centre_hz: self.channel.centre_hz,
            busy: (self.busy as f64 / self.evaluated.max(1) as f64) as f32,
            level_dbfs: (self.busy > 0)
                .then(|| fast_power_db((self.busy_power / self.busy as f64) as f32)),
            peak_dbfs: (self.busy > 0).then(|| fast_power_db(self.peak)),
            floor_dbfs: self.floor.floor_db(),
        };
        self.evaluated = 0;
        self.busy = 0;
        self.busy_power = 0.0;
        self.peak = 0.0;
        load
    }
}

fn lanes(params: &WifiOccupancyParams, frequency_hz: f64) -> Vec<Lane> {
    let half = usable_half_hz(params.span);
    let len = fft_len(params.span);
    plan::twenty_mhz_within(frequency_hz - half, frequency_hz + half)
        .map(|channel| Lane::new(channel, frequency_hz, len))
        .collect()
}

pub struct WifiOccupancyChannel {
    params: WifiOccupancyParams,
    frequency_hz: f64,
    frames: PowerFrames,
    lanes: Vec<Lane>,
    frame: u64,
    measured: u64,
}

impl WifiOccupancyChannel {
    fn rebuild(&mut self) {
        self.frames.reset();
        self.lanes = lanes(&self.params, self.frequency_hz);
        self.frame = 0;
        self.measured = 0;
    }
}

impl ChannelRx for WifiOccupancyChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        let params = params(&settings)?;
        check_rate(ctx, &DESCRIPTOR, input_rate(&params))?;
        Ok(Self {
            frames: PowerFrames::new(fft_len(params.span)),
            lanes: lanes(&params, settings.frequency_hz),
            frequency_hz: settings.frequency_hz,
            params,
            frame: 0,
            measured: 0,
        })
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = params(&settings)?;
        if params.span != self.params.span {
            return Err(ChannelError::InvalidSettings(
                "a new span changes the input rate; rebuild the channel".to_owned(),
            ));
        }
        let moved = settings.frequency_hz != self.frequency_hz;
        self.params = params;
        self.frequency_hz = settings.frequency_hz;
        if moved {
            self.rebuild();
        }
        Ok(())
    }

    fn retuned(&mut self) {
        self.rebuild();
    }

    fn needs_gated_input(&self) -> bool {
        false
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let Self {
            params,
            frames,
            lanes,
            frame,
            measured,
            ..
        } = self;
        frames.process(iq, |power| {
            *frame += 1;
            if !gated(power) {
                *measured += 1;
                lanes.iter_mut().for_each(|lane| lane.frame(power));
            }
            if *frame == WARMUP_FRAMES && lanes.iter().all(|lane| !lane.threshold.is_finite()) {
                lanes.iter_mut().for_each(|lane| lane.settle(params.margin_db));
            }
            if *frame % REPORT_FRAMES == 0 {
                out.events.push(DecoderEvent::WifiOccupancy(WifiOccupancyReport {
                    window_ms: AIRTIME_REPORT_MS,
                    measured: (*measured as f64 / REPORT_FRAMES as f64) as f32,
                    channels: lanes.iter_mut().map(Lane::load).collect(),
                }));
                lanes.iter_mut().for_each(|lane| lane.settle(params.margin_db));
                *measured = 0;
            }
        });
    }
}

#[cfg(test)]
mod tests;
