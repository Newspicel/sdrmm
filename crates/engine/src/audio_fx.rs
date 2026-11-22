use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, mpsc},
    thread::JoinHandle,
};

use sdrmm_decoder::{AudioChain, ClickProfile};
use sdrmm_dsp::audio::noise::NeuralDenoiseError;
use sdrmm_wire::{AudioProcessing, AudioRoute};
use tokio::sync::{
    broadcast::{self, error::RecvError},
    watch,
};

use crate::{
    EngineError,
    audio::{self, AudioPacket, PcmBlock, PcmPayload},
    audio_recording::AudioTaps,
    denoise_models::DenoiseModels,
};

pub(crate) enum FxControl {
    Record(Option<AudioTaps>),
}

pub(crate) struct FxSource {
    pub(crate) pcm: broadcast::Receiver<PcmBlock>,
    pub(crate) channels: u8,
    pub(crate) profile: ClickProfile,
    pub(crate) capacity: usize,
}

struct FxStream {
    pcm_tx: broadcast::WeakSender<PcmBlock>,
    audio_tx: broadcast::WeakSender<AudioPacket>,
    control: mpsc::Sender<FxControl>,
    worker: JoinHandle<()>,
}

impl FxStream {
    fn live(&self) -> bool {
        !self.worker.is_finished() && self.pcm_tx.strong_count() > 0
    }
}

pub(crate) struct AudioFxHub {
    models: Arc<DenoiseModels>,
    settings: HashMap<String, watch::Sender<AudioProcessing>>,
    streams: HashMap<AudioRoute, FxStream>,
}

impl AudioFxHub {
    pub(crate) fn new(models: Arc<DenoiseModels>) -> Self {
        Self {
            models,
            settings: HashMap::new(),
            streams: HashMap::new(),
        }
    }

    pub(crate) fn set(&mut self, node: &str, settings: AudioProcessing) {
        match self.settings.get(node) {
            Some(tx) => {
                tx.send_if_modified(|current| {
                    let changed = *current != settings;
                    if changed {
                        *current = settings;
                    }
                    changed
                });
            }
            None => {
                self.settings
                    .insert(node.to_owned(), watch::channel(settings).0);
            }
        }
    }

    pub(crate) fn retain(&mut self, nodes: &HashSet<String>) {
        self.settings.retain(|node, _| nodes.contains(node));
        self.prune();
    }

    fn prune(&mut self) {
        self.streams.retain(|_, stream| stream.live());
    }

    #[cfg(test)]
    pub(crate) fn has(&self, route: &AudioRoute) -> bool {
        self.streams.get(route).is_some_and(FxStream::live)
    }

    pub(crate) fn subscribe_pcm(
        &mut self,
        route: &AudioRoute,
        source: impl FnOnce() -> Result<FxSource, EngineError>,
    ) -> Result<broadcast::Receiver<PcmBlock>, EngineError> {
        self.stream(route, source)?
            .pcm_tx
            .upgrade()
            .map(|tx| tx.subscribe())
            .ok_or_else(stopped)
    }

    pub(crate) fn subscribe_audio(
        &mut self,
        route: &AudioRoute,
        source: impl FnOnce() -> Result<FxSource, EngineError>,
    ) -> Result<broadcast::Receiver<AudioPacket>, EngineError> {
        self.stream(route, source)?
            .audio_tx
            .upgrade()
            .map(|tx| tx.subscribe())
            .ok_or_else(stopped)
    }

    pub(crate) fn control(
        &mut self,
        route: &AudioRoute,
        source: impl FnOnce() -> Result<FxSource, EngineError>,
    ) -> Result<mpsc::Sender<FxControl>, EngineError> {
        Ok(self.stream(route, source)?.control.clone())
    }

