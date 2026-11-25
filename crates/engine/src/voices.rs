//! Voices the server carries into the patch, a microphone or a voice chat, as audio any audio
//! input subscribes to the way it subscribes to a channel, and records the way it records one.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    thread::JoinHandle,
};

use sdrmm_wire::{AudioRecordingStatus, NodeStream, ServerEvent, StateScope};
use tokio::sync::broadcast;

use sdrmm_decoder::ClickProfile;

use crate::{
    AudioPacket, EngineError, PcmBlock, PcmPayload,
    audio::{AUDIO_CHANNEL_CAP, spawn_encoder},
    audio_fx::{Stage, report},
    audio_recording::{self, AudioRecorderTap, AudioRecordingShared},
};

/// Twenty-millisecond frames a listener may fall behind by: a second and a quarter.
const VOICE_QUEUE: usize = 64;

/// One recorder's take of a voice.
pub(crate) struct VoiceRecording {
    pub(crate) recorder: String,
    pub(crate) file: String,
    pub(crate) started_at: String,
    pub(crate) tap: AudioRecorderTap,
    pub(crate) shared: Arc<AudioRecordingShared>,
    pub(crate) writer: JoinHandle<()>,
}

impl VoiceRecording {
    fn status(&self) -> AudioRecordingStatus {
        AudioRecordingStatus {
            node: self.recorder.clone(),
            fx: None,
            file: self.file.clone(),
            started_at: self.started_at.clone(),
            channels: 1,
            frames: self.shared.frames(),
            bytes: self.shared.bytes(),
            error: self.shared.error(),
        }
    }

    /// Lets the writer run out and close the file.
    pub(crate) fn finish(self) -> AudioRecordingStatus {
        let Self {
            recorder,
            file,
            started_at,
            tap,
            shared,
            writer,
        } = self;
        drop(tap);
        if writer.join().is_err() {
            tracing::error!(%file, "the voice recording writer panicked");
        }
        AudioRecordingStatus {
            node: recorder,
            fx: None,
            file,
            started_at,
            channels: 1,
            frames: shared.frames(),
            bytes: shared.bytes(),
            error: shared.error(),
        }
    }
}

/// A voice's Audio FX chain, run on each frame before anyone hears it.
pub(crate) struct VoiceFx {
    stage: Stage,
    failed: bool,
}

impl VoiceFx {
    /// False once the node's chain is off.
    pub(crate) fn run(&mut self, pcm: &mut [f32]) -> bool {
        match self.stage.prepare(1, ClickProfile::default()) {
            Ok(false) => return false,
            Ok(true) => {}
            Err(error) => report(&mut self.failed, &error),
        }
        if let Err(error) = self.stage.process(pcm) {
            report(&mut self.failed, &error);
        }
        true
    }
}

struct VoiceStream {
    pcm_tx: broadcast::Sender<PcmBlock>,
    audio_tx: broadcast::Sender<AudioPacket>,
    next_frame: u64,
    recordings: Vec<VoiceRecording>,
    fx: Option<VoiceFx>,
    /// Ends on its own once `pcm_tx` drops.
    _encoder: JoinHandle<()>,
}

impl VoiceStream {
    fn open(stage: Option<Stage>) -> Result<Self, EngineError> {
        let (pcm_tx, pcm_rx) = broadcast::channel(VOICE_QUEUE);
        let (audio_tx, _) = broadcast::channel(AUDIO_CHANNEL_CAP);
        let encoder = spawn_encoder(1, pcm_rx, audio_tx.clone())?;
        Ok(Self {
            pcm_tx,
            audio_tx,
            next_frame: 0,
            recordings: Vec::new(),
            fx: stage.map(voice_fx),
            _encoder: encoder,
        })
    }

    fn node_stream(&self, node: &str) -> NodeStream {
        NodeStream {
            node: node.to_owned(),
            audio_recordings: self.recordings.iter().map(VoiceRecording::status).collect(),
            ..NodeStream::default()
        }
    }
}

/// Every voice a wire listens to, by its node.
#[derive(Default)]
pub(crate) struct VoiceHub {
    streams: HashMap<String, VoiceStream>,
}

