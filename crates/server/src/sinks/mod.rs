use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

use axum::extract::ws::Utf8Bytes;
use sdrmm_wire::{
    CapturedImage, DecodedRecord, DecoderLogQuery, EVENTS_PORT, EntityKey, Keeps, NodeBody,
    NodeCategory, PortType, PositionFix, PositionTrack, PropagationView, ResolvedNode, ServerEvent,
    SinkFeed, SinkUpdate, StreamKind, SurfaceRef,
};
use tokio::{
    sync::{
        broadcast::{self, error::RecvError},
        watch,
    },
    time::{MissedTickBehavior, interval, timeout},
};

use crate::{AppState, decoded::Decoded, resolved::Resolved, surfaces::SurfaceHub};

mod entities;
mod propagation;

use entities::{Entities, push_trail};
use propagation::{Heard, Propagation, heard_of};

pub(crate) const RECORDS_PER_SINK: usize = 500;
pub(crate) const PICTURES_PER_SINK: usize = 64;
pub(crate) const UPDATE_EVERY: Duration = Duration::from_millis(500);
pub(crate) const AGE_OUT_EVERY: Duration = Duration::from_secs(15);
pub(crate) const PROPAGATION_EVERY: Duration = Duration::from_secs(2);
pub(crate) const PROPAGATION_HISTORY: Duration = Duration::from_secs(6 * 60 * 60);
const SINK_BACKLOG: usize = 16;
pub(crate) const SINK_WAIT: Duration = Duration::from_secs(10);

pub(crate) type Watched = (Utf8Bytes, broadcast::Receiver<Utf8Bytes>);

enum Lookup {
    Sink(Watched),
    Other,
    Unknown,
}

#[derive(Default)]
struct Pending {
    upserts: BTreeSet<EntityKey>,
    removed: BTreeSet<EntityKey>,
    records: VecDeque<DecodedRecord>,
    pictures: bool,
    tracks: bool,
    surfaces: bool,
    lost: bool,
}

struct Sink {
    entities: Entities,
    records: VecDeque<DecodedRecord>,
    pictures: VecDeque<CapturedImage>,
    takes_pictures: bool,
    tracks: BTreeMap<String, PositionTrack>,
    surfaces: Vec<SurfaceRef>,
    lost: u64,
    keeps_clips: bool,
    positions_from: HashSet<String>,
    events_from: HashSet<String>,
    propagation: Option<Propagation>,
    propagation_at: Option<Instant>,
    pending: Pending,
    out: broadcast::Sender<Utf8Bytes>,
}

fn now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

impl Sink {
    fn new() -> Self {
        Self {
            entities: Entities::default(),
            records: VecDeque::new(),
            pictures: VecDeque::new(),
            takes_pictures: false,
            tracks: BTreeMap::new(),
            surfaces: Vec::new(),
            lost: 0,
            keeps_clips: false,
            positions_from: HashSet::new(),
            events_from: HashSet::new(),
            propagation: None,
            propagation_at: None,
            pending: Pending::default(),
            out: broadcast::channel(SINK_BACKLOG).0,
        }
    }

    fn feed(&self) -> SinkFeed {
        SinkFeed {
            entities: self.entities.all(),
            records: self.records.iter().cloned().collect(),
            tracks: self.tracks.values().cloned().collect(),
            surfaces: self.surfaces.clone(),
            pictures: self.pictures.iter().cloned().collect(),
            propagation: self.propagation.as_ref().map(|model| model.view(now_ms())),
            lost: self.lost,
        }
    }

    fn propagation_due(&mut self, now: Instant) -> Option<PropagationView> {
        let model = self.propagation.as_mut()?;
        let quiet = self
            .propagation_at
            .is_some_and(|at| now.duration_since(at) < PROPAGATION_EVERY);
        if quiet || !model.changed && !model.holds_any() {
            return None;
        }
        model.changed = false;
        self.propagation_at = Some(now);
        Some(model.view(now_ms()))
    }