    fn stream(
        &mut self,
        route: &AudioRoute,
        source: impl FnOnce() -> Result<FxSource, EngineError>,
    ) -> Result<&FxStream, EngineError> {
        self.prune();
        if !self.streams.contains_key(route) {
            let stages = route
                .fx
                .iter()
                .map(|node| {
                    self.settings
                        .get(node)
                        .map(watch::Sender::subscribe)
                        .ok_or_else(|| {
                            EngineError::Audio(format!("audio FX `{node}` is not in the patch"))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let stream = spawn(source()?, stages, Arc::clone(&self.models))?;
            self.streams.insert(route.clone(), stream);
        }
        self.streams
            .get(route)
            .ok_or_else(|| EngineError::Audio("audio FX stream vanished".to_owned()))
    }
}

fn stopped() -> EngineError {
    EngineError::Audio("audio FX stream stopped".to_owned())
}

fn spawn(
    source: FxSource,
    settings: Vec<watch::Receiver<AudioProcessing>>,
    models: Arc<DenoiseModels>,
) -> Result<FxStream, EngineError> {
    let (pcm_tx, pcm_rx) = broadcast::channel(source.capacity);
    let (audio_tx, _) = broadcast::channel(audio::AUDIO_CHANNEL_CAP);
    let (control, commands) = mpsc::channel();
    let encoder = audio::spawn_encoder(source.channels, pcm_rx, audio_tx.clone())?;
    let weak_pcm = pcm_tx.downgrade();
    let weak_audio = audio_tx.downgrade();
    let worker = {
        std::thread::Builder::new()
            .name("sdrmm-audiofx".to_owned())
            .spawn(move || {
                sdrmm_device::schedule::claim(sdrmm_device::Latency::Interactive);
                Worker {
                    stages: settings
                        .into_iter()
                        .map(|settings| Stage::new(settings, Arc::clone(&models)))
                        .collect(),
                    profile: source.profile,
                    pcm_tx,
                    audio_tx,
                    commands,
                    recorder: None,
                    failed: false,
                    held: 0,
                }
                .run(source.pcm);
                drop(encoder);
            })
            .map_err(|e| EngineError::Audio(format!("spawn audio FX thread: {e}")))?
    };
    Ok(FxStream {
        pcm_tx: weak_pcm,
        audio_tx: weak_audio,
        control,
        worker,
    })
}

/// One audio chain following its settings and the loaded denoise models.
pub(crate) struct Stage {
    settings: watch::Receiver<AudioProcessing>,
    models: Arc<DenoiseModels>,
    generation: u64,
    chain: Option<AudioChain>,
}

impl Stage {
    pub(crate) fn new(
        settings: watch::Receiver<AudioProcessing>,
        models: Arc<DenoiseModels>,
    ) -> Self {
        Self {
            settings,
            generation: models.generation(),
            models,
            chain: None,
        }
    }

    /// Rebuilds the chain when its settings or models moved. False once the settings are gone.
    pub(crate) fn prepare(
        &mut self,
        channels: u8,
        profile: ClickProfile,
    ) -> Result<bool, NeuralDenoiseError> {
        let Ok(changed) = self.settings.has_changed() else {
            return Ok(false);
        };
        let generation = self.models.generation();
        let changed = changed || generation != self.generation;
        self.generation = generation;
        let planes = usize::from(channels).max(1);
        let stale = self
            .chain
            .as_ref()
            .is_none_or(|chain| chain.planes() != planes);
        if changed || stale {
            let settings = self.settings.borrow_and_update().clone();
            match &mut self.chain {
                Some(chain) => chain.configure(channels, &settings, profile)?,
                none => {
                    let nets = Arc::clone(&self.models);
                    *none = Some(AudioChain::new(channels, &settings, profile, nets)?);
                }
            }
        }
        Ok(true)
    }

    pub(crate) fn process(&mut self, pcm: &mut [f32]) -> Result<(), NeuralDenoiseError> {
        self.chain
            .as_mut()
            .map_or(Ok(()), |chain| chain.process_audio(pcm))
    }

    fn tail(&self) -> usize {
        self.chain.as_ref().map_or(0, AudioChain::tail)
    }
}

struct Worker {
    stages: Vec<Stage>,
    profile: ClickProfile,
    pcm_tx: broadcast::Sender<PcmBlock>,
    audio_tx: broadcast::Sender<AudioPacket>,
    commands: mpsc::Receiver<FxControl>,
    recorder: Option<AudioTaps>,
    failed: bool,
    /// Frames of the last open squelch still inside the chain.
    held: usize,
}

impl Worker {
    fn run(mut self, mut source: broadcast::Receiver<PcmBlock>) {
        loop {
            let block = match source.blocking_recv() {
                Ok(block) => block,
                Err(RecvError::Lagged(missed)) => {
                    tracing::warn!(missed, "audio FX fell behind its channel; audio dropped");
                    continue;
                }
                Err(RecvError::Closed) => return,
            };
            self.follow_commands();
            if self.idle() || !self.prepare(block.channels) {
                return;
            }
            match block.payload {
                PcmPayload::Samples(samples) => {
                    let payload = PcmPayload::Samples(self.processed(samples.to_vec()));
                    self.held = self.tail();
                    self.emit(PcmBlock { payload, ..block });
                }
                PcmPayload::Silence(frames) => {
                    self.silence(block.start_frame, block.channels, frames);
                }
            }
        }
    }

    fn emit(&self, block: PcmBlock) {
        if let Some(recorder) = &self.recorder {
            recorder.push(&block);
        }
        let _ = self.pcm_tx.send(block);
    }

    /// A closing squelch first pushes out what the chain still holds of the audio before it, so
    /// an over keeps its last words and the next one does not start with them.
    fn silence(&mut self, start_frame: u64, channels: u8, frames: usize) {
        let flushed = self.held.min(frames);
        self.held -= flushed;
        if flushed > 0 {
            let zeros = vec![0.0; flushed * usize::from(channels.max(1))];
            let payload = PcmPayload::Samples(self.processed(zeros));
            self.emit(PcmBlock {
                start_frame,
                channels,
                payload,
            });
        }
        if frames > flushed || flushed == 0 {
            self.emit(PcmBlock {
                start_frame: start_frame + flushed as u64,
                channels,
                payload: PcmPayload::Silence(frames - flushed),
            });
        }
    }

    fn follow_commands(&mut self) {
        while let Ok(command) = self.commands.try_recv() {
            match command {
                FxControl::Record(taps) => self.recorder = taps,
            }
        }
    }

    fn idle(&self) -> bool {
        self.recorder.is_none()
            && self.audio_tx.receiver_count() == 0
            && self.pcm_tx.receiver_count() <= 1
    }

    /// False once a stage's settings are gone.
    fn prepare(&mut self, channels: u8) -> bool {
        for stage in &mut self.stages {
            match stage.prepare(channels, self.profile) {
                Ok(true) => {}
                Ok(false) => return false,
                Err(error) => report(&mut self.failed, &error),
            }
        }
        true
    }

    fn processed(&mut self, mut pcm: Vec<f32>) -> Arc<[f32]> {
        for stage in &mut self.stages {
            if let Err(error) = stage.process(&mut pcm) {
                report(&mut self.failed, &error);
            }
        }
        Arc::from(pcm)
    }

    fn tail(&self) -> usize {
        self.stages.iter().map(Stage::tail).sum()
    }
}

pub(crate) fn report(failed: &mut bool, error: &NeuralDenoiseError) {
    if !*failed {
        tracing::error!(%error, "audio FX stage failed; its audio passes unprocessed");
        *failed = true;
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{
        AudioAgcMode, AudioFilterSettings, DenoiseMode, DenoiseModel, DenoiseSettings,
    };

    use super::*;

    fn tone(freq_hz: f64, amplitude: f32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|n| {
                amplitude
                    * (std::f64::consts::TAU * freq_hz * n as f64
                        / f64::from(sdrmm_decoder::AUDIO_RATE))
                    .sin() as f32
            })
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    fn source(tx: &broadcast::Sender<PcmBlock>) -> FxSource {
        FxSource {
            pcm: tx.subscribe(),
            channels: 1,
            profile: ClickProfile::Discriminator,
            capacity: 64,
        }
    }

    fn route(fx: &[&str]) -> AudioRoute {
        AudioRoute {
            fx: fx.iter().map(|&node| node.to_owned()).collect(),
            ..AudioRoute::channel(0, 1)
        }
    }

    fn highpass() -> AudioProcessing {
        AudioProcessing {
            filter: AudioFilterSettings {
                enabled: true,
                low_hz: 1_000.0,
                high_hz: 3_000.0,
            },
            ..AudioProcessing::default()
        }
    }

    fn feed(tx: &broadcast::Sender<PcmBlock>, samples: &[f32], start_frame: u64) {
        tx.send(PcmBlock {
            start_frame,
            channels: 1,
            payload: PcmPayload::Samples(Arc::from(samples)),
        })
        .expect("the worker listens");
    }

    fn collect(rx: &mut broadcast::Receiver<PcmBlock>, blocks: usize) -> Vec<f32> {
        let mut out = Vec::new();
        for _ in 0..blocks {
            match rx.blocking_recv().expect("a processed block").payload {
                PcmPayload::Samples(samples) => out.extend_from_slice(&samples),
                PcmPayload::Silence(frames) => out.resize(out.len() + frames, 0.0),
            }
        }
        out
    }

    #[test]
    fn a_route_runs_its_channel_audio_through_every_fx_node_in_order() {
        let (tx, _keep) = broadcast::channel(64);
        let mut hub = AudioFxHub::new(Arc::default());
        hub.set("a", highpass());
        hub.set(
            "b",
            AudioProcessing {
                agc: AudioAgcMode::Fast,
                ..AudioProcessing::default()
            },
        );
        let mut rx = hub
            .subscribe_pcm(&route(&["a", "b"]), || Ok(source(&tx)))
            .expect("the route opens");
        let low = tone(200.0, 0.3, 4_800);
        for n in 0..20 {
            feed(&tx, &low, n * 4_800);
        }
        let out = collect(&mut rx, 20);
        assert_eq!(out.len(), 20 * 4_800);
        assert!(
            rms(&out[48_000..]) < 0.05,
            "the highpass let 200 Hz through"
        );
    }

    #[test]
    fn a_route_through_an_unknown_node_is_refused() {
        let (tx, _keep) = broadcast::channel::<PcmBlock>(8);
        let mut hub = AudioFxHub::new(Arc::default());
        let refused = hub.subscribe_pcm(&route(&["ghost"]), || Ok(source(&tx)));
        assert!(matches!(refused, Err(EngineError::Audio(_))));
    }

    #[test]
    fn a_settings_change_reaches_a_running_route() {
        let (tx, _keep) = broadcast::channel(64);
        let mut hub = AudioFxHub::new(Arc::default());
        hub.set("a", AudioProcessing::default());
        let mut rx = hub
            .subscribe_pcm(&route(&["a"]), || Ok(source(&tx)))
            .expect("the route opens");
        let low = tone(200.0, 0.3, 4_800);
        feed(&tx, &low, 0);
        assert!(rms(&collect(&mut rx, 1)) > 0.2);
        hub.set("a", highpass());
        for n in 1..11 {
            feed(&tx, &low, n * 4_800);
        }
        let out = collect(&mut rx, 10);
        assert!(rms(&out[24_000..]) < 0.05, "the new settings never arrived");
    }

    #[test]
    fn silence_passes_as_silence_with_its_stamp() {
        let (tx, _keep) = broadcast::channel(64);
        let mut hub = AudioFxHub::new(Arc::default());
        hub.set("a", highpass());
        let mut rx = hub
            .subscribe_pcm(&route(&["a"]), || Ok(source(&tx)))
            .expect("the route opens");
        tx.send(PcmBlock {
            start_frame: 777,
            channels: 1,
            payload: PcmPayload::Silence(960),
        })
        .expect("the worker listens");
        let block = rx.blocking_recv().expect("a block");
        assert_eq!(block.start_frame, 777);
        assert!(matches!(block.payload, PcmPayload::Silence(960)));
    }

    fn dpdfnet(dir: &std::path::Path) -> Arc<DenoiseModels> {
        let models = DenoiseModels::default();
        models.set_dir(dir.to_path_buf());
        let bytes = sdrmm_test_support::denoise_model(DenoiseModel::Dpdfnet2).expect("model");
        models
            .install(DenoiseModel::Dpdfnet2, &bytes)
            .expect("installs");
        Arc::new(models)
    }

    /// At zero strength the neural denoiser is a pure delay, so what leaves it is what went in, later.
    fn delaying() -> AudioProcessing {
        AudioProcessing {
            denoise: DenoiseSettings {
                enabled: true,
                mode: DenoiseMode::Neural,
                model: DenoiseModel::Dpdfnet2,
                strength: 0.0,
            },
            ..AudioProcessing::default()
        }
    }

    fn silence(tx: &broadcast::Sender<PcmBlock>, frames: usize, start_frame: u64) {
        tx.send(PcmBlock {
            start_frame,
            channels: 1,
            payload: PcmPayload::Silence(frames),
        })
        .expect("the worker listens");
    }

    fn heard(block: &PcmBlock) -> Option<&[f32]> {
        match &block.payload {
            PcmPayload::Samples(samples) => Some(samples),
            PcmPayload::Silence(_) => None,
        }
    }

    #[test]
    fn a_closing_squelch_gets_the_denoisers_tail_then_silence() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (tx, _keep) = broadcast::channel(64);
        let mut hub = AudioFxHub::new(dpdfnet(dir.path()));
        hub.set("a", delaying());
        let mut rx = hub
            .subscribe_pcm(&route(&["a"]), || Ok(source(&tx)))
            .expect("the route opens");
        let over = tone(1_000.0, 0.3, 4_800);
        for n in 0..10 {
            feed(&tx, &over, n * 4_800);
        }
        silence(&tx, 4_800, 48_000);
        silence(&tx, 960, 52_800);
        for _ in 0..10 {
            assert!(heard(&rx.blocking_recv().expect("the over")).is_some());
        }
        let tail = rx.blocking_recv().expect("the tail");
        let held = heard(&tail).expect("the tail is audio").len();
        assert_eq!(tail.start_frame, 48_000);
        assert!(held > 0 && held < 4_800, "{held} frames held");
        assert!(
            rms(heard(&tail).unwrap_or_default()) > 0.1,
            "the last words were lost"
        );
        let closed = rx.blocking_recv().expect("the rest of the silence");
        assert_eq!(closed.start_frame, 48_000 + held as u64);
        assert!(matches!(closed.payload, PcmPayload::Silence(n) if n == 4_800 - held));
        let quiet = rx.blocking_recv().expect("more silence");
        assert_eq!(quiet.start_frame, 52_800);
        assert!(matches!(quiet.payload, PcmPayload::Silence(960)));
    }

    #[test]
    fn the_next_over_does_not_start_with_the_last_ones_words() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (tx, _keep) = broadcast::channel(64);
        let mut hub = AudioFxHub::new(dpdfnet(dir.path()));
        hub.set("a", delaying());
        let mut rx = hub
            .subscribe_pcm(&route(&["a"]), || Ok(source(&tx)))
            .expect("the route opens");
        feed(&tx, &tone(1_000.0, 0.3, 4_800), 0);
        silence(&tx, 4_800, 4_800);
        feed(&tx, &tone(700.0, 0.3, 4_800), 9_600);
        let mut next = Vec::new();
        while next.is_empty() {
            let block = rx.blocking_recv().expect("a block");
            if block.start_frame == 9_600 {
                next = heard(&block).expect("the next over").to_vec();
            }
        }
        assert!(
            rms(&next[..1_000]) < 1e-3,
            "the last over leaked into the next"
        );
        assert!(rms(&next[3_800..]) > 0.1, "the next over is missing");
    }

    #[test]
    fn removing_a_node_from_the_patch_ends_the_routes_through_it() {
        let (tx, _keep) = broadcast::channel(64);
        let mut hub = AudioFxHub::new(Arc::default());
        hub.set("a", AudioProcessing::default());
        let mut rx = hub
            .subscribe_pcm(&route(&["a"]), || Ok(source(&tx)))
            .expect("the route opens");
        hub.retain(&HashSet::new());
        feed(&tx, &[0.1; 480], 0);
        assert!(matches!(rx.blocking_recv(), Err(RecvError::Closed)));
    }

    #[test]
    fn a_route_nobody_listens_to_stops() {
        let (tx, _keep) = broadcast::channel(64);
        let mut hub = AudioFxHub::new(Arc::default());
        hub.set("a", AudioProcessing::default());
        let rx = hub
            .subscribe_pcm(&route(&["a"]), || Ok(source(&tx)))
            .expect("the route opens");
        drop(rx);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while hub.has(&route(&["a"])) {
            assert!(
                std::time::Instant::now() < deadline,
                "the idle route kept running"
            );
            let _ = tx.send(PcmBlock {
                start_frame: 0,
                channels: 1,
                payload: PcmPayload::Silence(480),
            });
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}
