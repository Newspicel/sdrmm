//! Running channels and array processors moved onto what a reloaded plugin replaced.

use sdrmm_wire::{ServerEvent, StateScope};

use crate::{
    ChannelAudioRecording, Engine, EngineError, RebuildEntry, channel_ops::Settled,
    runtime::DspCommand,
};

impl Engine {
    /// Tells every client the registry changed, then reopens the channels and array processors
    /// of `replaced`.
    pub fn plugins_reloaded(&self, replaced: &[String]) -> Vec<EngineError> {
        self.emit_scope(StateScope::Decoders);
        let mut errors = self.reopen_decoders(replaced);
        errors.extend(self.reopen_processors(replaced));
        errors
    }

    /// Reopens every running channel of `type_ids` on the decoder the registry holds now, keeping
    /// its settings, node and wires. A channel that will not reopen keeps running on the decoder
    /// it had, and the reason goes out as an error.
    pub fn reopen_decoders(&self, type_ids: &[String]) -> Vec<EngineError> {
        let mut errors = Vec::new();
        for (ds, rebuild) in self.running_of(type_ids) {
            if let Err(error) = self.reopen_channel(ds, rebuild) {
                errors.push(error);
            }
        }
        errors
    }

    fn running_of(&self, type_ids: &[String]) -> Vec<(u32, RebuildEntry)> {
        let inner = self.lock();
        inner
            .device_sets
            .iter()
            .flat_map(|(&ds, state)| {
                state
                    .channels
                    .iter()
                    .filter(|c| type_ids.iter().any(|id| id == c.settings.params.type_id()))
                    .filter_map(move |c| {
                        state.media.get(&c.id).map(|media| {
                            (
                                ds,
                                RebuildEntry {
                                    id: c.id,
                                    stream: c.stream,
                                    settings: c.settings.clone(),
                                    sinks: media.sinks.clone(),
                                },
                            )
                        })
                    })
            })
            .collect()
    }

    fn reopen_channel(&self, ds: u32, rebuild: RebuildEntry) -> Result<(), EngineError> {
        let (id, stream) = (rebuild.id, rebuild.stream);
        let Some(Settled {
            mut inner,
            built,
            settings,
        }) = self.settle_host(ds, rebuild)
        else {
            return Ok(());
        };
        let type_id = settings.params.type_id().to_owned();
        let mut host = match built {
            Ok(host) => host,
            Err(error) => {
                drop(inner);
                self.kept_old_decoder(ds, id, &type_id, &error);
                return Err(error);
            }
        };
        let Some(state) = inner.device_sets.get_mut(&ds) else {
            return Ok(());
        };
        if let Some(media) = state.media.get(&id) {
            host.position_changed(media.position.as_ref());
        }
        state.send_dsp(
            stream,
            DspCommand::AddChannel {
                id,
                host,
                reset_state: false,
            },
        );
        let has_audio = sdrmm_channels::descriptor(&type_id).is_some_and(|d| d.has_audio);
        let orphaned: Vec<ChannelAudioRecording> = if has_audio {
            state.rearm_audio_recording(id, stream);
            Vec::new()
        } else {
            state.take_audio_recordings(id)
        };
        inner.revision += 1;
        drop(inner);
        for recording in orphaned {
            recording.join();
        }
        tracing::info!(ds, channel = id, decoder = %type_id, "channel reopened on the new decoder");
        self.emit(ServerEvent::StateChanged {
            scope: StateScope::DeviceSet(ds),
        });
        Ok(())
    }

    fn kept_old_decoder(&self, ds: u32, id: u32, type_id: &str, error: &EngineError) {
        tracing::error!(ds, channel = id, decoder = %type_id, %error, "channel kept its old decoder");
        self.emit(ServerEvent::Error {
            message: format!("{type_id} reload: channel {id} kept the old decoder: {error}"),
        });
    }
}