    fn take_update(&mut self, now: Instant) -> Option<SinkUpdate> {
        let propagation = self.propagation_due(now);
        let pending = std::mem::take(&mut self.pending);
        let update = SinkUpdate {
            upserts: pending
                .upserts
                .iter()
                .filter(|key| !pending.removed.contains(key))
                .filter_map(|key| self.entities.get(key).cloned())
                .collect(),
            removed: pending.removed.into_iter().collect(),
            records: pending.records.into(),
            tracks: pending
                .tracks
                .then(|| self.tracks.values().cloned().collect()),
            surfaces: pending.surfaces.then(|| self.surfaces.clone()),
            pictures: pending
                .pictures
                .then(|| self.pictures.iter().cloned().collect()),
            propagation,
            lost: self.lost,
        };
        (!update.is_empty() || pending.lost).then_some(update)
    }

    fn take(&mut self, record: DecodedRecord, now: Instant) -> u64 {
        let mut record = record;
        if !self.keeps_clips {
            strip_clip(&mut record);
        }
        let Some(view) = record.view.clone() else {
            return 0;
        };
        if let (Some(model), Some(spot)) = (self.propagation.as_mut(), view.spot.as_ref())
            && let Some(heard) = heard_of(&record, spot)
        {
            model.hear(heard);
        }
        let change = self.entities.observe(&record, &view, now);
        if let Some(key) = change.upserted {
            self.pending.removed.remove(&key);
            self.pending.upserts.insert(key);
        }
        self.pending.removed.extend(change.removed);
        if self.records.len() == RECORDS_PER_SINK {
            self.records.pop_front();
        }
        self.records.push_back(record.clone());
        if self.pending.records.len() == RECORDS_PER_SINK {
            self.pending.records.pop_front();
        }
        self.pending.records.push_back(record);
        self.count_lost(change.evicted)
    }

    fn show(&mut self, image: &CapturedImage) {
        if self.pictures.len() == PICTURES_PER_SINK {
            self.pictures.pop_front();
        }
        self.pictures.push_back(image.clone());
        self.pending.pictures = true;
    }

    fn hold_pictures(&mut self, mut held: Vec<CapturedImage>) {
        held.truncate(PICTURES_PER_SINK);
        held.reverse();
        let held: VecDeque<CapturedImage> = held.into();
        if held != self.pictures {
            self.pictures = held;
            self.pending.pictures = true;
        }
    }

    fn count_lost(&mut self, count: u64) -> u64 {
        if count > 0 {
            self.lost += count;
            self.pending.lost = true;
        }
        count
    }

    fn position(&mut self, source: &str, fix: Option<&PositionFix>) {
        if !self.positions_from.contains(source) {
            return;
        }
        let track = self
            .tracks
            .entry(source.to_owned())
            .or_insert_with(|| PositionTrack {
                source: source.to_owned(),
                fix: None,
                trail: Vec::new(),
            });
        track.fix = fix.cloned();
        if let Some(fix) = fix {
            push_trail(&mut track.trail, fix.at());
        }
        locate(self);
        self.pending.tracks = true;
    }
}

fn strip_clip(record: &mut DecodedRecord) {
    record.event.strip_clip();
    if let Some(view) = &mut record.view {
        view.clip = None;
    }
}

fn sources_on(node: &ResolvedNode, port_type: PortType) -> HashSet<String> {
    node.inputs()
        .filter(|port| port.spec.port_type == port_type)
        .flat_map(|port| &port.feeds)
        .map(|feed| feed.source.clone())
        .collect()
}

fn keeps(node: &ResolvedNode, keeps: Keeps) -> bool {
    node.inputs().any(|port| port.spec.keeps.contains(&keeps))
}

fn takes_pictures(node: &ResolvedNode) -> bool {
    keeps(node, Keeps::Pictures)
}

fn is_sink(node: &ResolvedNode) -> bool {
    node.kind.category() == NodeCategory::Output
        && (takes_pictures(node)
            || node
                .inputs()
                .any(|port| port.spec.port_type == PortType::Events))
}

pub(crate) struct SinkHub {
    sinks: Mutex<HashMap<String, Sink>>,
    others: Mutex<HashSet<String>>,
    rewired: watch::Sender<u64>,
}

impl Default for SinkHub {
    fn default() -> Self {
        Self {
            sinks: Mutex::default(),
            others: Mutex::default(),
            rewired: watch::Sender::new(0),
        }
    }
}

