//! A plugin's modulator, opened like a built-in `ChannelTx`.

use std::sync::Arc;

use num_complex::Complex;
use sdrmm_channels::{ChannelError, ChannelTx, TxPayload};
use sdrmm_wire::{
    ChannelSettings,
    plugin::{
        transmit::{PayloadKind, export},
        unpack,
    },
};
use wasmtime::TypedFunc;

use crate::{
    engine::CALL_BUDGET,
    module::{Guest, PluginModule, typed},
    samples::SAMPLE_BYTES,
};

/// One transmitter of a plugin, on an instance of its own.
pub struct WasmTx {
    module: Arc<PluginModule>,
    guest: Guest,
    handle: i32,
    apply: TypedFunc<(i32, u32, u32), i32>,
    submit: TypedFunc<(i32, u32, u32, u32), i32>,
    generate: TypedFunc<(i32, u32), i64>,
    close: TypedFunc<i32, ()>,
    fault: Option<String>,
}

fn invalid(error: String) -> ChannelError {
    ChannelError::InvalidSettings(error)
}

fn halted(error: &str) -> ChannelError {
    ChannelError::Unsupported(format!("plugin transmitter stopped: {error}"))
}

/// Whether the plugin exports every transmit entry its descriptor promises.
pub(crate) fn check_exports(module: &wasmtime::Module) -> Result<(), String> {
    let missing: Vec<&str> = export::ALL
        .into_iter()
        .filter(|name| module.get_export(name).is_none())
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("transmits but lacks {}", missing.join(", ")))
    }
}

impl WasmTx {
    pub fn open(
        module: Arc<PluginModule>,
        rate: f64,
        settings: &serde_json::Value,
    ) -> Result<Self, ChannelError> {
        let mut guest = module.instantiate().map_err(invalid)?;
        let open: TypedFunc<(f64, u32, u32), i32> =
            typed(&guest.instance, &mut guest.store, export::OPEN).map_err(invalid)?;
        let handle = guest.call_json(&open, rate, settings).map_err(invalid)?;
        if handle < 0 {
            return Err(invalid(guest.last_error()));
        }
        Ok(Self {
            apply: typed(&guest.instance, &mut guest.store, export::APPLY).map_err(invalid)?,
            submit: typed(&guest.instance, &mut guest.store, export::SUBMIT).map_err(invalid)?,
            generate: typed(&guest.instance, &mut guest.store, export::GENERATE)
                .map_err(invalid)?,
            close: typed(&guest.instance, &mut guest.store, export::CLOSE).map_err(invalid)?,
            module,
            guest,
            handle,
            fault: None,
        })
    }

    fn stopped(&self) -> Result<(), ChannelError> {
        self.fault
            .as_deref()
            .map_or(Ok(()), |error| Err(halted(error)))
    }

    /// Stops the transmitter for good; every later call says why.
    fn stop(&mut self, error: String) -> ChannelError {
        tracing::error!(plugin = %self.module.name, %error, "plugin transmitter stopped");
        let halted = halted(&error);
        self.fault = Some(error);
        halted
    }

    fn pull(&mut self, out: &mut [Complex<f32>]) -> Result<usize, String> {
        let max = u32::try_from(out.len()).map_err(|_| "block too large".to_owned())?;
        self.guest.store.set_epoch_deadline(CALL_BUDGET);
        let packed = self
            .generate
            .call(&mut self.guest.store, (self.handle, max))
            .map_err(|error| error.to_string())?;
        let (ptr, written) = unpack(packed);
        let written = (written as usize).min(out.len());
        let data = self.guest.memory.data(&self.guest.store);
        let start = ptr as usize;
        let bytes = data
            .get(start..start + written * SAMPLE_BYTES)
            .ok_or_else(|| "output outside plugin memory".to_owned())?;
        for (slot, chunk) in out.iter_mut().zip(bytes.as_chunks::<SAMPLE_BYTES>().0) {
            let [a, b, c, d, e, f, g, h] = *chunk;
            *slot = Complex::new(
                f32::from_le_bytes([a, b, c, d]),
                f32::from_le_bytes([e, f, g, h]),
            );
        }
        Ok(written)
    }
}

fn encode(payload: &TxPayload) -> (PayloadKind, Vec<u8>) {
    match payload {
        TxPayload::Audio(pcm) => (
            PayloadKind::Audio,
            pcm.iter().flat_map(|sample| sample.to_le_bytes()).collect(),
        ),
        TxPayload::Frame(bytes) => (PayloadKind::Frame, bytes.clone()),
    }
}

impl ChannelTx for WasmTx {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        self.stopped()?;
        let code = self
            .guest
            .call_json(&self.apply, self.handle, &settings.params.settings)
            .map_err(invalid)?;
        if code < 0 {
            return Err(invalid(self.guest.last_error()));
        }
        Ok(())
    }

    fn submit(&mut self, payload: TxPayload) -> Result<(), ChannelError> {
        self.stopped()?;
        let (kind, bytes) = encode(&payload);
        let (ptr, len) = self.guest.write(&bytes).map_err(|error| self.stop(error))?;
        self.guest.store.set_epoch_deadline(CALL_BUDGET);
        let code = self
            .submit
            .call(&mut self.guest.store, (self.handle, kind as u32, ptr, len))
            .map_err(|error| self.stop(error.to_string()))?;
        if code < 0 {
            return Err(ChannelError::InvalidPayload(self.guest.last_error()));
        }
        Ok(())
    }

    fn generate(&mut self, out: &mut [Complex<f32>]) -> usize {
        if self.fault.is_some() {
            return 0;
        }
        self.pull(out).unwrap_or_else(|error| {
            self.stop(error);
            0
        })
    }
}

impl Drop for WasmTx {
    fn drop(&mut self) {
        self.guest.store.set_epoch_deadline(CALL_BUDGET);
        if let Err(error) = self.close.call(&mut self.guest.store, self.handle) {
            tracing::warn!(plugin = %self.module.name, %error, "plugin transmitter did not close");
        }
    }
}
