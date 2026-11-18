use std::{
    borrow::Cow,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use num_complex::Complex;
use sdrmm_decoder::{Channel, ChannelError, ChannelOutputs, DecodedImage, VideoPicture};
use sdrmm_wire::{
    ChannelSettings, ChannelStatus, DecoderEvent, PositionFix,
    plugin::{OutputHeader, Picture, decoder as export, plane_of, process, read_pictures},
    present::{Field, FieldValue},
};
use wasmtime::TypedFunc;

use crate::{
    engine::CALL_BUDGET,
    module::{Guest, PluginModule, typed},
    samples::{SAMPLE_BYTES, floats, write_samples},
};

/// The channel hooks a plugin may leave out, each taking the trait's default when it does.
struct Hooks {
    retuned: Option<TypedFunc<i32, ()>>,
    position: Option<TypedFunc<(i32, u32, u32), ()>>,
    clock: Option<TypedFunc<(i32, i64), ()>>,
    lo_artifact: Option<TypedFunc<(i32, f64), ()>>,
    gated: Option<TypedFunc<i32, i32>>,
}

impl Hooks {
    fn of(guest: &mut Guest) -> Self {
        let instance = guest.instance;
        let store = &mut guest.store;
        Self {
            retuned: typed(&instance, store, export::RETUNED).ok(),
            position: typed(&instance, store, export::POSITION).ok(),
            clock: typed(&instance, store, export::CLOCK).ok(),
            lo_artifact: typed(&instance, store, export::LO_ARTIFACT).ok(),
            gated: typed(&instance, store, export::GATED).ok(),
        }
    }
}

/// One channel of a plugin decoder, on an instance of its own.
pub struct WasmChannel {
    module: Arc<PluginModule>,
    guest: Guest,
    handle: i32,
    apply: TypedFunc<(i32, u32, u32), i32>,
    input: TypedFunc<(i32, u32), u32>,
    process: TypedFunc<(i32, u32, u32), u32>,
    close: TypedFunc<i32, ()>,
    hooks: Hooks,
    gated: bool,
    /// A hook's trap, shown on the node face at the next block.
    trapped: Option<String>,
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
        settings: &ChannelSettings,
    ) -> Result<Self, ChannelError> {
        let mut guest = module.instantiate().map_err(opened)?;
        let open: TypedFunc<(f64, u32, u32), i32> =
            typed(&guest.instance, &mut guest.store, export::OPEN).map_err(opened)?;
        let handle = guest
            .call_json(&open, input_rate, settings)
            .map_err(opened)?;
        if handle < 0 {
            return Err(opened(guest.last_error()));
        }
        let hooks = Hooks::of(&mut guest);
        let mut channel = Self {
            apply: typed(&guest.instance, &mut guest.store, export::APPLY).map_err(opened)?,
            input: typed(&guest.instance, &mut guest.store, export::INPUT).map_err(opened)?,
            process: typed(&guest.instance, &mut guest.store, export::PROCESS).map_err(opened)?,
            close: typed(&guest.instance, &mut guest.store, export::CLOSE).map_err(opened)?,
            hooks,
            gated: true,
            module,
            guest,
            handle,
            trapped: None,
            fault: None,
            rejected: 0,
            skipped: 0,
            header: [0; OutputHeader::BYTES],
        };
        channel.gated = channel.ask_gated().map_err(opened)?;
        Ok(channel)
    }

    fn ask_gated(&mut self) -> Result<bool, String> {
        let Some(gated) = &self.hooks.gated else {
            return Ok(true);
        };
        self.guest.store.set_epoch_deadline(CALL_BUDGET);
        gated
            .call(&mut self.guest.store, self.handle)
            .map(|code| code != 0)
            .map_err(|error| error.to_string())
    }

    /// Stops the channel for good and says why on its node face.
    fn stop(&mut self, error: String, out: &mut ChannelOutputs) {
        tracing::error!(plugin = %self.module.name, %error, "plugin channel stopped");
        out.status = Some(ChannelStatus::readouts(vec![Field::new(
            "plugin",
            "Plugin",
            FieldValue::Text(format!("stopped: {error}")),
        )]));
        self.fault = Some(error);
    }

    /// Runs a hook; one that traps stops the channel at its next block.
    fn hook<P: wasmtime::WasmParams>(&mut self, hook: Option<TypedFunc<P, ()>>, params: P) {
        let Some(hook) = hook else {
            return;
        };
        self.guest.store.set_epoch_deadline(CALL_BUDGET);
        if let Err(error) = hook.call(&mut self.guest.store, params) {
            self.trapped.get_or_insert(error.to_string());
        }
    }

    fn run(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) -> Result<(), String> {
        let samples = u32::try_from(iq.len()).map_err(|_| "block too large".to_owned())?;
        self.guest.store.set_epoch_deadline(CALL_BUDGET);
        let ptr = self
            .input
            .call(&mut self.guest.store, (self.handle, samples))
            .map_err(|error| error.to_string())?;
        self.write_iq(ptr, iq)?;
        let flags = if out.symbols.wanted() {
            process::WANT_SYMBOLS
        } else {
            0
        };
        let header_ptr = self
            .process
            .call(&mut self.guest.store, (self.handle, samples, flags))
            .map_err(|error| error.to_string())?;
        self.guest
            .memory
            .read(&self.guest.store, header_ptr as usize, &mut self.header)
            .map_err(|error| error.to_string())?;
        let header = OutputHeader::from_bytes(self.header);
        self.collect(&header, out)
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

    fn collect(&mut self, header: &OutputHeader, out: &mut ChannelOutputs) -> Result<(), String> {
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
        for picture in read_pictures(slice(header.pictures_ptr, header.pictures_len as usize)?) {
            collect_picture(picture?, out);
        }
        if let Some(plane) = plane_of(header.plane) {
            let symbols = slice(header.symbols_ptr, header.symbols_len as usize * 4)?;
            let reference = slice(header.reference_ptr, header.reference_len as usize * 4)?;
            let tap = &mut out.symbols;
            tap.plane = Some(plane);
            tap.symbols.extend(floats(symbols));
            tap.reference.extend(floats(reference));
            tap.symbol_rate = header.symbol_rate;
            tap.evm = header.evm;
            tap.mer_db = header.mer_db;
            tap.margin = header.margin;
            tap.freq_error_hz = header.freq_error_hz;
        }
        self.rejected = header.rejected;
        self.skipped = self.skipped.max(header.skipped);
        Ok(())
    }
}

fn collect_picture(picture: Picture<'_>, out: &mut ChannelOutputs) {
    let frame = VideoPicture {
        width: picture.width,
        height: picture.height,
        luma: picture.luma.to_vec(),
        rgb: picture.rgb.to_vec(),
    };
    match picture.image {
        Some(image) => out.images.push(DecodedImage {
            source: Cow::Owned(image.source),
            mode: image.mode,
            complete: image.complete,
            lines: image.lines,
            picture: frame,
        }),
        None => out.video.push(frame),
    }
}

/// Nanoseconds from the Unix epoch to `time`, negative before it.
fn unix_ns(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_nanos()).unwrap_or(i64::MAX),
        Err(before) => i64::try_from(before.duration().as_nanos()).map_or(i64::MIN, |ns| -ns),
    }
}

