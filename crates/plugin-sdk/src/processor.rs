//! The array processor half of a plugin, written like a built-in one: implement
//! [`crate::array::ArrayProcessor`] on settings that implement [`crate::array::Settings`],
//! declare the type as the manifest's `processor` and call [`crate::export_processor!`] once.
//! What runs it is kept off the ABI so it runs, and is tested, natively too.

use num_complex::Complex;
use sdrmm_array::{
    ArrayCtx, ArrayProcessor, EVENT_BYTES, EventBytes, LaneBuffer, MAX_EVENTS_PER_BLOCK,
    OutputSlots, OutputTally, Params, PluginReading, ProcessorAction, Reading, ResetCause,
    Settings,
    abi::{BlockParts, write_steer},
    df::reserved_bearing,
};
use sdrmm_wire::{
    DecoderEvent, ProcessorType, StreamKind, SurfaceFrame, ViewSpec,
    fusion::DfBearing,
    patch::MAX_NODE_ID_LEN,
    plugin::processor::{
        ArrayFacts, ArrayHeader, ArrayOpen, BlockContext, CONTEXT_LANES, LANE_OUTPUTS, Reset,
        action, lock,
    },
};
use serde_json::Value;

use crate::from_value;

type Parse = fn(Value) -> Result<Params, String>;

/// The settings `value` holds, as `S`.
fn parse<S: Settings>(value: Value) -> Result<Params, String> {
    from_value::<S>(value).map(Params::new)
}

/// What one block left for the host to read, owned here until the next block.
#[derive(Debug, Default)]
pub struct Staged {
    pub values: Vec<f64>,
    pub events: Vec<u8>,
    pub surface: Vec<u8>,
    pub lanes: [Vec<f32>; LANE_OUTPUTS],
    pub header: ArrayHeader,
}

/// One running processor of a plugin and the slots it writes into.
pub struct Hosted {
    processor: Box<dyn ArrayProcessor>,
    parse: Parse,
    kind: ProcessorType,
    facts: ArrayFacts,
    input: Vec<Complex<f32>>,
    lanes: usize,
    context: Vec<u8>,
    parts: BlockParts,
    reading: Option<Reading>,
    surface: Option<SurfaceFrame>,
    bearings: Vec<DfBearing>,
    lane_out: Vec<LaneBuffer>,
    events: EventBytes,
    truncated: u32,
    staged: Staged,
}

fn ctx<'a>(kind: &'a ProcessorType, facts: &'a ArrayFacts) -> ArrayCtx<'a> {
    ArrayCtx {
        node: &kind.type_id,
        lanes: facts.lanes as usize,
        sample_rate: facts.sample_rate,
        center_hz: facts.center_hz,
        lane_centers_hz: &facts.lane_centers_hz,
        geometry: &facts.geometry,
        positions_m: &facts.positions_m,
        manifold: None,
        tier: facts.tier,
        tuning: facts.tuning,
        max_block: facts.max_block as usize,
    }
}

fn surface_slot(kind: &ProcessorType) -> Option<SurfaceFrame> {
    let stream = kind.facets.views.iter().find_map(|view| match view {
        ViewSpec::Surface { stream } => Some(*stream),
        _ => None,
    })?;
    Some(match stream {
        StreamKind::RangeDoppler => SurfaceFrame::RangeDoppler(Default::default()),
        StreamKind::SpatialSpectrum => SurfaceFrame::SpatialSpectrum(Default::default()),
        StreamKind::Visibility => SurfaceFrame::Visibility(Default::default()),
        _ => return None,
    })
}

const fn reset_of(cause: Reset) -> ResetCause {
    match cause {
        Reset::Realigned => ResetCause::Realigned,
        Reset::Retuned => ResetCause::Retuned,
        Reset::Calibrated => ResetCause::Calibrated,
        Reset::Gap => ResetCause::Gap,
        Reset::Resumed => ResetCause::Resumed,
    }
}

const fn lock_code(lock: Option<bool>) -> u32 {
    match lock {
        Some(true) => lock::HELD,
        Some(false) => lock::LOST,
        None => lock::UNKNOWN,
    }
}

