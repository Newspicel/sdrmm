mod form;
mod model;
mod present;

use core::ops::Range;

use sdrmm_dsp::array::correlator::{BandVisibility, CorrelatorError, FxCorrelator};
use sdrmm_wire::{
    Face, ProcessorFacets, ProcessorType, StreamKind, SurfaceFrame, ViewSpec, VisibilityOwned,
};

use crate::{
    ArrayBlock, ArrayCtx, ArrayProcessor, Execution, MAX_LANES, Native, Params, ProcessorAction,
    ProcessorFaults, ProcessorNeeds, ProcessorOutput, Reading, ResetCause, Settings, TuningNeed,
    boxed, check_tuning, kind_of, no_gpu, no_lane_format, parse, stamp_at,
};
use sdrmm_decoder::ChannelError;

pub use model::*;

pub const VISIBILITY_DB_MIN: f32 = -40.0;
pub const VISIBILITY_DB_MAX: f32 = 0.0;

const SNR_FLOOR_DB: f32 = -99.0;
const NANOS_PER_MILLI: u64 = 1_000_000;

pub fn descriptor() -> Native {
    Native {
        kind: ProcessorType {
            settings: CorrelatorParams::form(),
            facets: ProcessorFacets::new(
                &[ViewSpec::Surface {
                    stream: StreamKind::Visibility,
                }],
                &[],
            ),
            face: Face::sized(520, 460).min(380, 340),
            ..kind_of::<CorrelatorParams>("Correlator", "Baseline visibilities")
        },
        parse: parse::<CorrelatorParams>,
        reading: || {
            Some(Reading::new(
                CorrelatorParams::TYPE_ID,
                CorrelatorReading::reserved(),
            ))
        },
        needs: |_| ProcessorNeeds::CALIBRATED,
        band: |_| None,
        tuning: |_| TuningNeed::Together,
        lane_format: no_lane_format,
        execution: |_, ctx| Execution::Worker {
            batch: ctx.max_block,
        },
        in_place,
        gpu: no_gpu,
        event_bytes: 0,
        enricher: None,
        mission: None,
        create: Some(boxed::<CorrelatorProcessor>),
        dedicated: None,
    }
}

fn settings(params: &Params) -> Option<&CorrelatorParams> {
    params.get()
}

const fn same_shape(old: &CorrelatorParams, new: &CorrelatorParams) -> bool {
    old.bins == new.bins && old.channels == new.channels && old.overlap == new.overlap
}

fn in_place(old: &Params, new: &Params) -> bool {
    let (Some(old), Some(new)) = (settings(old), settings(new)) else {
        return false;
    };
    same_shape(old, new)
}

const fn refused(error: CorrelatorError) -> ChannelError {
    match error {
        CorrelatorError::FftSize(_) => ChannelError::Refused("Bins out of range"),
        CorrelatorError::Lanes(_) => ChannelError::Refused("Too few elements"),
        CorrelatorError::LaneLength => ChannelError::Refused("Lane out of range"),
        CorrelatorError::Baseline(_) | CorrelatorError::Bins(..) => {
            ChannelError::Refused("Band out of range")
        }
    }
}

fn fringe_bins(params: &CorrelatorParams, rate: f64) -> Result<Range<usize>, ChannelError> {
    let bins = params.bins as usize;
    if params.offset_hz.abs() > rate / 2.0 {
        return Err(ChannelError::Refused("Offset out of range"));
    }
    let Some(bandwidth) = params.bandwidth_hz else {
        return Ok(0..bins);
    };
    let to_bin = |hz: f64| hz / rate * bins as f64 + (bins / 2) as f64;
    let low = to_bin(params.offset_hz - bandwidth / 2.0).ceil().max(0.0) as usize;
    let high = (to_bin(params.offset_hz + bandwidth / 2.0).floor().max(0.0) as usize + 1).min(bins);
    if low < high {
        return Ok(low..high);
    }
    let centre = (to_bin(params.offset_hz).round().max(0.0) as usize).min(bins - 1);
    Ok(centre..centre + 1)
}