impl SinkHub {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Sink>> {
        self.sinks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn others(&self) -> MutexGuard<'_, HashSet<String>> {
        self.others.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn rewire(
        &self,
        resolved: &Resolved,
        fix: impl Fn(&str) -> Option<PositionFix>,
    ) -> Vec<String> {
        let mut sinks = self.lock();
        let wanted: Vec<&ResolvedNode> = resolved
            .patch
            .nodes
            .iter()
            .filter(|node| is_sink(node))
            .collect();
        sinks.retain(|id, _| wanted.iter().any(|node| &node.id == id));
        let mut seeded = Vec::new();
        for node in wanted {
            let sink = sinks.entry(node.id.clone()).or_insert_with(Sink::new);
            sink.takes_pictures = takes_pictures(node);
            sink.keeps_clips = keeps(node, Keeps::Clips);
            sink.events_from = sources_on(node, PortType::Events);
            sink.positions_from = sources_on(node, PortType::Position);
            if follow_settings(sink, resolved, &node.id) {
                seeded.push(node.id.clone());
            }
            prune(sink, &node.id, resolved);
            let fresh: Vec<String> = sink
                .positions_from
                .iter()
                .filter(|source| !sink.tracks.contains_key(*source))
                .cloned()
                .collect();
            for source in fresh {
                sink.position(&source, fix(&source).as_ref());
            }
            locate(sink);
        }
        drop(sinks);
        *self.others() = resolved
            .patch
            .nodes
            .iter()
            .filter(|node| !is_sink(node))
            .map(|node| node.id.clone())
            .collect();
        self.rewired
            .send_modify(|count| *count = count.wrapping_add(1));
        seeded
    }

    pub(crate) fn seed(&self, node: &str, heard: Vec<Heard>) {
        if let Some(model) = self
            .lock()
            .get_mut(node)
            .and_then(|sink| sink.propagation.as_mut())
        {
            for one in heard {
                model.hear(one);
            }
        }
    }

    pub(crate) fn clear(&self, node: &str) -> bool {
        let mut sinks = self.lock();
        let Some(sink) = sinks.get_mut(node) else {
            return false;
        };
        sink.entities.retain(|_| false);
        sink.records.clear();
        sink.pending = Pending::default();
        if let Some(model) = sink.propagation.as_mut() {
            model.clear(now_ms());
            model.changed = false;
        }
        let snapshot = ServerEvent::SinkSnapshot {
            node: node.to_owned(),
            feed: Box::new(sink.feed()),
        };
        let _unwatched = sink.out.send(crate::ws::encode_event(&snapshot));
        true
    }

    pub(crate) fn deliver(&self, record: &DecodedRecord, now: Instant) -> Vec<(String, u64)> {
        let mut sinks = self.lock();
        record
            .sinks
            .iter()
            .filter_map(|id| {
                let sink = sinks.get_mut(id)?;
                Some((id.clone(), sink.take(record.clone(), now)))
            })
            .filter(|(_, lost)| *lost > 0)
            .collect()
    }

    pub(crate) fn lost(&self, count: u64) -> Vec<String> {
        self.lock()
            .iter_mut()
            .filter(|(_, sink)| !sink.events_from.is_empty())
            .map(|(id, sink)| {
                sink.count_lost(count);
                id.clone()
            })
            .collect()
    }

    pub(crate) fn picture(&self, image: &CapturedImage) {
        let mut sinks = self.lock();
        for id in &image.sinks {
            if let Some(sink) = sinks.get_mut(id) {
                sink.show(image);
            }
        }
    }

    pub(crate) fn seed_pictures(&self, stored: impl Fn(&str) -> Vec<CapturedImage>) {
        for (node, sink) in self.lock().iter_mut() {
            if sink.takes_pictures && sink.pictures.is_empty() {
                sink.hold_pictures(stored(node));
            }
        }
    }

    pub(crate) fn reload_pictures(&self, stored: impl Fn(&str) -> Vec<CapturedImage>) {
        for (node, sink) in self.lock().iter_mut() {
            if sink.takes_pictures {
                sink.hold_pictures(stored(node));
            }
        }
    }

    pub(crate) fn position(&self, source: &str, fix: Option<&PositionFix>) {
        for sink in self.lock().values_mut() {
            sink.position(source, fix);
        }
    }

