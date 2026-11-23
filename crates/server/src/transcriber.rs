//! Transcriber nodes: each call or transmission they hear goes on with what was said in it.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    time::Instant,
};

use axum::body::Bytes;
use sdrmm_dsp::audio::speech::to_model_rate;
use sdrmm_engine::Engine;
use sdrmm_wire::{
    DecodedRecord, EventOrigin, NodeBody, PatchGraph, ServerEvent, SpeechModel, TranscriberStatus,
};
use serde_json::Value;
use tokio::sync::{
    broadcast::{self, error::RecvError},
    watch,
};

use crate::{
    AppState, calls::Calls, decoded::Decoded, resolved::Resolved, speech_models::SpeechModels,
};

/// Clips waiting for the model; past this a clip goes on without its text and counts as dropped.
const QUEUE: usize = 32;
const WAV_HEADER: usize = 44;
const MAX_THREADS: usize = 8;

struct Job {
    node: String,
    model: SpeechModel,
    record: DecodedRecord,
    clip: Option<Bytes>,
}

#[derive(Default)]
pub(crate) struct TranscriberHub {
    statuses: Mutex<HashMap<String, TranscriberStatus>>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn announce(engine: &Engine, status: &TranscriberStatus) {
    engine.emit_event(ServerEvent::TranscriberUpdate {
        status: Box::new(status.clone()),
    });
}

impl TranscriberHub {
    pub(crate) fn snapshot(&self) -> Vec<ServerEvent> {
        locked(&self.statuses)
            .values()
            .map(|status| ServerEvent::TranscriberUpdate {
                status: Box::new(status.clone()),
            })
            .collect()
    }

    /// The transcriber nodes in `graph` and the model each runs.
    fn follow(&self, engine: &Engine, graph: &PatchGraph) -> HashMap<String, SpeechModel> {
        let nodes: HashMap<String, SpeechModel> = graph
            .nodes
            .iter()
            .filter_map(|node| match &node.body {
                NodeBody::Transcriber(body) => Some((node.id.clone(), body.settings.model)),
                _ => None,
            })
            .collect();
        let mut statuses = locked(&self.statuses);
        statuses.retain(|node, _| nodes.contains_key(node));
        for (node, &model) in &nodes {
            let status = statuses
                .entry(node.clone())
                .or_insert_with(|| TranscriberStatus {
                    node: node.clone(),
                    ..TranscriberStatus::default()
                });
            if status.readouts.is_empty() || status.model != model {
                status.model = model;
                *status = status.clone().with_readouts();
                announce(engine, status);
            }
        }
        nodes
    }

    fn update(&self, engine: &Engine, node: &str, change: impl FnOnce(&mut TranscriberStatus)) {
        let mut statuses = locked(&self.statuses);
        if let Some(status) = statuses.get_mut(node) {
            change(status);
            *status = status.clone().with_readouts();
            announce(engine, status);
        }
    }
}

pub(crate) fn start(state: &AppState) {
    let (jobs, queue) = mpsc::sync_channel(QUEUE);
    let worker = Worker {
        engine: state.engine.clone(),
        hub: state.transcribers.clone(),
        models: state.speech.clone(),
    };
    let spawned = std::thread::Builder::new()
        .name("sdrmm-transcriber".into())
        .spawn(move || worker.run(&queue));
    if let Err(error) = spawned {
        tracing::error!(%error, "transcriber thread did not start");
        return;
    }
    let feed = Feed {
        engine: state.engine.clone(),
        hub: state.transcribers.clone(),
        calls: state.calls.clone(),
        jobs,
        nodes: HashMap::new(),
    };
    let records = state.decoded.subscribe();
    let changes = state.resolved.subscribe();
    let _detached = crate::spawn_task("sdrmm-transcriber-feed", move || feed.run(records, changes));
}

struct Feed {
    engine: Arc<Engine>,
    hub: Arc<TranscriberHub>,
    calls: Arc<Calls>,
    jobs: SyncSender<Job>,
    nodes: HashMap<String, SpeechModel>,
}

impl Feed {
    async fn run(
        mut self,
        mut records: broadcast::Receiver<Decoded>,
        mut changes: watch::Receiver<Arc<Resolved>>,
    ) {
        self.nodes = self
            .hub
            .follow(&self.engine, &changes.borrow_and_update().graph);
        loop {
            tokio::select! {
                changed = changes.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let resolved = changes.borrow_and_update().clone();
                    self.nodes = self.hub.follow(&self.engine, &resolved.graph);
                }
                record = records.recv() => match record {
                    Ok(Decoded::Record(routed)) => self.hear(&routed.record),
                    Ok(Decoded::Lost(count)) | Err(RecvError::Lagged(count)) => self.lost(count),
                    Err(RecvError::Closed) => break,
                },
            }
        }
    }

