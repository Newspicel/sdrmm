//! A plugin's array processor, run by the engine like a built-in one on a worker of its own.

use std::sync::Arc;

use sdrmm_channels::{
    ChannelError,
    array_processor::{
        ArrayBlock, ArrayCtx, ArrayProcessor, ProcessorAction, ProcessorDescriptor,
        ProcessorFactory, ProcessorFaults, ProcessorOutput, ResetCause, plugin_descriptor,
    },
};
use sdrmm_wire::{
    ChannelHealth, PluginParams, ProcessorParams, ProcessorReading, ProcessorType,
    plugin::{
        PluginManifest,
        processor::{ArrayFacts, ArrayHeader, ArrayOpen, Reset, export, lock},
    },
};
use wasmtime::TypedFunc;

use crate::{
    engine::CALL_BUDGET,
    module::{Guest, PluginModule, typed},
    samples::{SAMPLE_BYTES, write_samples},
};

const F64_BYTES: usize = 8;

/// The processor type a plugin declares, and how to start one.
pub struct WasmProcessors {
    module: Arc<PluginModule>,
    kind: ProcessorType,
}

impl WasmProcessors {
    /// The processor `module` declares, if it declares one.
    #[must_use]
    pub fn of(module: &Arc<PluginModule>) -> Option<Self> {
        module.manifest.processor.clone().map(|kind| Self {
            module: module.clone(),
            kind,
        })
    }
}

impl ProcessorFactory for WasmProcessors {
    fn kind(&self) -> &ProcessorType {
        &self.kind
    }

    fn create(
        &self,
        ctx: &ArrayCtx<'_>,
        params: &PluginParams,
    ) -> Result<Box<dyn ArrayProcessor>, ChannelError> {
        Ok(Box::new(WasmProcessor::open(
            self.module.clone(),
            &self.kind,
            ctx,
            &params.settings,
        )?))
    }
}

/// Whether the plugin exports every array entry its manifest promises, and emits only kinds it
/// declares.
pub(crate) fn check_processor(
    module: &wasmtime::Module,
    manifest: &PluginManifest,
) -> Result<(), String> {
    let Some(kind) = &manifest.processor else {
        return Ok(());
    };
    let missing: Vec<&str> = export::ALL
        .into_iter()
        .filter(|name| module.get_export(name).is_none())
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "brings a processor but lacks {}",
            missing.join(", ")
        ));
    }
    match kind
        .emits
        .iter()
        .find(|emitted| !manifest.kinds.iter().any(|entry| entry.kind == **emitted))
    {
        Some(stray) => Err(format!(
            "processor emits {stray}, a kind it does not declare"
        )),
        None => Ok(()),
    }
}

/// What the plugin is told of the array it runs on.
fn facts(ctx: &ArrayCtx<'_>) -> ArrayFacts {
    ArrayFacts {
        lanes: u32::try_from(ctx.lanes).unwrap_or(u32::MAX),
        sample_rate: ctx.sample_rate,
        center_hz: ctx.center_hz,
        lane_centers_hz: ctx.lane_centers_hz.to_vec(),
        positions_m: ctx.positions_m.to_vec(),
    }
}

const fn reset_code(cause: ResetCause) -> Reset {
    match cause {
        ResetCause::Realigned => Reset::Realigned,
        ResetCause::Retuned => Reset::Retuned,
        ResetCause::Calibrated => Reset::Calibrated,
        ResetCause::Gap => Reset::Gap,
        ResetCause::Resumed => Reset::Resumed,
    }
}

fn health(header: &ArrayHeader) -> ChannelHealth {
    let known = |value: f32| (!value.is_nan()).then_some(value);
    ChannelHealth {
        lock: match header.lock {
            lock::HELD => Some(true),
            lock::LOST => Some(false),
            _ => None,
        },
        snr_db: known(header.snr_db),
        freq_error_hz: known(header.freq_error_hz).map(f64::from),
        errors_per_s: known(header.errors_per_s),
    }
}

fn invalid(error: String) -> ChannelError {
    ChannelError::InvalidSettings(error)
}

