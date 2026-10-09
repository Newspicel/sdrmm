mod classify;
mod tally;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_dsp::{
    airtime::{Burst, Bursts, PowerFrames, QuietFloor, band_degrees},
    fastmath::{fast_db_to_power, fast_power_db},
};
use sdrmm_wire::{
    AIRTIME_REPORT_MS, ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent,
    DecoderFamily, IsmSurveyParams, IsmSurveyReport,
};

use self::{
    classify::{Shape, classify},
    tally::Tally,
};
use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx,
    airtime::{
        self, BIN_HZ, FLOOR_RISE_DB, REPORT_FRAMES, WARMUP_FRAMES, bin_offset_hz, bins_between,
        fft_len, frame_s, gated, threshold, usable_half_hz,
    },
    check_rate,
};

const FLOOR_STRIDE: u64 = 8;
const TRACKS: usize = 48;
const LONGEST_S: f64 = 50e-3;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "ism_survey".to_owned(),
    name: "2.4 GHz survey".to_owned(),
    summary: "Wi-Fi, Bluetooth, 802.15.4 and ovens by airtime, energy only".to_owned(),
    family: DecoderFamily::Utility,
    bandwidth_hz: 2.0 * usable_half_hz(IsmSurveyParams::default().span),
    input_rate_hz: IsmSurveyParams::default().span.sample_rate_hz(),
    has_audio: false,
    decoder_kind: Some("ism_survey".to_owned()),
    ..ChannelDescriptor::default()
});

fn params(settings: &ChannelSettings) -> Result<IsmSurveyParams, ChannelError> {
    match &settings.params {
        ChannelParams::IsmSurvey(params) => {
            airtime::check_margin(params.margin_db)?;
            Ok(*params)
        }
        other => Err(ChannelError::InvalidSettings(format!(
            "2.4 GHz survey channel got {} params",
            other.type_id()
        ))),
    }
}

pub(crate) fn input_rate(params: &IsmSurveyParams) -> f64 {
    params.span.sample_rate_hz()
}

pub(crate) fn occupied_band(params: &IsmSurveyParams) -> (f64, f64) {
    airtime::occupied_band(params.span)
}

pub(crate) fn channel_filter(_: &IsmSurveyParams) -> ChannelFilter {
    airtime::channel_filter()
}

struct Floors {
    bins: Vec<QuietFloor>,
    linear: Vec<f32>,
    usable: std::ops::Range<usize>,
    sorted: Vec<f32>,
}

impl Floors {
    fn new(params: &IsmSurveyParams) -> Self {
        let len = fft_len(params.span);
        let half = usable_half_hz(params.span);
        Self {
            bins: (0..len)
                .map(|_| QuietFloor::new(band_degrees(1), FLOOR_RISE_DB))
                .collect(),
            linear: vec![f32::INFINITY; len],
            usable: bins_between(-half, half, len),
            sorted: Vec::with_capacity(len),
        }
    }

    fn observe(&mut self, power: &[f32]) {
        for bin in self.usable.clone() {
            self.bins[bin].observe(fast_power_db(power[bin]));
        }
    }

    fn settle(&mut self) {
        for bin in self.usable.clone() {
            self.linear[bin] = threshold(self.bins[bin].settle(), 0.0);
        }
    }

    fn typical_db(&mut self) -> Option<f32> {
        self.sorted.clear();
        self.sorted
            .extend(self.usable.clone().filter_map(|bin| self.bins[bin].floor_db()));
        if self.sorted.is_empty() {
            return None;
        }
        let middle = self.sorted.len() / 2;
        let (_, median, _) = self.sorted.select_nth_unstable_by(middle, f32::total_cmp);
        Some(*median)
    }

    fn span_db(&self) -> Option<f32> {
        let total: f32 = self
            .usable
            .clone()
            .filter_map(|bin| self.bins[bin].floor_db())
            .map(fast_db_to_power)
            .sum();
        (total > 0.0).then(|| fast_power_db(total))
    }

    fn raised(&self, typical_db: f32, margin_db: f32) -> impl Iterator<Item = (usize, f32)> + '_ {
        self.usable.clone().filter_map(move |bin| {
            let floor = self.bins[bin].floor_db()?;
            (floor >= typical_db + margin_db).then_some((bin, floor))
        })
    }
}

pub struct IsmSurveyChannel {
    params: IsmSurveyParams,
    frequency_hz: f64,
    frames: PowerFrames,
    floors: Floors,
    bursts: Bursts,
    tally: Tally,
    frame: u64,
    measured: u64,
    dropped: u64,
}

impl IsmSurveyChannel {
    fn rebuild(&mut self) {
        self.frames.reset();
        self.floors = Floors::new(&self.params);
        self.bursts.reset();
        self.tally = Tally::new(self.frequency_hz, self.params.span);
        self.frame = 0;
        self.measured = 0;
        self.dropped = self.bursts.dropped();
    }
}

fn shape(burst: &Burst, frequency_hz: f64, len: usize) -> Shape {
    Shape {
        centre_hz: frequency_hz + bin_offset_hz(f64::from(burst.centre), len),
        width_hz: (burst.high - burst.low + 1) as f64 * BIN_HZ,
        duration_s: burst.frames as f64 * frame_s(),
        edge: burst.edge,
        cut: burst.cut,
    }
}

impl ChannelRx for IsmSurveyChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        let params = params(&settings)?;
        check_rate(ctx, &DESCRIPTOR, input_rate(&params))?;
        let len = fft_len(params.span);
        Ok(Self {
            frames: PowerFrames::new(len),
            floors: Floors::new(&params),
            bursts: Bursts::new(len, TRACKS, (LONGEST_S / frame_s()) as u64),
            tally: Tally::new(settings.frequency_hz, params.span),
            frequency_hz: settings.frequency_hz,
            params,
            frame: 0,
            measured: 0,
            dropped: 0,
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
            frequency_hz,
            frames,
            floors,
            bursts,
            tally,
            frame,
            measured,
            dropped,
        } = self;
        let len = frames.len();
        let margin = fast_db_to_power(params.margin_db);
        frames.process(iq, |power| {
            *frame += 1;
            if !gated(power) {
                *measured += 1;
                if *frame % FLOOR_STRIDE == 0 {
                    floors.observe(power);
                }
                bursts.frame(power, &floors.linear, margin, |burst| {
                    tally.add(&burst, classify(&shape(&burst, *frequency_hz, len)));
                });
            }
            if *frame == WARMUP_FRAMES {
                floors.settle();
            }
            if *frame % REPORT_FRAMES == 0 {
                let floor_dbfs = floors.span_db();
                if let Some(typical) = floors.typical_db() {
                    tally.raised(floors.raised(typical, params.margin_db));
                }
                let lost = bursts.dropped() - *dropped;
                *dropped = bursts.dropped();
                out.events.push(DecoderEvent::IsmSurvey(IsmSurveyReport {
                    window_ms: AIRTIME_REPORT_MS,
                    low_hz: *frequency_hz - usable_half_hz(params.span),
                    high_hz: *frequency_hz + usable_half_hz(params.span),
                    measured: (*measured as f64 / REPORT_FRAMES as f64) as f32,
                    busy: tally.busy(*measured),
                    floor_dbfs,
                    kinds: tally.report(*measured),
                    dropped: u32::try_from(lost).unwrap_or(u32::MAX),
                }));
                floors.settle();
                *measured = 0;
            }
        });
    }
}

#[cfg(test)]
mod tests;
