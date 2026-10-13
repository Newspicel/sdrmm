use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use sdrmm_wire::{
    Capability, DecodedRecord, Entity, EntityKey, LatLon, Lifetime, geo::distance_m,
    present::Presentation,
};

pub(crate) const MAX_ENTITIES: usize = 1_000;
pub(crate) const TRAIL_POINTS: usize = 64;
pub(crate) const TRAIL_STEP_M: f64 = 10.0;

struct Held {
    entity: Entity,
    seen: Instant,
}

#[derive(Default)]
pub(crate) struct Entities {
    held: HashMap<EntityKey, Held>,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Change {
    pub upserted: Option<EntityKey>,
    pub removed: Vec<EntityKey>,
    pub evicted: u64,
}

pub(crate) fn key_of(record: &DecodedRecord, view: &Presentation) -> Option<EntityKey> {
    let kind = record.event.kind();
    if !kind.has(Capability::Station) {
        return None;
    }
    Some(EntityKey {
        source: record.origin.as_ref()?.node.clone(),
        kind,
        id: view.entity.clone()?,
    })
}

pub(crate) fn push_trail(trail: &mut Vec<LatLon>, at: LatLon) {
    if trail
        .last()
        .is_some_and(|last| distance_m(*last, at) <= TRAIL_STEP_M)
    {
        return;
    }
    trail.push(at);
    if trail.len() > TRAIL_POINTS {
        trail.remove(0);
    }
}

fn fresh(key: EntityKey, record: &DecodedRecord, view: Presentation) -> Entity {
    let info = key.kind.info();
    Entity {
        symbol: info.symbol,
        hue: info.hue,
        event: record.event.clone(),
        view,
        trail: Vec::new(),
        freq_hz: record.freq_hz,
        first_seen: record.at.clone(),
        last_seen: record.at.clone(),
        frames: 0,
        key,
    }
}

fn merged(entity: &mut Entity, record: &DecodedRecord, view: &Presentation) {
    if entity.event.merge(&record.event) {
        entity.view = entity.event.present();
    } else {
        entity.event = record.event.clone();
        entity.view = view.clone();
    }
    entity.view.clip = view.clip.clone();
}

impl Entities {
    pub(crate) fn observe(
        &mut self,
        record: &DecodedRecord,
        view: &Presentation,
        now: Instant,
    ) -> Change {
        let Some(key) = key_of(record, view) else {
            return Change::default();
        };
        if view.gone {
            return Change {
                removed: self.held.remove(&key).map(|_| key).into_iter().collect(),
                ..Change::default()
            };
        }
        let held = self.held.entry(key.clone()).or_insert_with(|| Held {
            entity: fresh(key.clone(), record, view.clone()),
            seen: now,
        });
        let entity = &mut held.entity;
        if entity.frames > 0 {
            merged(entity, record, view);
        }
        entity.frames = entity.frames.saturating_add(1);
        entity.freq_hz = record.freq_hz;
        entity.last_seen.clone_from(&record.at);
        if key.kind.has(Capability::Geo)
            && let Some(place) = &entity.view.place
        {
            push_trail(&mut entity.trail, place.at);
        }
        held.seen = now;
        let removed = self.evict();
        Change {
            upserted: Some(key),
            evicted: removed.len() as u64,
            removed,
        }
    }

    fn evict(&mut self) -> Vec<EntityKey> {
        let excess = self.held.len().saturating_sub(MAX_ENTITIES);
        if excess == 0 {
            return Vec::new();
        }
        let mut by_age: Vec<(Instant, EntityKey)> = self
            .held
            .iter()
            .map(|(key, held)| (held.seen, key.clone()))
            .collect();
        by_age.sort_unstable_by_key(|(seen, _)| *seen);
        by_age
            .into_iter()
            .take(excess)
            .filter_map(|(_, key)| self.held.remove(&key).map(|_| key))
            .collect()
    }

    pub(crate) fn age_out(&mut self, now: Instant) -> Vec<EntityKey> {
        self.remove_where(|held| match held.entity.key.kind.info().lifetime {
            Lifetime::Sighting { ttl_s } => {
                now.duration_since(held.seen) > Duration::from_secs(u64::from(ttl_s))
            }
            Lifetime::Instant | Lifetime::Standing => false,
        })
    }

    pub(crate) fn retain(&mut self, keep: impl Fn(&Entity) -> bool) -> Vec<EntityKey> {
        self.remove_where(|held| !keep(&held.entity))
    }

    fn remove_where(&mut self, drop: impl Fn(&Held) -> bool) -> Vec<EntityKey> {
        let gone: Vec<EntityKey> = self
            .held
            .iter()
            .filter(|(_, held)| drop(held))
            .map(|(key, _)| key.clone())
            .collect();
        for key in &gone {
            self.held.remove(key);
        }
        gone
    }

    pub(crate) fn get(&self, key: &EntityKey) -> Option<&Entity> {
        self.held.get(key).map(|held| &held.entity)
    }

    pub(crate) fn all(&self) -> Vec<Entity> {
        let mut all: Vec<Entity> = self.held.values().map(|held| held.entity.clone()).collect();
        all.sort_by(|a, b| a.key.cmp(&b.key));
        all
    }
}

#[cfg(test)]
mod tests;
