use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use sdrmm_wire::{
    Capability, DecodedRecord, DecoderEvent, EventKind, Lifetime, present::Presentation,
};

pub(super) const MAX_STATIONS: usize = 4_096;
const SWEEP_EVERY: Duration = Duration::from_secs(5);

type Key = (String, EventKind, String);

#[derive(Default)]
pub(crate) struct Stations {
    held: HashMap<Key, (DecoderEvent, Instant)>,
    swept: Option<Instant>,
}

impl Stations {
    pub(crate) fn judged(
        &mut self,
        source: &str,
        record: &DecodedRecord,
        view: &Presentation,
        now: Instant,
    ) -> Presentation {
        let kind = record.event.kind();
        let Some(entity) = view
            .entity
            .clone()
            .filter(|_| kind.has(Capability::Station))
        else {
            return view.clone();
        };
        let key = (source.to_owned(), kind, entity);
        if view.gone {
            self.held.remove(&key);
            return view.clone();
        }
        self.age_out(now);
        if !self.held.contains_key(&key) && self.held.len() >= MAX_STATIONS {
            self.evict_oldest();
        }
        let (event, seen) = self
            .held
            .entry(key)
            .or_insert_with(|| (record.event.clone(), now));
        if !event.merge(&record.event) {
            *event = record.event.clone();
        }
        *seen = now;
        let mut merged = event.present();
        merged.gone = false;
        merged
    }

    fn age_out(&mut self, now: Instant) {
        if self
            .swept
            .is_some_and(|at| now.duration_since(at) < SWEEP_EVERY)
        {
            return;
        }
        self.swept = Some(now);
        self.held
            .retain(|(_, kind, _), (_, seen)| match kind.info().lifetime {
                Lifetime::Sighting { ttl_s } => {
                    now.duration_since(*seen) <= Duration::from_secs(u64::from(ttl_s))
                }
                Lifetime::Instant | Lifetime::Standing => true,
            });
    }

    fn evict_oldest(&mut self) {
        let oldest = self
            .held
            .iter()
            .min_by_key(|(_, (_, seen))| *seen)
            .map(|(key, _)| key.clone());
        if let Some(key) = oldest {
            self.held.remove(&key);
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.held.len()
    }
}
