//! A plugin's encoder, listed and opened like a built-in one.

use std::sync::Arc;

use num_complex::Complex;
use sdrmm_channels::encoder::{Encoder, EncoderError, EncoderParams, Modulator, TxPayload};
use sdrmm_wire::{
    EncoderDescriptor, EncoderInput,
    plugin::{
        encoder::{PayloadKind, export},
        unpack,
    },
};
use wasmtime::TypedFunc;

use crate::{
    engine::CALL_BUDGET,
    module::{Guest, PluginModule, typed},
    samples::SAMPLE_BYTES,
};

/// A plugin's encoder as the encoder registry sees it.
pub struct WasmEncoder {
    module: Arc<PluginModule>,
    descriptor: EncoderDescriptor,
}

impl WasmEncoder {
    /// The encoder `module` declares, if it declares one.
    #[must_use]
    pub fn of(module: &Arc<PluginModule>) -> Option<Self> {
        let descriptor = module.manifest.encoder.clone()?;
        Some(Self {
            module: module.clone(),
            descriptor,
        })
    }

    fn check_type(&self, params: &EncoderParams) -> Result<(), EncoderError> {
        if params.type_id == self.descriptor.type_id {
            Ok(())
        } else {
            Err(EncoderError::WrongType {
                encoder: self.descriptor.type_id.clone(),
                got: params.type_id.clone(),
            })
        }
    }
}

impl Encoder for WasmEncoder {
    fn descriptor(&self) -> &EncoderDescriptor {
        &self.descriptor
    }

    fn open(&self, params: &EncoderParams) -> Result<Box<dyn Modulator>, EncoderError> {
        self.check_type(params)?;
        Ok(Box::new(WasmModulator::open(
            self.module.clone(),
            &self.descriptor,
            &params.settings,
        )?))
    }

    /// A message encoder gets the filled form as UTF-8 JSON in a frame.
    fn compose(&self, message: &serde_json::Value) -> Result<TxPayload, EncoderError> {
        let type_id = &self.descriptor.type_id;
        if self.descriptor.input != EncoderInput::Message {
            return Err(EncoderError::NoMessages(type_id.clone()));
        }
        serde_json::to_vec(message)
            .map(TxPayload::Frame)
            .map_err(|error| EncoderError::message(type_id, error.to_string()))
    }

    fn width_hz(&self, params: &EncoderParams) -> Result<f64, EncoderError> {
        self.check_type(params)?;
        Ok(self.descriptor.bandwidth_hz)
    }
}

/// One running encoder of a plugin, on an instance of its own.
pub struct WasmModulator {
    module: Arc<PluginModule>,
    type_id: String,
    guest: Guest,
    handle: i32,
    submit: TypedFunc<(i32, u32, u32, u32), i32>,
    generate: TypedFunc<(i32, u32), i64>,
    close: TypedFunc<i32, ()>,
    fault: Option<String>,
}

impl WasmModulator {
    pub fn open(
        module: Arc<PluginModule>,
        descriptor: &EncoderDescriptor,
        settings: &serde_json::Value,
    ) -> Result<Self, EncoderError> {
        let type_id = descriptor.type_id.clone();
        let refused = |reason: String| EncoderError::settings(&type_id, reason);
        let mut guest = module.instantiate().map_err(refused)?;
        let open: TypedFunc<(f64, u32, u32), i32> =
            typed(&guest.instance, &mut guest.store, export::OPEN).map_err(refused)?;
        let handle = guest
            .call_json(&open, descriptor.rate_hz, settings)
            .map_err(refused)?;
        if handle < 0 {
            return Err(refused(guest.last_error()));
        }
        Ok(Self {
            submit: typed(&guest.instance, &mut guest.store, export::SUBMIT).map_err(refused)?,
            generate: typed(&guest.instance, &mut guest.store, export::GENERATE)
                .map_err(refused)?,
            close: typed(&guest.instance, &mut guest.store, export::CLOSE).map_err(refused)?,
            module,
            type_id,
            guest,
            handle,
            fault: None,
        })
    }

    fn halted(&self, reason: String) -> EncoderError {
        EncoderError::Stopped {
            encoder: self.type_id.clone(),
            reason,
        }
    }

    fn stopped(&self) -> Result<(), EncoderError> {
        self.fault
            .clone()
            .map_or(Ok(()), |reason| Err(self.halted(reason)))
    }

    /// Stops the encoder for good; every later call says why.
    fn stop(&mut self, error: String) -> EncoderError {
        tracing::error!(plugin = %self.module.name, %error, "plugin encoder stopped");
        self.fault = Some(error.clone());
        self.halted(error)
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

impl Modulator for WasmModulator {
    fn submit(&mut self, payload: TxPayload) -> Result<(), EncoderError> {
        self.stopped()?;
        let (kind, bytes) = encode(&payload);
        let (ptr, len) = self.guest.write(&bytes).map_err(|error| self.stop(error))?;
        self.guest.store.set_epoch_deadline(CALL_BUDGET);
        let code = self
            .submit
            .call(&mut self.guest.store, (self.handle, kind as u32, ptr, len))
            .map_err(|error| self.stop(error.to_string()))?;
        if code < 0 {
            return Err(EncoderError::Refused {
                encoder: self.type_id.clone(),
                reason: self.guest.last_error(),
            });
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

impl Drop for WasmModulator {
    fn drop(&mut self) {
        self.guest.store.set_epoch_deadline(CALL_BUDGET);
        if let Err(error) = self.close.call(&mut self.guest.store, self.handle) {
            tracing::warn!(plugin = %self.module.name, %error, "plugin encoder did not close");
        }
    }
}
