use std::{
    sync::{Arc, mpsc},
    time::Duration,
};

use sdrmm_decoder::{ClickProfile, encoder::TxPayload};
use sdrmm_dsp::audio::mix::MixBus;
use sdrmm_wire::AudioProcessing;
use tokio::sync::watch;

use super::worker::TxCommand;
use crate::{
    EngineError,
    audio_fx::{Stage, report},
    denoise_models::DenoiseModels,
};

/// A voice encoder's audio chain. It runs on its own thread so a heavy denoiser never stalls
/// the modulator, and ends when the transmitter drops it.
pub(crate) struct Voice {
    pcm: mpsc::Sender<(u64, Vec<f32>)>,
    settings: watch::Sender<AudioProcessing>,
}

impl Voice {
    pub(crate) fn spawn(
        settings: AudioProcessing,
        models: Arc<DenoiseModels>,
        out: mpsc::Sender<TxCommand>,
    ) -> Result<Self, EngineError> {
        let (pcm, received) = mpsc::channel();
        let (settings, watched) = watch::channel(settings);
        std::thread::Builder::new()
            .name("sdrmm-tx-voice".to_owned())
            .spawn(move || run(&received, Stage::new(watched, models), &out))
            .map_err(|error| EngineError::Transmit(format!("spawn voice thread: {error}")))?;
        Ok(Self { pcm, settings })
    }

    /// False once the chain has stopped. Sources with different lanes are mixed in time.
    pub(crate) fn speak(&self, lane: u64, pcm: Vec<f32>) -> bool {
        self.pcm.send((lane, pcm)).is_ok()
    }

    pub(crate) fn set(&self, settings: AudioProcessing) {
        self.settings.send_if_modified(|current| {
            let changed = *current != settings;
            if changed {
                *current = settings;
            }
            changed
        });
    }
}

const MAX_LANE_LAG: usize = 4_800;
const FLUSH_AFTER: Duration = Duration::from_millis(40);

fn run(
    received: &mpsc::Receiver<(u64, Vec<f32>)>,
    mut stage: Stage,
    out: &mpsc::Sender<TxCommand>,
) {
    let mut failed = false;
    let mut bus = MixBus::new(MAX_LANE_LAG);
    let mut drained = Vec::new();
    loop {
        match received.recv_timeout(FLUSH_AFTER) {
            Ok((lane, samples)) => {
                bus.push(lane, &samples);
                bus.drain(&mut drained);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => bus.flush(&mut drained),
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        if drained.is_empty() {
            continue;
        }
        let mut pcm = std::mem::take(&mut drained);
        match stage.prepare(1, ClickProfile::default()) {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => report(&mut failed, &error),
        }
        if let Err(error) = stage.process(&mut pcm) {
            report(&mut failed, &error);
        }
        if out.send(TxCommand::Payload(TxPayload::Audio(pcm))).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sdrmm_wire::AudioFilterSettings;

    use super::*;

    fn tone(freq_hz: f64, len: usize) -> Vec<f32> {
        (0..len)
            .map(|n| {
                0.3 * (std::f64::consts::TAU * freq_hz * n as f64
                    / f64::from(sdrmm_decoder::AUDIO_RATE))
                .sin() as f32
            })
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    fn bandpass() -> AudioProcessing {
        AudioProcessing {
            filter: AudioFilterSettings {
                enabled: true,
                low_hz: 1_000.0,
                high_hz: 3_000.0,
            },
            ..AudioProcessing::default()
        }
    }

    fn heard(commands: &mpsc::Receiver<TxCommand>, blocks: usize) -> Vec<f32> {
        let mut out = Vec::new();
        for _ in 0..blocks {
            match commands.recv_timeout(Duration::from_secs(5)) {
                Ok(TxCommand::Payload(TxPayload::Audio(pcm))) => out.extend(pcm),
                Ok(_) => panic!("the voice sent something other than audio"),
                Err(error) => panic!("no audio reached the modulator: {error}"),
            }
        }
        out
    }

    #[test]
    fn a_voice_runs_its_audio_through_the_chain_before_the_modulator() {
        let (out, commands) = mpsc::channel();
        let voice = Voice::spawn(bandpass(), Arc::default(), out).expect("spawned");
        for _ in 0..20 {
            assert!(voice.speak(1, tone(200.0, 4_800)));
        }
        let pcm = heard(&commands, 20);
        assert!(rms(&pcm[48_000..]) < 0.05, "200 Hz passed the bandpass");
    }

    #[test]
    fn a_voice_follows_new_settings() {
        let (out, commands) = mpsc::channel();
        let voice = Voice::spawn(AudioProcessing::default(), Arc::default(), out).expect("spawned");
        voice.speak(1, tone(200.0, 4_800));
        assert!(rms(&heard(&commands, 1)) > 0.2);
        voice.set(bandpass());
        for _ in 0..10 {
            voice.speak(1, tone(200.0, 4_800));
        }
        assert!(rms(&heard(&commands, 10)[24_000..]) < 0.05);
    }

    #[test]
    fn one_source_passes_through_unchanged_and_at_once() {
        let (out, commands) = mpsc::channel();
        let voice = Voice::spawn(AudioProcessing::default(), Arc::default(), out).expect("spawned");
        let pcm = tone(300.0, 960);
        assert!(voice.speak(7, pcm.clone()));
        assert_eq!(heard(&commands, 1), pcm);
    }

    #[test]
    fn the_last_words_are_not_held_back_by_a_quiet_lane() {
        let (out, commands) = mpsc::channel();
        let voice = Voice::spawn(AudioProcessing::default(), Arc::default(), out).expect("spawned");
        assert!(voice.speak(1, Vec::new()));
        assert!(voice.speak(2, Vec::new()));
        assert!(voice.speak(1, vec![0.1; 480]));
        assert_eq!(heard(&commands, 1), vec![0.1; 480]);
    }

    #[test]
    fn two_sources_on_one_voice_are_mixed_not_interleaved() {
        let (out, commands) = mpsc::channel();
        let voice = Voice::spawn(AudioProcessing::default(), Arc::default(), out).expect("spawned");
        assert!(voice.speak(1, Vec::new()));
        assert!(voice.speak(2, Vec::new()));
        assert!(voice.speak(1, vec![0.1; 4_800]));
        assert!(voice.speak(2, vec![0.2; 4_800]));
        let pcm = heard(&commands, 1);
        assert_eq!(pcm.len(), 4_800);
        assert!(pcm.iter().all(|s| (s - 0.3).abs() < 1e-4));
    }
}