impl Hosted {
    /// Opens a `P` of type `kind` on settings `S`, as `request` asks.
    pub fn open<P, S>(kind: &ProcessorType, request: ArrayOpen) -> Result<Self, String>
    where
        P: ArrayProcessor + 'static,
        S: Settings,
    {
        let params = parse::<S>(request.settings)?;
        let facts = request.facts;
        let processor = P::new(&ctx(kind, &facts), &params).map_err(|error| error.to_string())?;
        let capacity = facts.max_block as usize;
        Ok(Self {
            processor: Box::new(processor),
            parse: parse::<S>,
            lane_out: kind
                .lane_ports()
                .take(LANE_OUTPUTS)
                .map(|_| LaneBuffer::new(capacity))
                .collect(),
            reading: Some(Reading::new(&kind.type_id, PluginReading::reserved(kind))),
            surface: surface_slot(kind),
            kind: kind.clone(),
            facts,
            input: Vec::new(),
            lanes: 0,
            context: vec![0; BlockContext::BYTES],
            parts: BlockParts::default(),
            bearings: (0..MAX_EVENTS_PER_BLOCK)
                .map(|_| reserved_bearing(MAX_NODE_ID_LEN))
                .collect(),
            events: EventBytes::with_capacity(EVENT_BYTES),
            truncated: 0,
            staged: Staged::default(),
        })
    }

    pub fn apply(&mut self, settings: Value) -> Result<(), String> {
        let params = (self.parse)(settings)?;
        self.processor
            .apply(&params)
            .map_err(|error| error.to_string())
    }

    pub fn retune(&mut self, facts: ArrayFacts) -> Result<(), String> {
        self.facts = facts;
        self.processor
            .retune(&ctx(&self.kind, &self.facts))
            .map_err(|error| error.to_string())
    }

    pub fn reset(&mut self, cause: Reset) {
        self.processor.reset(reset_of(cause));
    }

    pub fn action(&mut self, code: u32) -> Result<(), String> {
        let action = match code {
            action::CLEAR_TRACKS => ProcessorAction::ClearTracks,
            other => return Err(format!("unknown action {other}")),
        };
        self.processor
            .action(action)
            .map_err(|error| error.to_string())
    }

    /// Room for `lanes × samples` samples, one lane after the other.
    pub fn input(&mut self, lanes: usize, samples: usize) -> &mut [Complex<f32>] {
        self.lanes = lanes.min(CONTEXT_LANES);
        self.input
            .resize(self.lanes.saturating_mul(samples), Complex::new(0.0, 0.0));
        &mut self.input
    }

    /// Room for the next block's [`BlockContext`].
    pub fn context(&mut self) -> &mut [u8] {
        &mut self.context
    }

    /// Runs one block of `samples` per lane and stages what it left.
    pub fn process(&mut self, samples: usize) -> &mut Staged {
        self.parts.read(&BlockContext::from_bytes(&self.context));
        if let Some(steer) = self.parts.steer {
            self.processor.steer(&steer);
        }
        let mut views: [&[Complex<f32>]; CONTEXT_LANES] = [&[]; CONTEXT_LANES];
        for (view, lane) in views
            .iter_mut()
            .zip(self.input.chunks_exact(samples.max(1)))
            .take(self.lanes)
        {
            *view = lane;
        }
        for lane in &mut self.lane_out {
            lane.clear();
        }
        let block = self
            .parts
            .block(&views[..self.lanes], self.facts.sample_rate);
        let tally = {
            let mut out = sdrmm_array::ProcessorOutput::new(OutputSlots {
                report: self.reading.as_mut(),
                surface: self.surface.as_mut(),
                bearings: &mut self.bearings,
                lanes: &mut self.lane_out,
            })
            .with_events(Some(&mut self.events));
            self.processor.process(&block, &mut out);
            out.tally()
        };
        self.stage(&tally);
        &mut self.staged
    }

    fn stage(&mut self, tally: &OutputTally) {
        self.truncated = self.truncated.saturating_add(
            u32::try_from(
                tally.dropped_reports
                    + tally.dropped_surfaces
                    + tally.dropped_bearings
                    + tally.dropped_events,
            )
            .unwrap_or(u32::MAX),
        );
        let mut header = ArrayHeader {
            truncated: self.truncated.saturating_add(
                u32::try_from(self.processor.faults().truncated).unwrap_or(u32::MAX),
            ),
            ..ArrayHeader::default()
        };
        self.stage_report(tally, &mut header);
        self.stage_events(tally);
        write_steer(&mut header, tally.steer.as_ref());
        self.stage_surface(tally);
        self.stage_lanes();
        self.staged.header = header;
    }