    fn hear(&self, record: &DecodedRecord) {
        for sink in &record.sinks {
            let Some(&model) = self.nodes.get(sink) else {
                continue;
            };
            let clip = record
                .view
                .as_ref()
                .map_or_else(|| record.event.present().clip, |view| view.clip.clone())
                .and_then(|audio| self.calls.event_audio(&audio));
            let job = Job {
                node: sink.clone(),
                model,
                record: record.clone(),
                clip,
            };
            self.hub
                .update(&self.engine, sink, |status| status.queued += 1);
            match self.jobs.try_send(job) {
                Ok(()) => {}
                Err(TrySendError::Full(job) | TrySendError::Disconnected(job)) => {
                    self.hub.update(&self.engine, sink, |status| {
                        status.queued = status.queued.saturating_sub(1);
                        status.dropped += 1;
                    });
                    self.engine
                        .publish_decoded(passed_on(&job.node, job.record));
                }
            }
        }
    }

    fn lost(&self, count: u64) {
        for node in self.nodes.keys() {
            self.hub
                .update(&self.engine, node, |status| status.dropped += count);
        }
    }
}

struct Worker {
    engine: Arc<Engine>,
    hub: Arc<TranscriberHub>,
    models: Arc<SpeechModels>,
}

impl Worker {
    fn run(&self, queue: &Receiver<Job>) {
        let threads = std::thread::available_parallelism()
            .map_or(1, |cores| cores.get() / 2)
            .clamp(1, MAX_THREADS);
        for job in queue {
            let started = Instant::now();
            let mut record = job.record;
            let outcome = job
                .clip
                .as_deref()
                .filter(|_| self.models.installed(job.model))
                .map(|wav| self.transcribe(job.model, wav, threads));
            let took_ms = started.elapsed().as_millis() as u64;
            let (done, error) = match outcome {
                Some(Ok(text)) => {
                    with_transcript(&mut record, text);
                    (true, None)
                }
                Some(Err(error)) => (false, Some(error)),
                None => (false, None),
            };
            self.engine.publish_decoded(passed_on(&job.node, record));
            self.hub.update(&self.engine, &job.node, |status| {
                status.queued = status.queued.saturating_sub(1);
                if done {
                    status.done += 1;
                    status.took_ms = Some(took_ms);
                }
                status.error = error;
            });
        }
    }

    fn transcribe(&self, model: SpeechModel, wav: &[u8], threads: usize) -> Result<String, String> {
        let parakeet = self.models.load(model)?;
        let (rate, pcm) = wav_pcm(wav)?;
        let pcm = to_model_rate(&pcm, rate).map_err(|error| error.to_string())?;
        parakeet
            .transcribe(&pcm, threads)
            .map_err(|error| error.to_string())
    }
}

/// The mono 16-bit samples and rate of a clip the call store wrote.
fn wav_pcm(wav: &[u8]) -> Result<(u32, Vec<f32>), String> {
    let rate = wav
        .get(24..28)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| "clip is not a WAV".to_owned())?;
    let data = wav
        .get(WAV_HEADER..)
        .ok_or_else(|| "clip is not a WAV".to_owned())?;
    let pcm = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| f32::from(i16::from_le_bytes(*pair)) / 32_768.0)
        .collect();
    Ok((rate, pcm))
}

/// Clip events all carry a `transcript`, so the text goes in without knowing the event's type.
fn with_transcript(record: &mut DecodedRecord, text: String) {
    if text.is_empty() {
        return;
    }
    let mut data = record.event.data();
    if let Value::Object(fields) = &mut data {
        fields.insert("transcript".to_owned(), Value::String(text));
        record.event.set_data(&data);
    }
}

fn passed_on(node: &str, record: DecodedRecord) -> DecodedRecord {
    DecodedRecord {
        origin: Some(EventOrigin {
            node: node.to_owned(),
            transmission: record
                .origin
                .as_ref()
                .map_or(0, |origin| origin.transmission),
        }),
        sinks: Vec::new(),
        view: None,
        ..record
    }
}

#[cfg(test)]
mod tests;
