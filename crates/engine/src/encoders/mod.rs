//! Encoders as nodes of their own: each runs on its own clock and hands its baseband to every
//! wire on its output, a transmitter, a baseband recorder or a network export alike.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
};

use sdrmm_decoder::encoder::{EncoderParams, encoder};
use sdrmm_wire::{
    AudioProcessing, EncoderInput, NetworkExportSettings, NetworkExportStatus, NodeStream,
    RecordingStatus, ServerEvent, StateScope,
};

use crate::{
    Engine, EngineError, NetworkExportState, check_export_request, denoise_models::DenoiseModels,
    join_network_writer, join_recording_writer, network_export, recording,
    recording::RecordingShared, remove_recording_files, transmit::AirTap,
};

mod run;
mod voice;

use run::{Command, EncoderShared, Run, Sink, SinkKind};
use voice::Voice;

/// What an encoder node runs: its mode and settings, and the chain its audio passes through.
#[derive(Clone, Debug, PartialEq)]
pub struct EncoderSpec {
    pub params: EncoderParams,
    pub audio: AudioProcessing,
}

/// Where an encoder's baseband sits: its rate and the width its mode keeps to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EncodedBand {
    pub(crate) rate: f64,
    pub(crate) width_hz: f64,
}

struct EncodedRecording {
    file: String,
    started_at: String,
    shared: Arc<RecordingShared>,
    writer: JoinHandle<()>,
}

impl EncodedRecording {
    fn status(&self) -> RecordingStatus {
        RecordingStatus {
            file: self.file.clone(),
            stream: 0,
            node: None,
            started_at: self.started_at.clone(),
            samples: self.shared.samples(),
            bytes: self.shared.bytes(),
            overruns: 0,
            error: self.shared.error(),
        }
    }

    fn finish(self) -> RecordingStatus {
        let Self {
            file,
            started_at,
            shared,
            writer,
        } = self;
        join_recording_writer(writer);
        RecordingStatus {
            file,
            stream: 0,
            node: None,
            started_at,
            samples: shared.samples(),
            bytes: shared.bytes(),
            overruns: 0,
            error: shared.error(),
        }
    }
}

struct Running {
    spec: EncoderSpec,
    band: EncodedBand,
    commands: mpsc::Sender<Command>,
    voice: Option<Voice>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    recording: Option<EncodedRecording>,
    export: Option<NetworkExportState>,
}

impl Running {
    fn start(spec: &EncoderSpec, models: &Arc<DenoiseModels>) -> Result<Self, EngineError> {
        let found = encoder(&spec.params.type_id)?;
        let descriptor = found.descriptor();
        let band = EncodedBand {
            rate: descriptor.rate_hz,
            width_hz: found.width_hz(&spec.params)?,
        };
        let tx = found.open(&spec.params)?;
        let (commands, received) = mpsc::channel();
        let voice = (descriptor.input == EncoderInput::Audio)
            .then(|| Voice::spawn(spec.audio.clone(), Arc::clone(models), commands.clone()))
            .transpose()?;
        let stop = Arc::new(AtomicBool::new(false));
        let halt = stop.clone();
        let run = Run::new(tx, band.rate, Arc::new(EncoderShared::default()));
        let thread = std::thread::Builder::new()
            .name("sdrmm-encoder".to_owned())
            .spawn(move || {
                sdrmm_device::schedule::claim(sdrmm_device::Latency::Interactive);
                run.run(&received, &halt);
            })
            .map_err(|error| EngineError::Audio(format!("spawn encoder thread: {error}")))?;
        Ok(Self {
            spec: spec.clone(),
            band,
            commands,
            voice,
            stop,
            thread: Some(thread),
            recording: None,
            export: None,
        })
    }

    fn send(&self, node: &str, command: Command) -> Result<(), EngineError> {
        self.commands
            .send(command)
            .map_err(|_| EngineError::EncoderNotRunning(node.to_owned()))
    }

    fn node_stream(&self, node: &str) -> Option<NodeStream> {
        if self.recording.is_none() && self.export.is_none() {
            return None;
        }
        Some(NodeStream {
            node: node.to_owned(),
            baseband_recording: self.recording.as_ref().map(EncodedRecording::status),
            network_export: self.export.as_ref().map(|export| export.status(0)),
            ..NodeStream::default()
        })
    }

    /// Stops the clock and lets every sink on it finish.
    fn halt(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("an encoder thread panicked");
        }
        if let Some(recording) = self.recording.take() {
            recording.finish();
        }
        if let Some(mut export) = self.export.take() {
            export.join();
        }
    }
}

/// Every encoder node that runs, by its node.
#[derive(Default)]
pub(crate) struct EncoderHub {
    runs: HashMap<String, Running>,
}

