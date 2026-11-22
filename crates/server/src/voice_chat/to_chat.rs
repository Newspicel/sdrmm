use std::sync::Arc;

use sdrmm_dsp::audio::{downmix::mono_mix, mix::MixBus, voice_gate::VoiceGate};
use sdrmm_engine::{PcmBlock, PcmPayload};
use sdrmm_voicechat::FRAME;
use sdrmm_wire::VoiceGateSettings;
use tokio::sync::{
    broadcast::{self, error::RecvError},
    mpsc::{self, error::TrySendError},
    watch,
};

use super::{
    MIX_LAG, Tuning,
    gate::{gate, passes, scaled},
    status::Meters,
};

/// Wired channel audio on its way into the voice channel.
pub(super) enum Feed {
    Audio { lane: u64, mono: Vec<f32> },
    Gone(u64),
}

/// Mixes the wired channels and cuts the mix into frames the gate lets into the chat.
pub(super) struct ToChat {
    bus: MixBus,
    held: Vec<f32>,
    gate: VoiceGate,
}

impl ToChat {
    pub(super) fn new(settings: &VoiceGateSettings) -> Self {
        Self {
            bus: MixBus::new(MIX_LAG),
            held: Vec::with_capacity(2 * FRAME),
            gate: gate(settings),
        }
    }

    pub(super) fn take(&mut self, feed: Feed) {
        match feed {
            Feed::Audio { lane, mono } => self.bus.push(lane, &mono),
            Feed::Gone(lane) => self.bus.forget(lane),
        }
        self.bus.drain(&mut self.held);
    }

    /// The next frame to send, or None once less than a frame is held. Frames the gate keeps
    /// back are passed over.
    pub(super) fn next_frame(
        &mut self,
        settings: &VoiceGateSettings,
        meters: &Meters,
    ) -> Option<Vec<f32>> {
        while self.held.len() >= FRAME {
            let frame: Vec<f32> = self.held.drain(..FRAME).collect();
            let open = passes(&mut self.gate, settings, &frame);
            meters.chat(Some(self.gate.level_db()), open);
            if open {
                return Some(scaled(&frame, settings));
            }
        }
        None
    }
}

/// Sends the mixed channels to the chat until every wire is gone or the link closes.
pub(super) async fn run(
    mut feeds: mpsc::Receiver<Feed>,
    send: mpsc::Sender<Vec<f32>>,
    tuning: watch::Receiver<Tuning>,
    meters: Arc<Meters>,
) {
    let mut chat = ToChat::new(&tuning.borrow().node.to_chat);
    let mut lost = 0u64;
    while let Some(feed) = feeds.recv().await {
        chat.take(feed);
        let settings = tuning.borrow().node.to_chat.clone();
        while let Some(frame) = chat.next_frame(&settings, &meters) {
            match send.try_send(frame) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    lost += 1;
                    if lost.is_power_of_two() {
                        tracing::warn!(
                            frames = lost,
                            "voice chat dropped audio the chat could not take"
                        );
                    }
                }
                Err(TrySendError::Closed(_)) => return,
            }
        }
    }
}

/// Carries one wired channel into the mix, as mono.
pub(super) async fn forward(
    mut pcm: broadcast::Receiver<PcmBlock>,
    lane: u64,
    feeds: mpsc::Sender<Feed>,
) {
    loop {
        match pcm.recv().await {
            Ok(block) => {
                let mono = mono(&block);
                if feeds.send(Feed::Audio { lane, mono }).await.is_err() {
                    return;
                }
            }
            Err(RecvError::Lagged(blocks)) => {
                tracing::warn!(
                    blocks,
                    "voice chat fell behind the channel and skipped audio"
                );
            }
            Err(RecvError::Closed) => return,
        }
    }
}

pub(super) fn mono(block: &PcmBlock) -> Vec<f32> {
    match &block.payload {
        PcmPayload::Silence(frames) => vec![0.0; *frames],
        PcmPayload::Samples(samples) => mono_mix(samples, usize::from(block.channels)),
    }
}