    pub(crate) fn age_out(&self, now: Instant) {
        for sink in self.lock().values_mut() {
            let gone = sink.entities.age_out(now);
            sink.pending.removed.extend(gone);
        }
    }

    pub(crate) fn flush(&self, surfaces: &SurfaceHub) {
        let now = Instant::now();
        let mut sinks = self.lock();
        for (node, sink) in sinks.iter_mut() {
            let geo: Vec<SurfaceRef> = sink
                .events_from
                .iter()
                .filter(|source| surfaces.kind_of(source) == Some(StreamKind::FusionGrid))
                .map(|source| SurfaceRef {
                    node: source.clone(),
                    kind: StreamKind::FusionGrid,
                })
                .collect();
            let geo = sorted(geo);
            if geo != sink.surfaces {
                sink.surfaces = geo;
                sink.pending.surfaces = true;
            }
            if let Some(update) = sink.take_update(now) {
                let event = ServerEvent::SinkUpdate {
                    node: node.clone(),
                    update: Box::new(update),
                };
                let _unwatched = sink.out.send(crate::ws::encode_event(&event));
            }
        }
    }

    fn lookup(&self, node: &str) -> Lookup {
        if let Some(sink) = self.lock().get(node) {
            let updates = sink.out.subscribe();
            let snapshot = ServerEvent::SinkSnapshot {
                node: node.to_owned(),
                feed: Box::new(sink.feed()),
            };
            return Lookup::Sink((crate::ws::encode_event(&snapshot), updates));
        }
        if self.others().contains(node) {
            Lookup::Other
        } else {
            Lookup::Unknown
        }
    }

    pub(crate) async fn watch(&self, node: &str) -> Option<Watched> {
        let mut rewired = self.rewired.subscribe();
        let found = async {
            loop {
                match self.lookup(node) {
                    Lookup::Sink(watched) => return Some(watched),
                    Lookup::Other => return None,
                    Lookup::Unknown => rewired.changed().await.ok()?,
                }
            }
        };
        timeout(SINK_WAIT, found).await.ok().flatten()
    }

    pub(crate) fn snapshot(&self, node: &str) -> Option<Utf8Bytes> {
        let sinks = self.lock();
        let snapshot = ServerEvent::SinkSnapshot {
            node: node.to_owned(),
            feed: Box::new(sinks.get(node)?.feed()),
        };
        Some(crate::ws::encode_event(&snapshot))
    }

