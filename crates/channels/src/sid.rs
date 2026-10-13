mod stations;
mod tracker;

use std::{ops::Range, sync::LazyLock};

use num_complex::Complex;
use sdrmm_dsp::airtime::PowerFrames;
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent, DecoderFamily, EventKind,
    MAX_SID_REPORT_S, MAX_SID_THRESHOLD_DB, MIN_SID_REPORT_S, MIN_SID_THRESHOLD_DB, SidParams,
    SidReport, SidStationLevel,
};

use crate::{ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_input_rate};
use stations::Station;
use tracker::Tracker;

const RATE: f64 = 48_000.0;
const FFT_LEN: usize = 4_096;
const BIN_HZ: f64 = RATE / FFT_LEN as f64;
const USABLE_HALF_HZ: f64 = 21_000.0;
pub(crate) const STATION_HALF_HZ: f64 = 50.0;
const ARTIFACT_GUARD_HZ: f64 = 100.0;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "sid".to_owned(),
    name: "SID monitor".to_owned(),
    summary: "Solar flares in VLF transmitter levels".to_owned(),
    family: DecoderFamily::Utility,
    bandwidth_hz: 2.0 * USABLE_HALF_HZ,
    input_rate_hz: RATE,
    has_audio: false,
    emits: vec![EventKind::Sid],
    ..ChannelDescriptor::default()
});

fn params(settings: &ChannelSettings) -> Result<SidParams, ChannelError> {
    let ChannelParams::Sid(params) = &settings.params else {
        return Err(ChannelError::InvalidSettings(format!(
            "SID channel got {} params",
            settings.params.type_id()
        )));
    };
    if !(MIN_SID_REPORT_S..=MAX_SID_REPORT_S).contains(&params.report_s) {
        return Err(ChannelError::InvalidSettings(format!(
            "report interval must be {MIN_SID_REPORT_S}..={MAX_SID_REPORT_S} s, got {}",
            params.report_s
        )));
    }
    if !(params.threshold_db.is_finite()
        && (MIN_SID_THRESHOLD_DB..=MAX_SID_THRESHOLD_DB).contains(&params.threshold_db))
    {
        return Err(ChannelError::InvalidSettings(format!(
            "threshold must be {MIN_SID_THRESHOLD_DB}..={MAX_SID_THRESHOLD_DB} dB, got {}",
            params.threshold_db
        )));
    }
    Ok(*params)
}

pub(crate) fn occupied_band() -> (f64, f64) {
    (-USABLE_HALF_HZ, USABLE_HALF_HZ)
}

pub(crate) fn channel_filter() -> ChannelFilter {
    ChannelFilter::Passthrough
}

fn bin_of(offset_hz: f64) -> f64 {
    offset_hz / BIN_HZ + (FFT_LEN / 2) as f64
}

fn bins_around(offset_hz: f64, half_hz: f64) -> Range<usize> {
    let first = bin_of(offset_hz - half_hz).ceil().max(0.0) as usize;
    let last = bin_of(offset_hz + half_hz)
        .floor()
        .min((FFT_LEN - 1) as f64) as usize;
    first..(last + 1).max(first)
}

fn report_frames(report_s: u16) -> u32 {
    (f64::from(report_s) * RATE / FFT_LEN as f64).round() as u32
}

fn db(power: f64) -> f32 {
    (10.0 * power.max(f64::MIN_POSITIVE).log10()) as f32
}

struct Lane {
    station: Station,
    bins: Range<usize>,
    tracker: Tracker,
}

fn lanes(frequency_hz: f64, artifact_hz: Option<f64>) -> Vec<Lane> {
    stations::within(frequency_hz - USABLE_HALF_HZ, frequency_hz + USABLE_HALF_HZ)
        .filter(|station| {
            artifact_hz.is_none_or(|at| {
                (station.frequency_hz - frequency_hz - at).abs()
                    > ARTIFACT_GUARD_HZ + STATION_HALF_HZ
            })
        })
        .map(|station| Lane {
            bins: bins_around(station.frequency_hz - frequency_hz, STATION_HALF_HZ),
            station,
            tracker: Tracker::default(),
        })
        .collect()
}