impl EncoderHub {
    fn running(&self, node: &str) -> Result<&Running, EngineError> {
        self.runs
            .get(node)
            .ok_or_else(|| EngineError::EncoderNotRunning(node.to_owned()))
    }

    fn running_mut(&mut self, node: &str) -> Result<&mut Running, EngineError> {
        self.runs
            .get_mut(node)
            .ok_or_else(|| EngineError::EncoderNotRunning(node.to_owned()))
    }

    pub(crate) fn band(&self, node: &str) -> Result<EncodedBand, EngineError> {
        Ok(self.running(node)?.band)
    }

    pub(crate) fn attach_air(&self, node: &str, tap: AirTap) -> Result<(), EngineError> {
        self.running(node)?
            .send(node, Command::Attach(Sink::Air(tap)))
    }

    pub(crate) fn node_streams(&self) -> Vec<NodeStream> {
        self.runs
            .iter()
            .filter_map(|(node, running)| running.node_stream(node))
            .collect()
    }
}

impl Engine {
    pub(crate) fn encoders(&self) -> MutexGuard<'_, EncoderHub> {
        self.encoders.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs the encoder `node`, or brings a running one in line with `spec`. A new mode starts
    /// it over; new settings retune it in place, keeping every sink on it.
    pub fn run_encoder(&self, node: &str, spec: &EncoderSpec) -> Result<(), EngineError> {
        spec.audio.validate().map_err(EngineError::Audio)?;
        let restarted = {
            let mut hub = self.encoders();
            match hub.runs.get_mut(node) {
                Some(running) if running.spec == *spec => return Ok(()),
                Some(running) if running.spec.params.type_id == spec.params.type_id => {
                    if running.spec.params != spec.params {
                        let tx = encoder(&spec.params.type_id)?.open(&spec.params)?;
                        running.send(node, Command::Retune(tx))?;
                    }
                    if let Some(voice) = &running.voice {
                        voice.set(spec.audio.clone());
                    }
                    running.spec = spec.clone();
                    return Ok(());
                }
                _ => {
                    let started = Running::start(spec, &self.denoise_models)?;
                    hub.runs.insert(node.to_owned(), started)
                }
            }
        };
        if let Some(old) = restarted {
            old.halt();
        }
        self.encoder_changed();
        Ok(())
    }

    /// Stops every encoder but those of `nodes`.
    pub fn retain_encoders(&self, nodes: &HashSet<String>) {
        let stopped: Vec<Running> = {
            let mut hub = self.encoders();
            let gone: Vec<String> = hub
                .runs
                .keys()
                .filter(|node| !nodes.contains(*node))
                .cloned()
                .collect();
            gone.iter()
                .filter_map(|node| hub.runs.remove(node))
                .collect()
        };
        if stopped.is_empty() {
            return;
        }
        for running in stopped {
            running.halt();
        }
        self.encoder_changed();
    }

    #[must_use]
    pub fn runs_encoder(&self, node: &str) -> bool {
        self.encoders().runs.contains_key(node)
    }

    /// What the encoder `node` runs, if it runs.
    #[must_use]
    pub fn encoder_spec(&self, node: &str) -> Option<EncoderSpec> {
        self.encoders()
            .runs
            .get(node)
            .map(|running| running.spec.clone())
    }

    /// Feeds mono audio at the encoder's rate. Audio from different `lane`s is summed in time.
    pub fn encoder_audio(&self, node: &str, lane: u64, pcm: Vec<f32>) -> Result<(), EngineError> {
        let hub = self.encoders();
        let running = hub.running(node)?;
        let voice = running
            .voice
            .as_ref()
            .ok_or_else(|| EngineError::Transmit("this encoder takes no audio".to_owned()))?;
        if voice.speak(lane, pcm) {
            Ok(())
        } else {
            Err(EngineError::EncoderNotRunning(node.to_owned()))
        }
    }

    /// Queues one message, shaped by the form its encoder declares.
    pub fn encoder_message(
        &self,
        node: &str,
        message: &serde_json::Value,
    ) -> Result<(), EngineError> {
        let hub = self.encoders();
        let running = hub.running(node)?;
        let payload = encoder(&running.spec.params.type_id)?.compose(message)?;
        running.send(node, Command::Payload(payload))
    }