fn voice_fx(stage: Stage) -> VoiceFx {
    VoiceFx {
        stage,
        failed: false,
    }
}

impl VoiceHub {
    /// `node`'s stream, opened with `stage` as its Audio FX when it was not running.
    fn stream(
        &mut self,
        node: &str,
        stage: Option<Stage>,
    ) -> Result<&mut VoiceStream, EngineError> {
        if !self.streams.contains_key(node) {
            self.streams
                .insert(node.to_owned(), VoiceStream::open(stage)?);
        }
        self.streams
            .get_mut(node)
            .ok_or_else(|| EngineError::Audio(format!("voice `{node}` vanished")))
    }

    pub(crate) fn subscribe_pcm(
        &mut self,
        node: &str,
        stage: Option<Stage>,
    ) -> Result<broadcast::Receiver<PcmBlock>, EngineError> {
        Ok(self.stream(node, stage)?.pcm_tx.subscribe())
    }

    pub(crate) fn subscribe_audio(
        &mut self,
        node: &str,
        stage: Option<Stage>,
    ) -> Result<broadcast::Receiver<AudioPacket>, EngineError> {
        Ok(self.stream(node, stage)?.audio_tx.subscribe())
    }

    /// Runs a running voice through the chain its node just switched on.
    pub(crate) fn attach_fx(&mut self, node: &str, stage: Option<Stage>) {
        if let (Some(stream), Some(stage)) = (self.streams.get_mut(node), stage) {
            stream.fx = Some(voice_fx(stage));
        }
    }

    pub(crate) fn take_fx(&mut self, node: &str) -> Option<VoiceFx> {
        self.streams.get_mut(node)?.fx.take()
    }

    /// Hands back a chain taken to run, unless a newer one came on meanwhile.
    pub(crate) fn restore_fx(&mut self, node: &str, fx: VoiceFx) {
        if let Some(stream) = self.streams.get_mut(node) {
            stream.fx.get_or_insert(fx);
        }
    }

    /// Hands one mono 48 kHz frame to whoever listens to `node`; a voice nobody listens to
    /// costs nothing.
    pub(crate) fn speak(&mut self, node: &str, pcm: &[f32]) {
        let Some(stream) = self.streams.get_mut(node) else {
            return;
        };
        let block = PcmBlock {
            start_frame: stream.next_frame,
            channels: 1,
            payload: PcmPayload::Samples(Arc::from(pcm)),
        };
        stream.next_frame = stream.next_frame.wrapping_add(pcm.len() as u64);
        for recording in &stream.recordings {
            if !recording.tap.push(block.clone()) {
                tracing::debug!(node, recorder = %recording.recorder, "a voice take fell behind");
            }
        }
        if stream.pcm_tx.send(block).is_err() {
            tracing::debug!(node, "a voice spoke after its encoder stopped");
        }
    }

    pub(crate) fn records(&self, voice: &str, recorder: &str) -> bool {
        self.streams.get(voice).is_some_and(|stream| {
            stream
                .recordings
                .iter()
                .any(|recording| recording.recorder == recorder)
        })
    }

    pub(crate) fn record(
        &mut self,
        voice: &str,
        recording: VoiceRecording,
        stage: Option<Stage>,
    ) -> Result<AudioRecordingStatus, (EngineError, Box<VoiceRecording>)> {
        if self.records(voice, &recording.recorder) {
            return Err((
                EngineError::Recording("this recorder already records this voice".to_owned()),
                Box::new(recording),
            ));
        }
        match self.stream(voice, stage) {
            Ok(stream) => {
                let status = recording.status();
                stream.recordings.push(recording);
                Ok(status)
            }
            Err(error) => Err((error, Box::new(recording))),
        }
    }

    pub(crate) fn stop_recording(&mut self, voice: &str, recorder: &str) -> Option<VoiceRecording> {
        let stream = self.streams.get_mut(voice)?;
        let at = stream
            .recordings
            .iter()
            .position(|recording| recording.recorder == recorder)?;
        Some(stream.recordings.remove(at))
    }

