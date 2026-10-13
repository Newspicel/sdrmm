mod levels;

use std::{ops::Range, sync::LazyLock};

use num_complex::Complex;
use sdrmm_dsp::airtime::PowerFrames;
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent, DecoderFamily, EventKind,
    MAX_ENR_DB, MEASUREMENT_REPORT_MS, MIN_ENR_DB, NoiseFigureParams, NoiseFigureReport,
};

use crate::{ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_input_rate};
use levels::{Levels, Pair};

const RATE: f64 = 500_000.0;
const FFT_LEN: usize = 512;
const BIN_HZ: f64 = RATE / FFT_LEN as f64;
const MEASURED_HALF_HZ: f64 = 200_000.0;
const ARTIFACT_GUARD_HZ: f64 = 5_000.0;
const BLOCK_S: f64 = 0.1;
const BLOCK_FRAMES: u32 = (BLOCK_S * RATE / FFT_LEN as f64) as u32;
const REPORT_BLOCKS: u32 = (MEASUREMENT_REPORT_MS as f64 / 1_000.0 / BLOCK_S) as u32;
const REFERENCE_K: f64 = 290.0;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "noise_figure".to_owned(),
    name: "Noise figure".to_owned(),
    summary: "Receiver noise figure by the Y-factor method".to_owned(),
    family: DecoderFamily::Utility,
    bandwidth_hz: 2.0 * MEASURED_HALF_HZ,
    input_rate_hz: RATE,
    has_audio: false,
    emits: vec![EventKind::NoiseFigure],
    ..ChannelDescriptor::default()
});

fn params(settings: &ChannelSettings) -> Result<NoiseFigureParams, ChannelError> {
    match &settings.params {
        ChannelParams::NoiseFigure(params)
            if params.enr_db.is_finite() && (MIN_ENR_DB..=MAX_ENR_DB).contains(&params.enr_db) =>
        {
            Ok(*params)
        }
        ChannelParams::NoiseFigure(params) => Err(ChannelError::InvalidSettings(format!(
            "ENR must be {MIN_ENR_DB}..={MAX_ENR_DB} dB, got {}",
            params.enr_db
        ))),
        other => Err(ChannelError::InvalidSettings(format!(
            "noise figure channel got {} params",
            other.type_id()
        ))),
    }
}

pub(crate) fn occupied_band() -> (f64, f64) {
    (-MEASURED_HALF_HZ, MEASURED_HALF_HZ)
}

pub(crate) fn channel_filter() -> ChannelFilter {
    ChannelFilter::Passthrough
}

fn bin_of(offset_hz: f64) -> f64 {
    offset_hz / BIN_HZ + (FFT_LEN / 2) as f64
}

fn measured_bins() -> Range<usize> {
    let first = bin_of(-MEASURED_HALF_HZ).ceil() as usize;
    let last = bin_of(MEASURED_HALF_HZ).floor() as usize;
    first..last + 1
}

fn guarded_bins(artifact_hz: Option<f64>) -> Range<usize> {
    artifact_hz.map_or(0..0, |at| {
        let first = bin_of(at - ARTIFACT_GUARD_HZ).ceil().max(0.0) as usize;
        let last = bin_of(at + ARTIFACT_GUARD_HZ).floor().max(0.0) as usize;
        first..(last + 1).min(FFT_LEN)
    })
}

#[must_use]
pub(crate) fn noise_factor(pair: Pair, enr_db: f32) -> Option<f64> {
    let y = pair.y();
    let factor = 10f64.powf(f64::from(enr_db) / 10.0) / (y - 1.0);
    (y > 1.0 && factor >= 1.0).then_some(factor)
}

fn db(power: f64) -> f32 {
    (10.0 * power.log10()) as f32
}

struct Meter {
    enr_db: f32,
    guarded: Range<usize>,
    block_power: f64,
    block_frames: u32,
    blocks: u32,
    levels: Levels,
}

impl Meter {
    fn new(enr_db: f32) -> Self {
        Self {
            enr_db,
            guarded: 0..0,
            block_power: 0.0,
            block_frames: 0,
            blocks: 0,
            levels: Levels::default(),
        }
    }

    fn reset(&mut self) {
        self.block_power = 0.0;
        self.block_frames = 0;
        self.blocks = 0;
        self.levels.reset();
    }

    fn frame(&mut self, power: &[f32], out: &mut ChannelOutputs) {
        self.block_power += measured_bins()
            .filter(|bin| !self.guarded.contains(bin))
            .map(|bin| f64::from(power[bin]))
            .sum::<f64>();
        self.block_frames += 1;
        if self.block_frames == BLOCK_FRAMES {
            self.end_block(out);
        }
    }

    fn end_block(&mut self, out: &mut ChannelOutputs) {
        self.levels
            .block(self.block_power / f64::from(self.block_frames));
        self.block_power = 0.0;
        self.block_frames = 0;
        self.blocks += 1;
        if self.blocks.is_multiple_of(REPORT_BLOCKS)
            && let Some(report) = self.report()
        {
            out.events.push(DecoderEvent::NoiseFigure(report));
        }
    }

    fn report(&self) -> Option<NoiseFigureReport> {
        let level = self.levels.current()?;
        let pair = self.levels.pair();
        let factor = pair.and_then(|pair| noise_factor(pair, self.enr_db));
        Some(NoiseFigureReport {
            window_ms: MEASUREMENT_REPORT_MS,
            level_dbfs: db(level),
            settled_s: (f64::from(self.levels.settled_blocks()) * BLOCK_S) as f32,
            source_on: self.levels.source_on(),
            hot_dbfs: pair.map(|pair| db(pair.hot)),
            cold_dbfs: pair.map(|pair| db(pair.cold)),
            y_db: pair.map(|pair| db(pair.y())),
            nf_db: factor.map(db),
            temperature_k: factor.map(|factor| (REFERENCE_K * (factor - 1.0)) as f32),
        })
    }
}

pub struct NoiseFigureChannel {
    frequency_hz: f64,
    artifact_hz: Option<f64>,
    frames: PowerFrames,
    meter: Meter,
}

impl NoiseFigureChannel {
    fn reset(&mut self) {
        self.frames.reset();
        self.meter.reset();
    }
}

impl ChannelRx for NoiseFigureChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        Ok(Self {
            meter: Meter::new(params(&settings)?.enr_db),
            frequency_hz: settings.frequency_hz,
            artifact_hz: None,
            frames: PowerFrames::new(FFT_LEN),
        })
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        self.meter.enr_db = params(&settings)?.enr_db;
        if settings.frequency_hz != self.frequency_hz {
            self.frequency_hz = settings.frequency_hz;
            self.reset();
        }
        Ok(())
    }

    fn retuned(&mut self) {
        self.reset();
    }

    fn lo_artifact_at(&mut self, offset_hz: Option<f64>) {
        if offset_hz != self.artifact_hz {
            self.artifact_hz = offset_hz;
            self.meter.guarded = guarded_bins(offset_hz);
            self.reset();
        }
    }

    fn needs_gated_input(&self) -> bool {
        false
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let Self { frames, meter, .. } = self;
        frames.process(iq, |power| meter.frame(power, out));
    }
}

#[cfg(test)]
mod tests;