/// One processor of a plugin, on an instance of its own.
pub struct WasmProcessor {
    module: Arc<PluginModule>,
    guest: Guest,
    handle: i32,
    lanes: usize,
    apply: TypedFunc<(i32, u32, u32), i32>,
    retune: TypedFunc<(i32, u32, u32), i32>,
    reset: TypedFunc<(i32, u32), ()>,
    input: TypedFunc<(i32, u32, u32), u32>,
    process: TypedFunc<(i32, u32, u32), u32>,
    close: TypedFunc<i32, ()>,
    fault: Option<String>,
    faults: ProcessorFaults,
    header: [u8; ArrayHeader::BYTES],
}

impl WasmProcessor {
    pub fn open(
        module: Arc<PluginModule>,
        kind: &ProcessorType,
        ctx: &ArrayCtx<'_>,
        settings: &serde_json::Value,
    ) -> Result<Self, ChannelError> {
        let mut guest = module.instantiate().map_err(invalid)?;
        let open: TypedFunc<(u32, u32), i32> =
            typed(&guest.instance, &mut guest.store, export::OPEN).map_err(invalid)?;
        let request = ArrayOpen {
            facts: facts(ctx),
            settings: settings.clone(),
        };
        let bytes = serde_json::to_vec(&request).map_err(|error| invalid(error.to_string()))?;
        let (ptr, len) = guest.write(&bytes).map_err(invalid)?;
        guest.store.set_epoch_deadline(CALL_BUDGET);
        let handle = open
            .call(&mut guest.store, (ptr, len))
            .map_err(|error| invalid(error.to_string()))?;
        if handle < 0 {
            return Err(invalid(guest.last_error()));
        }
        tracing::debug!(plugin = %module.name, processor = %kind.type_id, "processor opened");
        Ok(Self {
            apply: typed(&guest.instance, &mut guest.store, export::APPLY).map_err(invalid)?,
            retune: typed(&guest.instance, &mut guest.store, export::RETUNE).map_err(invalid)?,
            reset: typed(&guest.instance, &mut guest.store, export::RESET).map_err(invalid)?,
            input: typed(&guest.instance, &mut guest.store, export::INPUT).map_err(invalid)?,
            process: typed(&guest.instance, &mut guest.store, export::PROCESS).map_err(invalid)?,
            close: typed(&guest.instance, &mut guest.store, export::CLOSE).map_err(invalid)?,
            lanes: ctx.lanes,
            module,
            guest,
            handle,
            fault: None,
            faults: ProcessorFaults::default(),
            header: [0; ArrayHeader::BYTES],
        })
    }

    fn halted(&self) -> Result<(), ChannelError> {
        self.fault.as_deref().map_or(Ok(()), |error| {
            Err(ChannelError::Unsupported(format!(
                "plugin processor stopped: {error}"
            )))
        })
    }

    /// Stops the processor for good and says why on its node face.
    fn stop(&mut self, error: String, out: &mut ProcessorOutput<'_>) {
        tracing::error!(plugin = %self.module.name, %error, "plugin processor stopped");
        self.faults.solver_failures += 1;
        if let Some(ProcessorReading::Plugin(reading)) = out.report() {
            reading.error = Some(error.clone());
            out.publish_report();
        }
        self.fault = Some(error);
    }

    fn run(&mut self, block: &ArrayBlock<'_>, out: &mut ProcessorOutput<'_>) -> Result<(), String> {
        let samples = block.len();
        let count = u32::try_from(samples).map_err(|_| "block too large".to_owned())?;
        let lanes = u32::try_from(block.lanes.len()).map_err(|_| "too many lanes".to_owned())?;
        self.guest.store.set_epoch_deadline(CALL_BUDGET);
        let ptr = self
            .input
            .call(&mut self.guest.store, (self.handle, lanes, count))
            .map_err(|error| error.to_string())?;
        self.write_lanes(ptr, block, samples)?;
        let header_ptr = self
            .process
            .call(&mut self.guest.store, (self.handle, lanes, count))
            .map_err(|error| error.to_string())?;
        self.guest
            .memory
            .read(&self.guest.store, header_ptr as usize, &mut self.header)
            .map_err(|error| error.to_string())?;
        self.collect(ArrayHeader::from_bytes(self.header), out)
    }