    pub(crate) fn skip_silence(&self, voice: &str, recorder: &str, skip: bool) -> bool {
        self.streams
            .get(voice)
            .and_then(|stream| {
                stream
                    .recordings
                    .iter()
                    .find(|recording| recording.recorder == recorder)
            })
            .map(|recording| recording.shared.set_skip_silence(skip))
            .is_some()
    }

    /// Closes the voices of nodes no longer in the patch; their listeners see the end and their
    /// takes come back to be finished.
    pub(crate) fn retain(&mut self, nodes: &HashSet<String>) -> Vec<VoiceRecording> {
        let gone: Vec<String> = self
            .streams
            .keys()
            .filter(|node| !nodes.contains(*node))
            .cloned()
            .collect();
        gone.iter()
            .filter_map(|node| self.streams.remove(node))
            .flat_map(|stream| stream.recordings)
            .collect()
    }

    pub(crate) fn node_streams(&self) -> Vec<NodeStream> {
        self.streams
            .iter()
            .filter(|(_, stream)| !stream.recordings.is_empty())
            .map(|(node, stream)| stream.node_stream(node))
            .collect()
    }
}

impl crate::Engine {
    /// Starts `recorder`'s take of the voice `voice` as a mono WAV.
    pub(crate) fn start_voice_recording(
        &self,
        recorder: &str,
        voice: &str,
        skip_silence: bool,
    ) -> Result<AudioRecordingStatus, EngineError> {
        if self.voices().records(voice, recorder) {
            return Err(EngineError::Recording(
                "this recorder already records this voice".to_owned(),
            ));
        }
        let dir = self.audio_recordings_dir().ok_or_else(|| {
            EngineError::Recording("no recordings directory configured".to_owned())
        })?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| EngineError::RecordingIo(format!("create {}: {e}", dir.display())))?;
        let started_at = jiff::Timestamp::now();
        let writer = audio_recording::create_writer(
            &dir,
            &sdrmm_recorder::source_stem(started_at, voice),
            sdrmm_decoder::AUDIO_RATE,
            1,
        )?;
        let file = writer
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        let (tap, blocks, shared) =
            audio_recording::create_tap(f64::from(sdrmm_decoder::AUDIO_RATE), skip_silence);
        let writer = audio_recording::spawn_writer(writer, blocks, shared.clone())?;
        let take = VoiceRecording {
            recorder: recorder.to_owned(),
            file,
            started_at: started_at.to_string(),
            tap,
            shared,
            writer,
        };
        let stage = self.fx_hub().stage(voice);
        let recorded = self.voices().record(voice, take, stage);
        match recorded {
            Ok(status) => {
                self.voice_changed();
                Ok(status)
            }
            Err((error, take)) => {
                take.finish();
                Err(error)
            }
        }
    }

    pub fn stop_voice_recording(
        &self,
        recorder: &str,
        voice: &str,
    ) -> Result<AudioRecordingStatus, EngineError> {
        let take = self
            .voices()
            .stop_recording(voice, recorder)
            .ok_or_else(|| EngineError::Recording("not recording".to_owned()))?;
        let status = take.finish();
        self.voice_changed();
        self.emit(ServerEvent::StateChanged {
            scope: StateScope::Recordings,
        });
        Ok(status)
    }

    pub fn set_voice_skip_silence(
        &self,
        recorder: &str,
        voice: &str,
        skip: bool,
    ) -> Result<(), EngineError> {
        if self.voices().skip_silence(voice, recorder, skip) {
            Ok(())
        } else {
            Err(EngineError::Recording("not recording".to_owned()))
        }
    }

    fn voice_changed(&self) {
        self.lock().revision += 1;
        self.emit(ServerEvent::StateChanged {
            scope: StateScope::All,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listener_hears_what_the_voice_says_in_order() {
        let mut hub = VoiceHub::default();
        let mut heard = hub.subscribe_pcm("mic", None).expect("opens");
        hub.speak("mic", &[0.5; 960]);
        hub.speak("mic", &[0.25; 960]);
        let first = heard.try_recv().expect("first frame");
        let second = heard.try_recv().expect("second frame");
        assert_eq!((first.start_frame, second.start_frame), (0, 960));
        assert_eq!(first.channels, 1);
    }

    #[test]
    fn a_voice_nobody_listens_to_is_dropped_unopened() {
        let mut hub = VoiceHub::default();
        hub.speak("mic", &[0.5; 960]);
        assert!(hub.streams.is_empty());
    }

    #[test]
    fn a_removed_voice_closes_its_listeners() {
        let mut hub = VoiceHub::default();
        let mut heard = hub.subscribe_pcm("mic", None).expect("opens");
        hub.retain(&HashSet::new());
        assert!(matches!(
            heard.try_recv(),
            Err(broadcast::error::TryRecvError::Closed)
        ));
    }

    #[test]
    fn a_recorder_takes_what_the_voice_says_into_a_wav() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let engine = crate::Engine::new(Some(dir.path().to_path_buf()));
        let started = engine
            .start_voice_recording("rec", "mic", false)
            .expect("recording");
        assert_eq!(started.node, "rec");
        assert!(engine.start_voice_recording("rec", "mic", false).is_err());
        engine.speak_voice("mic", &[0.25; 960]);
        let live = engine.snapshot();
        assert_eq!(live.node_streams[0].node, "mic");
        assert_eq!(live.node_streams[0].audio_recordings[0].node, "rec");
        let done = engine.stop_voice_recording("rec", "mic").expect("stopped");
        assert_eq!((done.frames, done.error), (960, None));
        let wav = dir.path().join("audio").join(&done.file);
        let info = sdrmm_recorder::read_audio_info(&wav).expect("a wav");
        assert_eq!((info.channels, info.frames), (1, 960));
        assert!(engine.snapshot().node_streams.is_empty());
    }

    fn highpass() -> sdrmm_wire::AudioProcessing {
        sdrmm_wire::AudioProcessing {
            filter: sdrmm_wire::AudioFilterSettings {
                enabled: true,
                low_hz: 1_000.0,
                high_hz: 3_000.0,
            },
            ..sdrmm_wire::AudioProcessing::default()
        }
    }

    fn hum() -> Vec<f32> {
        (0..960)
            .map(|n| 0.3 * (std::f32::consts::TAU * 200.0 * n as f32 / 48_000.0).sin())
            .collect()
    }

    /// The loudness of what a listener hears of ten frames of 200 Hz hum, after the first five.
    fn heard_hum(engine: &crate::Engine, heard: &mut broadcast::Receiver<PcmBlock>) -> f32 {
        let mut late = Vec::new();
        for n in 0..10 {
            engine.speak_voice("mic", &hum());
            if let PcmPayload::Samples(samples) = heard.try_recv().expect("a frame").payload
                && n >= 5
            {
                late.extend_from_slice(&samples);
            }
        }
        (late.iter().map(|x| x * x).sum::<f32>() / late.len() as f32).sqrt()
    }

    #[test]
    fn a_voice_is_heard_through_its_node_s_audio_fx() {
        let engine = crate::Engine::new(None);
        engine.set_audio_fx("mic", highpass()).expect("valid");
        let stage = engine.fx_hub().stage("mic");
        let mut heard = engine.voices().subscribe_pcm("mic", stage).expect("opens");
        assert!(
            heard_hum(&engine, &mut heard) < 0.05,
            "the highpass let the hum through"
        );
    }

    #[test]
    fn a_running_voice_follows_its_chain_on_and_off() {
        let engine = crate::Engine::new(None);
        let mut heard = engine.voices().subscribe_pcm("mic", None).expect("opens");
        assert!(heard_hum(&engine, &mut heard) > 0.15);
        engine.set_audio_fx("mic", highpass()).expect("valid");
        assert!(
            heard_hum(&engine, &mut heard) < 0.05,
            "the chain never came on"
        );
        engine.retain_audio_fx(&HashSet::new());
        assert!(
            heard_hum(&engine, &mut heard) > 0.15,
            "the chain never went off"
        );
    }

    #[test]
    fn a_voice_reaches_the_browser_as_opus() {
        let mut hub = VoiceHub::default();
        let mut packets = hub.subscribe_audio("chat", None).expect("opens");
        hub.speak("chat", &[0.1; 960]);
        let packet = packets.blocking_recv().expect("one opus frame");
        assert_eq!((packet.channels, packet.timestamp), (1, 0));
    }
}