fn integration_frames(params: &CorrelatorParams, rate: f64, hop: usize) -> u64 {
    ((f64::from(params.integrate_s) * rate / hop as f64).ceil() as u64).max(1)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct BaselineShape {
    length_m: f32,
    azimuth_deg: f32,
}

fn baseline_shapes(
    ctx: &ArrayCtx<'_>,
    correlator: &FxCorrelator,
) -> [BaselineShape; MAX_BASELINES] {
    let mut shapes = [BaselineShape::default(); MAX_BASELINES];
    if ctx.positions_m.len() != ctx.lanes {
        return shapes;
    }
    for (baseline, shape) in shapes.iter_mut().enumerate() {
        let Some((a, b)) = correlator.pair(baseline) else {
            break;
        };
        let (from, to) = (ctx.positions_m[a], ctx.positions_m[b]);
        let (dx, dy, dz) = (to[0] - from[0], to[1] - from[1], to[2] - from[2]);
        *shape = BaselineShape {
            length_m: (dx * dx + dy * dy + dz * dz).sqrt() as f32,
            azimuth_deg: dx.atan2(dy).to_degrees().rem_euclid(360.0) as f32,
        };
    }
    shapes
}

fn level_byte(db: f32) -> u8 {
    let span = VISIBILITY_DB_MAX - VISIBILITY_DB_MIN;
    (255.0 * (db - VISIBILITY_DB_MIN) / span)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn phase_byte(phase_rad: f64) -> u8 {
    ((phase_rad.to_degrees() + 180.0) / 360.0 * 255.0)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn coherence_db(band: &BandVisibility) -> f32 {
    if band.coherence > 0.0 {
        (20.0 * band.coherence.log10()) as f32
    } else {
        VISIBILITY_DB_MIN
    }
}

fn reserved_visibility(cells: usize) -> VisibilityOwned {
    VisibilityOwned {
        amplitude: Vec::with_capacity(cells),
        phase: Vec::with_capacity(cells),
        ..VisibilityOwned::default()
    }
}

pub struct CorrelatorProcessor {
    params: CorrelatorParams,
    lanes: usize,
    rate: f64,
    center_hz: f64,
    correlator: FxCorrelator,
    fringe: Range<usize>,
    frames_target: u64,
    shapes: [BaselineShape; MAX_BASELINES],
    seq: u32,
    faults: ProcessorFaults,
}

impl CorrelatorProcessor {
    fn check_layout(&self, ctx: &ArrayCtx<'_>) -> Result<(), ChannelError> {
        if ctx.lanes != self.lanes {
            return Err(ChannelError::Refused("Lane out of range"));
        }
        if ctx.sample_rate != self.rate {
            return Err(ChannelError::Refused("Rate out of range"));
        }
        check_tuning(TuningNeed::Together, ctx)
    }

    fn publish(&mut self, unix_ns: u64, out: &mut ProcessorOutput<'_>) {
        if let Some(reading) = out.report_as::<CorrelatorReading>() {
            self.fill_report(reading, unix_ns);
            out.publish_report();
        }
        if let Some(slot) = out.surface() {
            if !matches!(slot, SurfaceFrame::Visibility(_)) {
                let cells = self.correlator.baselines() * self.params.channels as usize;
                *slot = SurfaceFrame::Visibility(reserved_visibility(cells));
            }
            if let SurfaceFrame::Visibility(frame) = slot
                && self.fill_surface(frame, unix_ns)
            {
                out.publish_surface();
            }
        }
        self.seq = self.seq.wrapping_add(1);
    }

    fn fringe_band(&mut self, baseline: usize) -> (BandVisibility, f64) {
        let band = self.correlator.band(baseline, self.fringe.clone());
        let delay = self.correlator.delay_samples(baseline, self.fringe.clone());
        if let (Ok(band), Ok(delay)) = (band, delay) {
            (band, delay)
        } else {
            self.faults.solver_failures += 1;
            (BandVisibility::default(), 0.0)
        }
    }

    fn fill_report(&mut self, reading: &mut CorrelatorReading, unix_ns: u64) {
        stamp_at(&mut reading.at, unix_ns);
        reading.frames = self.correlator.frames();
        reading.integrated_s =
            (reading.frames as f64 * self.correlator.hop() as f64 / self.rate) as f32;
        reading.baselines.clear();
        for baseline in 0..self.correlator.baselines() {
            let Some((a, b)) = self.correlator.pair(baseline) else {
                break;
            };
            let (band, delay) = self.fringe_band(baseline);
            let shape = self.shapes[baseline];
            let entry = Baseline {
                a: a as u32,
                b: b as u32,
                delay_ns: (delay / self.rate * 1e9) as f32,
                coherence: band.coherence as f32,
                phase_deg: band.phase_rad.to_degrees() as f32,
                length_m: shape.length_m,
                azimuth_deg: shape.azimuth_deg,
                snr_db: band.snr_db.max(SNR_FLOOR_DB),
            };
            self.faults.push_capped(&mut reading.baselines, entry);
        }
    }

    fn fill_surface(&mut self, frame: &mut VisibilityOwned, unix_ns: u64) -> bool {
        let truncated = self.faults.truncated;
        let channels = self.params.channels as usize;
        let width = self.correlator.fft_size() / channels;
        frame.seq = self.seq;
        frame.timestamp = unix_ns / NANOS_PER_MILLI;
        frame.center_hz = self.center_hz;
        frame.span_hz = self.rate as f32;
        frame.baselines = self.correlator.baselines() as u16;
        frame.bins = channels as u16;
        frame.db_min = VISIBILITY_DB_MIN;
        frame.db_max = VISIBILITY_DB_MAX;
        frame.amplitude.clear();
        frame.phase.clear();
        for baseline in 0..self.correlator.baselines() {
            for channel in 0..channels {
                let bins = channel * width..(channel + 1) * width;
                let band = self.correlator.band(baseline, bins).unwrap_or_else(|_| {
                    self.faults.solver_failures += 1;
                    BandVisibility::default()
                });
                self.faults
                    .push_capped(&mut frame.amplitude, level_byte(coherence_db(&band)));
                self.faults
                    .push_capped(&mut frame.phase, phase_byte(band.phase_rad));
            }
        }
        self.faults.truncated == truncated
    }
}

impl ArrayProcessor for CorrelatorProcessor {
    fn new(ctx: &ArrayCtx<'_>, params: &Params) -> Result<Self, ChannelError> {
        let settings = *settings(params).ok_or(ChannelError::Refused("Wrong settings"))?;
        if let Some(problem) = settings.problem() {
            return Err(ChannelError::Refused(problem));
        }
        if ctx.lanes > MAX_LANES {
            return Err(ChannelError::Refused("Too many elements"));
        }
        check_tuning(TuningNeed::Together, ctx)?;
        let correlator = FxCorrelator::new(ctx.lanes, settings.bins as usize, settings.overlap)
            .map_err(refused)?;
        let cells = correlator.baselines() * settings.channels as usize;
        if cells > MAX_VISIBILITY_CELLS as usize {
            return Err(ChannelError::Refused("Channels out of range"));
        }
        Ok(Self {
            params: settings,
            lanes: ctx.lanes,
            rate: ctx.sample_rate,
            center_hz: ctx.center_hz,
            fringe: fringe_bins(&settings, ctx.sample_rate)?,
            frames_target: integration_frames(&settings, ctx.sample_rate, correlator.hop()),
            shapes: baseline_shapes(ctx, &correlator),
            correlator,
            seq: 0,
            faults: ProcessorFaults::default(),
        })
    }

    fn apply(&mut self, params: &Params) -> Result<(), ChannelError> {
        let settings = *settings(params).ok_or(ChannelError::Refused("Wrong settings"))?;
        if let Some(problem) = settings.problem() {
            return Err(ChannelError::Refused(problem));
        }
        if !same_shape(&self.params, &settings) {
            return Err(ChannelError::Refused("Needs a rebuild"));
        }
        self.fringe = fringe_bins(&settings, self.rate)?;
        self.frames_target = integration_frames(&settings, self.rate, self.correlator.hop());
        self.params = settings;
        Ok(())
    }

    fn retune(&mut self, ctx: &ArrayCtx<'_>) -> Result<(), ChannelError> {
        self.check_layout(ctx)?;
        self.center_hz = ctx.center_hz;
        self.correlator.reset();
        Ok(())
    }

    fn reset(&mut self, _cause: ResetCause) {
        self.correlator.reset();
        self.faults.resets += 1;
    }

    fn action(&mut self, _action: ProcessorAction) -> Result<(), ChannelError> {
        Err(ChannelError::Refused("No tracks here"))
    }

    fn process(&mut self, block: &ArrayBlock<'_>, out: &mut ProcessorOutput<'_>) {
        if !self.faults.lanes_match(block, self.lanes) {
            return;
        }
        if self.correlator.push(block.lanes).is_err() {
            self.faults.lane_mismatch += 1;
            return;
        }
        if self.correlator.frames() >= self.frames_target {
            self.publish(block.unix_ns, out);
            self.correlator.clear_integration();
        }
    }

    fn faults(&self) -> ProcessorFaults {
        self.faults
    }
}

#[cfg(test)]
mod tests;
