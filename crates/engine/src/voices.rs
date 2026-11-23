//! Voices the server carries into the patch, a microphone or a voice chat, as audio any audio
//! input subscribes to the way it subscribes to a channel.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    thread::JoinHandle,
};

use tokio::sync::broadcast;

use crate::{
    AudioPacket, EngineError, PcmBlock, PcmPayload,
    audio::{AUDIO_CHANNEL_CAP, spawn_encoder},
};

/// Twenty-millisecond frames a listener may fall behind by: a second and a quarter.
const VOICE_QUEUE: usize = 64;

struct VoiceStream {
    pcm_tx: broadcast::Sender<PcmBlock>,
    audio_tx: broadcast::Sender<AudioPacket>,
    next_frame: u64,
    /// Ends on its own once `pcm_tx` drops.
    _encoder: JoinHandle<()>,
}

impl VoiceStream {
    fn open() -> Result<Self, EngineError> {
        let (pcm_tx, pcm_rx) = broadcast::channel(VOICE_QUEUE);
        let (audio_tx, _) = broadcast::channel(AUDIO_CHANNEL_CAP);
        let encoder = spawn_encoder(1, pcm_rx, audio_tx.clone())?;
        Ok(Self {
            pcm_tx,
            audio_tx,
            next_frame: 0,
            _encoder: encoder,
        })
    }
}

/// Every voice a wire listens to, by its node.
#[derive(Default)]
pub(crate) struct VoiceHub {
    streams: HashMap<String, VoiceStream>,
}

impl VoiceHub {
    fn stream(&mut self, node: &str) -> Result<&VoiceStream, EngineError> {
        if !self.streams.contains_key(node) {
            self.streams.insert(node.to_owned(), VoiceStream::open()?);
        }
        self.streams
            .get(node)
            .ok_or_else(|| EngineError::Audio(format!("voice `{node}` vanished")))
    }

    pub(crate) fn subscribe_pcm(
        &mut self,
        node: &str,
    ) -> Result<broadcast::Receiver<PcmBlock>, EngineError> {
        Ok(self.stream(node)?.pcm_tx.subscribe())
    }

    pub(crate) fn subscribe_audio(
        &mut self,
        node: &str,
    ) -> Result<broadcast::Receiver<AudioPacket>, EngineError> {
        Ok(self.stream(node)?.audio_tx.subscribe())
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
        if stream.pcm_tx.send(block).is_err() {
            tracing::debug!(node, "a voice spoke after its encoder stopped");
        }
    }

    /// Closes the voices of nodes no longer in the patch; their listeners see the end.
    pub(crate) fn retain(&mut self, nodes: &HashSet<String>) {
        self.streams.retain(|node, _| nodes.contains(node));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listener_hears_what_the_voice_says_in_order() {
        let mut hub = VoiceHub::default();
        let mut heard = hub.subscribe_pcm("mic").expect("opens");
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
        let mut heard = hub.subscribe_pcm("mic").expect("opens");
        hub.retain(&HashSet::new());
        assert!(matches!(
            heard.try_recv(),
            Err(broadcast::error::TryRecvError::Closed)
        ));
    }

    #[test]
    fn a_voice_reaches_the_browser_as_opus() {
        let mut hub = VoiceHub::default();
        let mut packets = hub.subscribe_audio("chat").expect("opens");
        hub.speak("chat", &[0.1; 960]);
        let packet = packets.blocking_recv().expect("one opus frame");
        assert_eq!((packet.channels, packet.timestamp), (1, 0));
    }
}