struct Window {
    spectrum: Vec<f64>,
    scratch: Vec<f64>,
    frames: u32,
}

impl Window {
    fn new() -> Self {
        let usable = bins_around(0.0, USABLE_HALF_HZ).len();
        Self {
            spectrum: vec![0.0; FFT_LEN],
            scratch: Vec::with_capacity(usable),
            frames: 0,
        }
    }

    fn reset(&mut self) {
        self.spectrum.fill(0.0);
        self.frames = 0;
    }

    fn add(&mut self, power: &[f32]) {
        for (sum, &bin) in self.spectrum.iter_mut().zip(power) {
            *sum += f64::from(bin);
        }
        self.frames += 1;
    }

    fn band(&self, bins: Range<usize>) -> f64 {
        self.spectrum[bins].iter().sum::<f64>() / f64::from(self.frames.max(1))
    }

    fn floor(&mut self) -> f64 {
        self.scratch.clear();
        self.scratch
            .extend_from_slice(&self.spectrum[bins_around(0.0, USABLE_HALF_HZ)]);
        let middle = self.scratch.len() / 2;
        let (_, median, _) = self.scratch.select_nth_unstable_by(middle, f64::total_cmp);
        *median / f64::from(self.frames.max(1))
    }
}

pub struct SidChannel {
    params: SidParams,
    frequency_hz: f64,
    artifact_hz: Option<f64>,
    frames: PowerFrames,
    window: Window,
    lanes: Vec<Lane>,
}

impl SidChannel {
    fn rebuild(&mut self) {
        self.frames.reset();
        self.window.reset();
        self.lanes = lanes(self.frequency_hz, self.artifact_hz);
    }
}

fn report(params: &SidParams, window: &mut Window, lanes: &mut [Lane]) -> SidReport {
    let floor = window.floor();
    let stations = lanes
        .iter_mut()
        .map(|lane| {
            let power = window.band(lane.bins.clone());
            let noise = floor * lane.bins.len() as f64;
            let level_dbfs = db(power);
            let snr_db = db(power / noise);
            let reading = lane.tracker.update(
                level_dbfs,
                snr_db,
                params.threshold_db,
                f64::from(params.report_s),
            );
            SidStationLevel {
                call: lane.station.call.to_owned(),
                frequency_hz: lane.station.frequency_hz,
                level_dbfs,
                snr_db,
                baseline_dbfs: reading.baseline_db,
                disturbed: reading.disturbed,
            }
        })
        .collect();
    SidReport {
        window_ms: u32::from(params.report_s) * 1_000,
        floor_dbfs_hz: db(floor / BIN_HZ),
        stations,
    }
}

impl ChannelRx for SidChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        Ok(Self {
            params: params(&settings)?,
            lanes: lanes(settings.frequency_hz, None),
            frequency_hz: settings.frequency_hz,
            artifact_hz: None,
            frames: PowerFrames::new(FFT_LEN),
            window: Window::new(),
        })
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = params(&settings)?;
        let moved = settings.frequency_hz != self.frequency_hz;
        let paced = params.report_s != self.params.report_s;
        self.params = params;
        self.frequency_hz = settings.frequency_hz;
        if moved {
            self.rebuild();
        } else if paced {
            self.window.reset();
            self.lanes.iter_mut().for_each(|lane| lane.tracker.reset());
        }
        Ok(())
    }

    fn retuned(&mut self) {
        self.rebuild();
    }

    fn lo_artifact_at(&mut self, offset_hz: Option<f64>) {
        if offset_hz != self.artifact_hz {
            self.artifact_hz = offset_hz;
            self.rebuild();
        }
    }

    fn needs_gated_input(&self) -> bool {
        false
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let Self {
            params,
            frames,
            window,
            lanes,
            ..
        } = self;
        let due = report_frames(params.report_s);
        frames.process(iq, |power| {
            window.add(power);
            if window.frames >= due {
                out.events
                    .push(DecoderEvent::Sid(report(params, window, lanes)));
                window.reset();
            }
        });
    }
}

#[cfg(test)]
mod tests;