    #[cfg(test)]
    pub(crate) fn subscribe(&self, node: &str) -> Option<Watched> {
        match self.lookup(node) {
            Lookup::Sink(watched) => Some(watched),
            Lookup::Other | Lookup::Unknown => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn feed(&self, node: &str) -> Option<SinkFeed> {
        self.lock().get(node).map(Sink::feed)
    }
}

fn follow_settings(sink: &mut Sink, resolved: &Resolved, node: &str) -> bool {
    let settings = match resolved.graph.node(node).map(|found| &found.body) {
        Some(NodeBody::Propagation(settings)) => *settings,
        _ => {
            sink.propagation = None;
            return false;
        }
    };
    let fresh = sink.propagation.is_none();
    let model = sink.propagation.get_or_insert_with(Propagation::default);
    if model.settings != settings {
        model.settings = settings;
        model.changed = true;
    }
    fresh
}

fn locate(sink: &mut Sink) {
    let receiver = sink
        .positions_from
        .iter()
        .min()
        .and_then(|source| sink.tracks.get(source))
        .and_then(|track| track.fix.as_ref())
        .map(PositionFix::at);
    if let Some(model) = sink.propagation.as_mut()
        && model.receiver != receiver
    {
        model.receiver = receiver;
        model.changed = true;
    }
}

fn sorted(mut surfaces: Vec<SurfaceRef>) -> Vec<SurfaceRef> {
    surfaces.sort_by(|a, b| a.node.cmp(&b.node));
    surfaces
}

fn prune(sink: &mut Sink, node: &str, resolved: &Resolved) {
    let routes = &resolved.events;
    let entities = &sink.entities;
    sink.records.retain(|record| {
        let source = record.origin.as_ref().map(|origin| origin.node.as_str());
        match (source, &record.view) {
            (Some(source), Some(view)) => {
                let judged = entities::key_of(record, view)
                    .and_then(|key| entities.get(&key))
                    .map_or(view, |entity| &entity.view);
                routes.admits(source, node, record.event.kind(), judged)
            }
            _ => false,
        }
    });
    let removed = sink
        .entities
        .retain(|entity| routes.admits(&entity.key.source, node, entity.key.kind, &entity.view));
    sink.pending.removed.extend(removed);
    if !sink.keeps_clips {
        sink.records.iter_mut().for_each(strip_clip);
    }
    let positions_from = &sink.positions_from;
    let before = sink.tracks.len();
    sink.tracks
        .retain(|source, _| positions_from.contains(source));
    if sink.tracks.len() != before {
        sink.pending.tracks = true;
    }
}

pub(crate) fn start(state: &AppState) {
    let records = state.decoded.subscribe();
    let positions = state.gps.subscribe();
    let followed = state.clone();
    let _detached = crate::spawn_task("sdrmm-sinks", move || run(followed, records, positions));
}

async fn run(
    state: AppState,
    mut records: broadcast::Receiver<Decoded>,
    mut positions: broadcast::Receiver<ServerEvent>,
) {
    let mut resolved = state.resolved.subscribe();
    let mut flush_tick = interval(UPDATE_EVERY);
    flush_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut age_tick = interval(AGE_OUT_EVERY);
    age_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    rewire(&state, &resolved.borrow_and_update().clone());
    loop {
        tokio::select! {
            changed = resolved.changed() => {
                if changed.is_err() {
                    return;
                }
                let current = resolved.borrow_and_update().clone();
                rewire(&state, &current);
            }
            record = records.recv() => match record {
                Ok(Decoded::Record(routed)) => {
                    for (sink, lost) in state.sinks.deliver(&routed.record, Instant::now()) {
                        state.activity.lost(&sink, EVENTS_PORT, lost);
                    }
                }
                Ok(Decoded::Lost(count)) | Err(RecvError::Lagged(count)) => {
                    tracing::warn!(count, "sink feeds missed decoded records");
                    for sink in state.sinks.lost(count) {
                        state.activity.lost(&sink, EVENTS_PORT, count);
                    }
                }
                Err(RecvError::Closed) => return,
            },
            position = positions.recv() => match position {
                Ok(ServerEvent::PositionChanged { node, fix, .. }) => {
                    state.sinks.position(&node, fix.as_ref());
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return,
            },
            _ = flush_tick.tick() => state.sinks.flush(&state.surfaces),
            _ = age_tick.tick() => state.sinks.age_out(Instant::now()),
        }
    }
}

fn rewire(state: &AppState, resolved: &Arc<Resolved>) {
    for node in state.sinks.rewire(resolved, |source| state.gps.fix(source)) {
        let state = state.clone();
        tokio::task::spawn_blocking(move || seed(&state, &node));
    }
    state.sinks.seed_pictures(|node| state.images.list(node));
}

fn seed(state: &AppState, node: &str) {
    let since = jiff::Timestamp::now()
        - jiff::SignedDuration::from_secs(PROPAGATION_HISTORY.as_secs() as i64);
    let query = DecoderLogQuery {
        sink: Some(node.to_owned()),
        since: Some(since.to_string()),
        limit: Some(crate::store::DECODER_LOG_LIMIT_MAX),
        ..DecoderLogQuery::default()
    };
    let entries = match state.store.query_decoder_log(&query) {
        Ok((entries, _)) => entries,
        Err(error) => {
            tracing::warn!(%error, node, "propagation history could not be read");
            return;
        }
    };
    let heard = entries
        .into_iter()
        .filter_map(|entry| {
            let view = entry.event.present();
            let record = DecodedRecord {
                origin: entry.origin,
                device_set: entry.device_set,
                channel: entry.channel,
                at: entry.at,
                freq_hz: entry.freq_hz,
                event: entry.event,
                sinks: Vec::new(),
                view: None,
                callsign: None,
            };
            heard_of(&record, view.spot.as_ref()?)
        })
        .collect();
    state.sinks.seed(node, heard);
}

#[cfg(test)]
mod tests;