    fn write_lanes(
        &mut self,
        ptr: u32,
        block: &ArrayBlock<'_>,
        samples: usize,
    ) -> Result<(), String> {
        let data = self.guest.memory.data_mut(&mut self.guest.store);
        let stride = samples * SAMPLE_BYTES;
        let start = ptr as usize;
        let target = data
            .get_mut(start..start + stride * block.lanes.len())
            .ok_or_else(|| "input buffer outside plugin memory".to_owned())?;
        for (chunk, lane) in target.chunks_exact_mut(stride.max(1)).zip(block.lanes) {
            write_samples(&lane[..samples.min(lane.len())], chunk);
        }
        Ok(())
    }

    fn collect(
        &mut self,
        header: ArrayHeader,
        out: &mut ProcessorOutput<'_>,
    ) -> Result<(), String> {
        let data = self.guest.memory.data(&self.guest.store);
        let slice = |ptr: u32, len: usize| {
            data.get(ptr as usize..ptr as usize + len)
                .ok_or_else(|| "output outside plugin memory".to_owned())
        };
        self.faults.truncated = u64::from(header.truncated);
        if header.events_len > 0 {
            out.events(slice(header.events_ptr, header.events_len as usize)?);
        }
        if header.report == 0 {
            return Ok(());
        }
        let values = slice(header.values_ptr, header.values_len as usize * F64_BYTES)?;
        if let Some(ProcessorReading::Plugin(reading)) = out.report() {
            reading.health = health(&header);
            reading.fill(
                values
                    .as_chunks::<F64_BYTES>()
                    .0
                    .iter()
                    .map(|chunk| f64::from_le_bytes(*chunk)),
            );
            out.publish_report();
        }
        Ok(())
    }

    fn send_json(
        &mut self,
        func: &TypedFunc<(i32, u32, u32), i32>,
        value: &impl serde::Serialize,
    ) -> Result<(), ChannelError> {
        self.halted()?;
        let code = self
            .guest
            .call_json(func, self.handle, value)
            .map_err(invalid)?;
        if code < 0 {
            return Err(invalid(self.guest.last_error()));
        }
        Ok(())
    }
}

impl ArrayProcessor for WasmProcessor {
    fn descriptor() -> &'static ProcessorDescriptor
    where
        Self: Sized,
    {
        plugin_descriptor()
    }

    fn new(_: &ArrayCtx<'_>, params: &ProcessorParams) -> Result<Self, ChannelError>
    where
        Self: Sized,
    {
        Err(ChannelError::UnknownType(params.type_id().to_owned()))
    }

    fn apply(&mut self, params: &ProcessorParams) -> Result<(), ChannelError> {
        let ProcessorParams::Plugin(plugin) = params else {
            return Err(ChannelError::UnknownType(params.type_id().to_owned()));
        };
        let apply = self.apply.clone();
        self.send_json(&apply, &plugin.settings)
    }

    fn retune(&mut self, ctx: &ArrayCtx<'_>) -> Result<(), ChannelError> {
        self.lanes = ctx.lanes;
        let retune = self.retune.clone();
        self.send_json(&retune, &facts(ctx))
    }

    fn reset(&mut self, cause: ResetCause) {
        if self.fault.is_some() {
            return;
        }
        self.guest.store.set_epoch_deadline(CALL_BUDGET);
        let code = reset_code(cause) as u32;
        if let Err(error) = self.reset.call(&mut self.guest.store, (self.handle, code)) {
            tracing::error!(plugin = %self.module.name, %error, "plugin processor stopped");
            self.faults.solver_failures += 1;
            self.fault = Some(error.to_string());
        }
    }

    fn action(&mut self, _: ProcessorAction) -> Result<(), ChannelError> {
        Err(ChannelError::Unsupported(
            "plugin processors take no actions".to_owned(),
        ))
    }

    fn process(&mut self, block: &ArrayBlock<'_>, out: &mut ProcessorOutput<'_>) {
        if self.fault.is_some() || !self.faults.lanes_match(block, self.lanes) {
            return;
        }
        if let Err(error) = self.run(block, out) {
            self.stop(error, out);
        }
    }

    fn faults(&self) -> ProcessorFaults {
        self.faults
    }
}

impl Drop for WasmProcessor {
    fn drop(&mut self) {
        self.guest.store.set_epoch_deadline(CALL_BUDGET);
        if let Err(error) = self.close.call(&mut self.guest.store, self.handle) {
            tracing::warn!(plugin = %self.module.name, %error, "plugin processor did not close");
        }
    }
}