    pub fn start_encoder_baseband_recording(
        &self,
        node: &str,
    ) -> Result<RecordingStatus, EngineError> {
        let band = self.encoders().band(node)?;
        let dir = self.recordings_dir.clone().ok_or_else(|| {
            EngineError::Recording("no recordings directory configured".to_owned())
        })?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| EngineError::RecordingIo(format!("create {}: {e}", dir.display())))?;
        let started_at = jiff::Timestamp::now();
        let (sigmf, file) = recording::create_writer(
            &dir,
            &format!("bb_{node}"),
            0,
            started_at,
            band.rate,
            0.0,
            "encoder",
        )?;
        let stem = sigmf.stem().to_path_buf();
        let (tap, position, messages, shared) = recording::create_tap(band.rate);
        drop(position);
        let writer = recording::spawn_writer(sigmf, messages, shared.clone())?;
        let take = EncodedRecording {
            file,
            started_at: started_at.to_string(),
            shared,
            writer,
        };
        let committed = {
            let mut hub = self.encoders();
            match hub.running_mut(node) {
                Ok(running) if running.recording.is_none() => running
                    .send(node, Command::Attach(Sink::Record { tap, at: 0 }))
                    .map(|()| {
                        let status = take.status();
                        running.recording = Some(take);
                        status
                    })
                    .map_err(|error| (error, None)),
                Ok(_) => Err((
                    EngineError::Recording("this encoder is already recording".to_owned()),
                    Some((tap, take)),
                )),
                Err(error) => Err((error, Some((tap, take)))),
            }
        };
        match committed {
            Ok(status) => {
                self.encoder_changed();
                Ok(status)
            }
            Err((error, unused)) => {
                if let Some((tap, take)) = unused {
                    drop(tap);
                    take.finish();
                }
                remove_recording_files(&stem);
                Err(error)
            }
        }
    }

    pub fn stop_encoder_baseband_recording(
        &self,
        node: &str,
    ) -> Result<RecordingStatus, EngineError> {
        let take = {
            let mut hub = self.encoders();
            let running = hub.running_mut(node)?;
            let take = running
                .recording
                .take()
                .ok_or_else(|| EngineError::Recording("not recording".to_owned()))?;
            if running
                .send(node, Command::Detach(SinkKind::Record))
                .is_err()
            {
                tracing::debug!(node, "the encoder stopped before its take did");
            }
            take
        };
        let status = take.finish();
        self.encoder_changed();
        self.emit(ServerEvent::StateChanged {
            scope: StateScope::Recordings,
        });
        Ok(status)
    }

    pub fn start_encoder_network_export(
        &self,
        node: &str,
        export: String,
        settings: NetworkExportSettings,
    ) -> Result<NetworkExportStatus, EngineError> {
        check_export_request(&export, &settings)?;
        let band = self.encoders().band(node)?;
        let network_export::Started {
            tap,
            shared,
            writer,
            ..
        } = network_export::start(&settings)?;
        let state = NetworkExportState {
            clients_seen: 0,
            node: export,
            stream: 0,
            settings,
            sample_rate: band.rate.round() as u64,
            center_hz: 0,
            shared,
            refeed: None,
            writer: Some(writer),
            overruns_at_start: 0,
            samples_seen: 0,
            error_seen: false,
        };
        let committed = {
            let mut hub = self.encoders();
            match hub.running_mut(node) {
                Ok(running) if running.export.is_none() => {
                    match running.send(node, Command::Attach(Sink::Export(tap))) {
                        Ok(()) => {
                            let status = state.status(0);
                            running.export = Some(state);
                            Ok(status)
                        }
                        Err(error) => Err((error, state)),
                    }
                }
                Ok(_) => Err((
                    EngineError::NetworkExport("this encoder is already being exported".to_owned()),
                    state,
                )),
                Err(error) => Err((error, state)),
            }
        };
        match committed {
            Ok(status) => {
                self.encoder_changed();
                Ok(status)
            }
            Err((error, mut state)) => {
                if let Some(writer) = state.writer.take() {
                    join_network_writer(writer);
                }
                Err(error)
            }
        }
    }

    pub fn stop_encoder_network_export(
        &self,
        node: &str,
        export: &str,
    ) -> Result<NetworkExportStatus, EngineError> {
        let mut state = {
            let mut hub = self.encoders();
            let running = hub.running_mut(node)?;
            if running
                .export
                .as_ref()
                .is_none_or(|held| held.node != export)
            {
                return Err(EngineError::NetworkExport(
                    "this encoder is not being exported".to_owned(),
                ));
            }
            if running
                .send(node, Command::Detach(SinkKind::Export))
                .is_err()
            {
                tracing::debug!(node, "the encoder stopped before its export did");
            }
            running.export.take()
        }
        .ok_or_else(|| EngineError::NetworkExport("the export vanished".to_owned()))?;
        state.join();
        let status = state.status(0);
        self.encoder_changed();
        Ok(status)
    }

    fn encoder_changed(&self) {
        self.lock().revision += 1;
        self.emit(ServerEvent::StateChanged {
            scope: StateScope::All,
        });
    }
}

#[cfg(test)]
mod tests;
