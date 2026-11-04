use std::sync::Arc;

use num_complex::Complex;
use sdrmm_channels::{ChannelError, ChannelOutputs, ChannelRx};
use sdrmm_wire::{
    ChannelSettings, ChannelStatus, DecoderEvent,
    plugin::{OutputHeader, export},
    present::{Field, FieldValue},
};
use wasmtime::TypedFunc;

use crate::{
    engine::CALL_BUDGET,
    module::{Guest, PluginModule, typed},
    samples::{SAMPLE_BYTES, floats, write_samples},
};

/// One channel of a plugin decoder, on an instance of its own.
pub struct WasmChannel {
    module: Arc<PluginModule>,
    guest: Guest,
    handle: i32,
    apply: TypedFunc<(i32, u32, u32), i32>,
    input: TypedFunc<(i32, u32), u32>,
    process: TypedFunc<(i32, u32), u32>,
    retuned: Option<TypedFunc<i32, ()>>,
    close: TypedFunc<i32, ()>,
    fault: Option<String>,
    rejected: u32,
    skipped: u32,
    header: [u8; OutputHeader::BYTES],
}

fn opened(error: String) -> ChannelError {
    ChannelError::InvalidSettings(error)
}

impl WasmChannel {
    pub fn open(
        module: Arc<PluginModule>,
        input_rate: f64,
        settings: &serde_json::Value,
    ) -> Result<Self, ChannelError> {
        let mut guest = module.instantiate().map_err(opened)?;
        let open: TypedFunc<(f64, u32, u32), i32> =
            typed(&guest.instance, &mut guest.store, export::OPEN).map_err(opened)?;
        let bytes = serde_json::to_vec(settings).map_err(|error| opened(error.to_string()))?;
        let (ptr, len) = guest.write(&bytes).map_err(opened)?;
        guest.store.set_epoch_deadline(CALL_BUDGET);
        let handle = open
            .call(&mut guest.store, (input_rate, ptr, len))
            .map_err(|error| opened(error.to_string()))?;
        if handle < 0 {
            return Err(opened(guest.last_error()));
        }
        let retuned = typed(&guest.instance, &mut guest.store, export::RETUNED).ok();
        Ok(Self {
            apply: typed(&guest.instance, &mut guest.store, export::APPLY).map_err(opened)?,
            input: typed(&guest.instance, &mut guest.store, export::INPUT).map_err(opened)?,
            process: typed(&guest.instance, &mut guest.store, export::PROCESS).map_err(opened)?,
            close: typed(&guest.instance, &mut guest.store, export::CLOSE).map_err(opened)?,
            retuned,
            module,
            guest,
            handle,
            fault: None,
            rejected: 0,
            skipped: 0,
            header: [0; OutputHeader::BYTES],
        })
    }

    /// Stops the channel for good and says why on its node face.
    fn stop(&mut self, error: String, out: &mut ChannelOutputs) {
        tracing::error!(plugin = %self.module.name, %error, "plugin channel stopped");
        out.status = Some(ChannelStatus {
            readouts: vec![Field {
                label: "Plugin".to_owned(),
                value: FieldValue::Text(format!("stopped: {error}")),
            }],
            choices: Vec::new(),
        });
        self.fault = Some(error);
    }

    fn run(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) -> Result<(), String> {
        let samples = u32::try_from(iq.len()).map_err(|_| "block too large".to_owned())?;
        self.guest.store.set_epoch_deadline(CALL_BUDGET);
        let ptr = self
            .input
            .call(&mut self.guest.store, (self.handle, samples))
            .map_err(|error| error.to_string())?;
        self.write_iq(ptr, iq)?;
        let header_ptr = self
            .process
            .call(&mut self.guest.store, (self.handle, samples))
            .map_err(|error| error.to_string())?;
        self.guest
            .memory
            .read(&self.guest.store, header_ptr as usize, &mut self.header)
            .map_err(|error| error.to_string())?;
        let header = OutputHeader::from_bytes(self.header);
        self.collect(header, out)
    }

    fn write_iq(&mut self, ptr: u32, iq: &[Complex<f32>]) -> Result<(), String> {
        let data = self.guest.memory.data_mut(&mut self.guest.store);
        let start = ptr as usize;
        let end = start + iq.len() * SAMPLE_BYTES;
        let Some(target) = data.get_mut(start..end) else {
            return Err("input buffer outside plugin memory".to_owned());
        };
        write_samples(iq, target);
        Ok(())
    }

    fn collect(&mut self, header: OutputHeader, out: &mut ChannelOutputs) -> Result<(), String> {
        let data = self.guest.memory.data(&self.guest.store);
        let slice = |ptr: u32, len: usize| {
            data.get(ptr as usize..ptr as usize + len)
                .ok_or_else(|| "output outside plugin memory".to_owned())
        };
        let audio = slice(header.audio_ptr, header.audio_len as usize * 4)?;
        out.audio_pcm.extend(floats(audio));
        if header.audio_len > 0 {
            out.audio_rate = header.audio_rate;
        }
        if header.events_len > 0 {
            let events: Vec<DecoderEvent> =
                serde_json::from_slice(slice(header.events_ptr, header.events_len as usize)?)
                    .map_err(|error| format!("events: {error}"))?;
            out.events.extend(events);
        }
        if header.status_len > 0 {
            out.status = Some(
                serde_json::from_slice(slice(header.status_ptr, header.status_len as usize)?)
                    .map_err(|error| format!("status: {error}"))?,
            );
        }
        self.rejected = header.rejected;
        self.skipped = self.skipped.max(header.skipped);
        Ok(())
    }
}

impl ChannelRx for WasmChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let bytes = serde_json::to_vec(&settings.params.settings)
            .map_err(|error| opened(error.to_string()))?;
        let (ptr, len) = self.guest.write(&bytes).map_err(opened)?;
        self.guest.store.set_epoch_deadline(CALL_BUDGET);
        let code = self
            .apply
            .call(&mut self.guest.store, (self.handle, ptr, len))
            .map_err(|error| opened(error.to_string()))?;
        if code < 0 {
            return Err(opened(self.guest.last_error()));
        }
        Ok(())
    }

    fn retuned(&mut self) {
        if let Some(retuned) = &self.retuned {
            self.guest.store.set_epoch_deadline(CALL_BUDGET);
            if let Err(error) = retuned.call(&mut self.guest.store, self.handle) {
                self.fault = Some(error.to_string());
            }
        }
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        if self.fault.is_some() {
            self.skipped = self.skipped.saturating_add(1);
            return;
        }
        if let Err(error) = self.run(iq, out) {
            self.stop(error, out);
        }
    }

    fn rejected(&self) -> u32 {
        self.rejected
    }

    fn skipped(&self) -> u32 {
        self.skipped
    }
}

impl Drop for WasmChannel {
    fn drop(&mut self) {
        self.guest.store.set_epoch_deadline(CALL_BUDGET);
        if let Err(error) = self.close.call(&mut self.guest.store, self.handle) {
            tracing::warn!(plugin = %self.module.name, %error, "plugin channel did not close");
        }
    }
}