impl Channel for WasmChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let code = self
            .guest
            .call_json(&self.apply, self.handle, &settings)
            .map_err(opened)?;
        if code < 0 {
            return Err(opened(self.guest.last_error()));
        }
        self.gated = self.ask_gated().map_err(opened)?;
        Ok(())
    }

    fn retuned(&mut self) {
        self.hook(self.hooks.retuned.clone(), self.handle);
    }

    fn position_changed(&mut self, fix: Option<&PositionFix>) {
        let Some(position) = self.hooks.position.clone() else {
            return;
        };
        let written = serde_json::to_vec(&fix)
            .map_err(|error| error.to_string())
            .and_then(|bytes| self.guest.write(&bytes));
        match written {
            Ok((ptr, len)) => self.hook(Some(position), (self.handle, ptr, len)),
            Err(error) => {
                self.trapped.get_or_insert(error);
            }
        }
    }

    fn captured_at(&mut self, time: SystemTime) {
        self.hook(self.hooks.clock.clone(), (self.handle, unix_ns(time)));
    }

    fn lo_artifact_at(&mut self, offset_hz: Option<f64>) {
        let offset = offset_hz.unwrap_or(f64::NAN);
        self.hook(self.hooks.lo_artifact.clone(), (self.handle, offset));
    }

    fn needs_gated_input(&self) -> bool {
        self.gated
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        if let Some(error) = self.trapped.take() {
            self.stop(error, out);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_reads_as_signed_nanoseconds_from_the_epoch() {
        let second = std::time::Duration::from_secs(1);
        assert_eq!(unix_ns(UNIX_EPOCH + second), 1_000_000_000);
        assert_eq!(unix_ns(UNIX_EPOCH - second), -1_000_000_000);
        assert_eq!(unix_ns(UNIX_EPOCH), 0);
    }
}