    fn stage_report(&mut self, tally: &OutputTally, header: &mut ArrayHeader) {
        self.staged.values.clear();
        let Some(reading) = self
            .reading
            .as_ref()
            .and_then(Reading::get::<PluginReading>)
        else {
            return;
        };
        header.report = u32::from(tally.report);
        header.lock = lock_code(reading.health.lock);
        header.snr_db = reading.health.snr_db.unwrap_or(f32::NAN);
        header.freq_error_hz = reading
            .health
            .freq_error_hz
            .map_or(f32::NAN, |hz| hz as f32);
        header.errors_per_s = reading.health.errors_per_s.unwrap_or(f32::NAN);
        self.staged.values.extend(
            reading
                .readouts
                .iter()
                .map(|readout| readout.value.unwrap_or(f64::NAN)),
        );
    }

    fn stage_events(&mut self, tally: &OutputTally) {
        self.staged.events.clear();
        let mut events: Vec<DecoderEvent> = if tally.events {
            serde_json::from_slice(self.events.as_slice()).unwrap_or_else(|_| {
                self.truncated = self.truncated.saturating_add(1);
                Vec::new()
            })
        } else {
            Vec::new()
        };
        events.extend(
            self.bearings
                .iter()
                .take(tally.bearings)
                .map(DecoderEvent::of),
        );
        if !events.is_empty() && serde_json::to_writer(&mut self.staged.events, &events).is_err() {
            self.staged.events.clear();
            self.truncated = self.truncated.saturating_add(1);
        }
    }

    fn stage_surface(&mut self, tally: &OutputTally) {
        self.staged.surface.clear();
        if !tally.surface {
            return;
        }
        match self.surface.as_ref().map(|surface| surface.encode(0)) {
            Some(Ok(bytes)) => self.staged.surface = bytes,
            _ => self.truncated = self.truncated.saturating_add(1),
        }
    }

    fn stage_lanes(&mut self) {
        for (staged, lane) in self.staged.lanes.iter_mut().zip(&self.lane_out) {
            staged.clear();
            staged.extend(
                lane.samples()
                    .iter()
                    .flat_map(|sample| [sample.re, sample.im]),
            );
        }
    }
}

/// The JSON a fresh node of a processor with `S` settings starts on.
#[must_use]
pub fn defaults<S: Settings>() -> Value {
    serde_json::to_string(&S::default())
        .and_then(|text| serde_json::from_str(&text))
        .unwrap_or(Value::Null)
}

/// Exports the array processor ABI for processor `$processor` on settings `$settings`, of the
/// type plugin `$plugin` declares.
#[macro_export]
macro_rules! export_processor {
    ($plugin:ty, $processor:ty, $settings:ty) => {
        #[cfg(target_arch = "wasm32")]
        const _: () = {
            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_array_open(ptr: u32, len: u32) -> i32 {
                $crate::abi::processor::open::<$processor, $settings>(
                    $crate::abi::parts::<$plugin>(),
                    ptr,
                    len,
                )
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_array_apply(handle: i32, ptr: u32, len: u32) -> i32 {
                $crate::abi::processor::apply(handle, ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_array_retune(handle: i32, ptr: u32, len: u32) -> i32 {
                $crate::abi::processor::retune(handle, ptr, len)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_array_reset(handle: i32, cause: u32) {
                $crate::abi::processor::reset(handle, cause);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_array_action(handle: i32, action: u32) -> i32 {
                $crate::abi::processor::action(handle, action)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_array_input(handle: i32, lanes: u32, samples: u32) -> u32 {
                $crate::abi::processor::input(handle, lanes, samples)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_array_context(handle: i32) -> u32 {
                $crate::abi::processor::context(handle)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_array_process(handle: i32, lanes: u32, samples: u32) -> u32 {
                $crate::abi::processor::process(handle, lanes, samples)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn sdrmm_array_close(handle: i32) {
                $crate::abi::close(handle);
            }
        };
    };
}
