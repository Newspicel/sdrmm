use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use sdrmm_wire::{
    decode::{DecodedRecord, DecoderEvent},
    position::PositionFix,
    processor::ProcessorReading,
    radar::RadarUpdate,
    satellite::SatelliteStatus,
    ws::ServerEvent,
};
use zgui::prelude::*;
use zgui::reactive::ArcRwSignal;

use super::{merge, millis_of, now_ms, tracks::Trail};
use crate::{store::Store, ui::map::Geo};

pub const POSITION_HISTORY: usize = 5_000;
pub const STATION_CAPACITY: usize = 1_000;
pub const BEARING_HISTORY: usize = 64;
pub const BEARING_MAX_AGE_MS: i64 = 300_000;
pub const FIX_TRAIL_POINTS: usize = 16;
pub const MAP_KINDS: [&str; 6] = ["adsb", "ais", "aprs", "radiosonde", "lora", "remote_id"];

#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub at: Geo,
    pub altitude_m: Option<f64>,
    pub accuracy_m: Option<f64>,
    pub received_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub fix: Option<PositionFix>,
    pub error: Option<String>,
    pub history: Vec<Sample>,
}

pub fn append(history: &mut Vec<Sample>, sample: Sample) {
    if let Some(last) = history.last_mut()
        && last.at == sample.at
        && last.altitude_m == sample.altitude_m
    {
        *last = sample;
        return;
    }
    history.push(sample);
    let overflow = history.len().saturating_sub(POSITION_HISTORY);
    history.drain(..overflow);
}

#[derive(Clone, Debug, PartialEq)]
pub struct Station {
    pub kind: &'static str,
    pub id: String,
    pub event: DecoderEvent,
    pub last_seen: i64,
    pub freq_hz: f64,
    pub frames: u32,
}

#[must_use]
pub fn station_id(event: &DecoderEvent) -> Option<String> {
    match event {
        DecoderEvent::Adsb(message) => Some(message.icao.clone()),
        DecoderEvent::Ais(message) => Some(message.mmsi.to_string()),
        DecoderEvent::Aprs(packet) => Some(packet.source.clone()),
        DecoderEvent::Radiosonde(frame) => Some(frame.serial.clone()),
        DecoderEvent::Lora(frame) => frame.station(),
        DecoderEvent::RemoteId(frame) => Some(frame.station()),
        _ => None,
    }
}

#[must_use]
pub fn event_position(event: &DecoderEvent) -> Option<Geo> {
    let (lat, lon) = event.position()?;
    Geo::checked(Some(lat), Some(lon))
}

#[must_use]
pub fn merge_forward(previous: &DecoderEvent, next: &DecoderEvent) -> DecoderEvent {
    match (previous, next) {
        (DecoderEvent::Lora(held), DecoderEvent::Lora(fresh)) => {
            return DecoderEvent::Lora(merge::lora(held, fresh));
        }
        (DecoderEvent::RemoteId(held), DecoderEvent::RemoteId(fresh)) => {
            return DecoderEvent::RemoteId(merge::remote_id(held, fresh));
        }
        _ => {}
    }
    let (Ok(mut held), Ok(fresh)) = (serde_json::to_value(previous), serde_json::to_value(next))
    else {
        return next.clone();
    };
    if previous.kind() != next.kind() {
        return next.clone();
    }
    if let (Some(held_data), Some(fresh_data)) = (
        held.get_mut("data")
            .and_then(serde_json::Value::as_object_mut),
        fresh.get("data").and_then(serde_json::Value::as_object),
    ) {
        for (key, value) in fresh_data {
            if !value.is_null() {
                held_data.insert(key.clone(), value.clone());
            }
        }
    }
    serde_json::from_value(held).unwrap_or_else(|_| next.clone())
}

#[derive(Clone, Debug, PartialEq)]
pub struct BearingSample {
    pub node: String,
    pub true_deg: f64,
    pub confidence: f64,
    pub sigma_deg: f64,
    pub from: Geo,
    pub at: i64,
    pub station_id: Option<String>,
}

#[must_use]
pub fn bearing_of(record: &DecodedRecord, now: i64) -> Option<BearingSample> {
    let DecoderEvent::Df(bearing) = &record.event else {
        return None;
    };
    let node = record
        .origin
        .as_ref()
        .map_or(bearing.node.as_str(), |origin| origin.node.as_str());
    let from = Geo::checked(bearing.lat, bearing.lon)?;
    (!node.is_empty()).then(|| BearingSample {
        node: node.to_owned(),
        true_deg: f64::from(bearing.bearing_deg),
        confidence: f64::from(bearing.confidence),
        sigma_deg: f64::from(bearing.sigma_deg),
        from,
        at: millis_of(&record.at).unwrap_or(now),
        station_id: bearing.station_id.clone(),
    })
}

#[must_use]
pub fn keep_recent(mut samples: Vec<BearingSample>, now: i64) -> Vec<BearingSample> {
    samples.retain(|sample| sample.at >= now - BEARING_MAX_AGE_MS);
    samples.sort_by_key(|sample| sample.at);
    let overflow = samples.len().saturating_sub(BEARING_HISTORY);
    samples.drain(..overflow);
    samples
}

fn same_sample(a: &BearingSample, b: &BearingSample) -> bool {
    a.at == b.at && a.true_deg == b.true_deg && a.from == b.from && a.station_id == b.station_id
}

pub type FixTrails = HashMap<u32, Vec<Geo>>;

#[must_use]
pub fn advance_trails(held: Option<&FixTrails>, update: &RadarUpdate) -> FixTrails {
    update
        .tracks
        .iter()
        .filter_map(|track| {
            let fix = track.fix.as_ref()?;
            let at = Geo::new(fix.lat, fix.lon);
            let mut trail = held
                .and_then(|trails| trails.get(&track.id))
                .cloned()
                .unwrap_or_default();
            if trail.last() != Some(&at) {
                trail.push(at);
                let overflow = trail.len().saturating_sub(FIX_TRAIL_POINTS);
                trail.drain(..overflow);
            }
            Some((track.id, trail))
        })
        .collect()
}

#[derive(Default)]
pub struct Held {
    pub tracks: HashMap<String, Track>,
    pub stations: HashMap<&'static str, HashMap<String, Station>>,
    pub trails: HashMap<&'static str, HashMap<String, Trail>>,
    pub bearings: HashMap<String, Vec<BearingSample>>,
    pub fix_trails: HashMap<String, FixTrails>,
    pub satellites: HashMap<String, SatelliteStatus>,
}

impl Held {
    pub fn position(
        &mut self,
        node: &str,
        fix: Option<PositionFix>,
        error: Option<String>,
        now: i64,
    ) {
        let track = self.tracks.entry(node.to_owned()).or_default();
        if let Some(fix) = &fix {
            append(
                &mut track.history,
                Sample {
                    at: Geo::new(fix.latitude, fix.longitude),
                    altitude_m: fix.altitude_m,
                    accuracy_m: fix.accuracy_m,
                    received_at: now,
                },
            );
        }
        track.fix = fix;
        track.error = error;
    }

    pub fn decoded(&mut self, record: &DecodedRecord) -> bool {
        let Some(id) = station_id(&record.event) else {
            return false;
        };
        let kind = record.event.kind();
        let last_seen = millis_of(&record.at).unwrap_or_else(now_ms);
        if let Some(at) = event_position(&record.event) {
            self.trails
                .entry(kind)
                .or_default()
                .entry(id.clone())
                .or_default()
                .extend(at, last_seen);
        }
        let stations = self.stations.entry(kind).or_default();
        let previous = stations.get(&id);
        let station = Station {
            kind,
            id: id.clone(),
            event: previous.map_or_else(
                || record.event.clone(),
                |held| merge_forward(&held.event, &record.event),
            ),
            last_seen,
            freq_hz: record.freq_hz,
            frames: previous.map_or(0, |held| held.frames) + 1,
        };
        stations.insert(id, station);
        if stations.len() > STATION_CAPACITY {
            let mut ages: Vec<(i64, String)> = stations
                .values()
                .map(|held| (held.last_seen, held.id.clone()))
                .collect();
            ages.sort();
            for (_, id) in ages.into_iter().take(stations.len() - STATION_CAPACITY) {
                stations.remove(&id);
            }
        }
        self.forget_trails(kind);
        true
    }

    fn forget_trails(&mut self, kind: &'static str) {
        let (Some(trails), Some(stations)) = (self.trails.get_mut(kind), self.stations.get(kind))
        else {
            return;
        };
        if trails.len() > stations.len() {
            trails.retain(|id, _| stations.contains_key(id));
        }
    }

    pub fn age_out(&mut self, max_age_ms: i64, now: i64) {
        for stations in self.stations.values_mut() {
            stations.retain(|_, station| station.last_seen >= now - max_age_ms);
        }
        let kinds: Vec<&'static str> = self.trails.keys().copied().collect();
        for kind in kinds {
            self.forget_trails(kind);
        }
    }

    pub fn bearings<'a>(
        &mut self,
        records: impl IntoIterator<Item = &'a DecodedRecord>,
        now: i64,
    ) -> bool {
        let mut added: HashMap<String, Vec<BearingSample>> = HashMap::new();
        for sample in records
            .into_iter()
            .filter_map(|record| bearing_of(record, now))
        {
            let held = self.bearings.get(&sample.node);
            let list = added.entry(sample.node.clone()).or_default();
            let seen = held
                .into_iter()
                .flatten()
                .chain(list.iter())
                .any(|kept| same_sample(kept, &sample));
            if !seen {
                list.push(sample);
            }
        }
        added.retain(|_, list| !list.is_empty());
        let changed = !added.is_empty();
        for (node, samples) in added {
            let mut merged = self.bearings.remove(&node).unwrap_or_default();
            merged.extend(samples);
            self.bearings.insert(node, keep_recent(merged, now));
        }
        changed
    }

    pub fn radar(&mut self, node: &str, update: &RadarUpdate) {
        let next = advance_trails(self.fix_trails.get(node), update);
        self.fix_trails.insert(node.to_owned(), next);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Topic {
    Positions,
    Stations,
    Df,
    Satellites,
}

pub struct Feed {
    pub held: RefCell<Held>,
    positions: ArcRwSignal<u64>,
    stations: ArcRwSignal<u64>,
    df: ArcRwSignal<u64>,
    satellites: ArcRwSignal<u64>,
    holders: RefCell<Vec<u64>>,
    next: Cell<u64>,
    seeded: Cell<bool>,
}

thread_local! {
    static FEED: Rc<Feed> = Rc::new(Feed {
        held: RefCell::new(Held::default()),
        positions: ArcRwSignal::new(0),
        stations: ArcRwSignal::new(0),
        df: ArcRwSignal::new(0),
        satellites: ArcRwSignal::new(0),
        holders: RefCell::new(Vec::new()),
        next: Cell::new(0),
        seeded: Cell::new(false),
    });
}

impl Feed {
    pub fn get() -> Rc<Self> {
        FEED.with(Rc::clone)
    }

    fn signal(&self, topic: Topic) -> &ArcRwSignal<u64> {
        match topic {
            Topic::Positions => &self.positions,
            Topic::Stations => &self.stations,
            Topic::Df => &self.df,
            Topic::Satellites => &self.satellites,
        }
    }

    pub fn track(&self, topic: Topic) {
        self.signal(topic).with(|_| ());
    }

    fn bump(&self, topic: Topic) {
        self.signal(topic).update(|count| *count += 1);
    }

    pub fn with<T>(&self, topic: Topic, read: impl FnOnce(&Held) -> T) -> T {
        self.track(topic);
        read(&self.held.borrow())
    }

    pub fn change(&self, topic: Topic, edit: impl FnOnce(&mut Held)) {
        edit(&mut self.held.borrow_mut());
        self.bump(topic);
    }

    pub fn listen(self: &Rc<Self>, store: Store) {
        let id = self.next.get() + 1;
        self.next.set(id);
        self.holders.borrow_mut().push(id);
        if !self.seeded.replace(true) {
            let seed = store.decoded.get_untracked();
            self.change(Topic::Stations, |held| {
                for record in seed.all_frames() {
                    held.decoded(record);
                }
            });
            self.change(Topic::Df, |held| {
                held.bearings(seed.all_frames().map(|record| record.as_ref()), now_ms());
            });
        }
        let feed = self.clone();
        on_cleanup_local(move || feed.holders.borrow_mut().retain(|held| *held != id));
        let feed = self.clone();
        store.on_event(move |event| {
            if feed.holders.borrow().first() == Some(&id) {
                feed.absorb(event);
            }
        });
    }

    fn absorb(&self, event: &ServerEvent) {
        let now = now_ms();
        match event {
            ServerEvent::PositionChanged { node, fix, error } => {
                self.change(Topic::Positions, |held| {
                    held.position(node, fix.clone(), error.clone(), now)
                });
            }
            ServerEvent::Decoded(record) => self.records(std::slice::from_ref(&**record), now),
            ServerEvent::DecodedBacklog { records } => self.records(records, now),
            ServerEvent::ProcessorUpdate { node, reading } => {
                if let ProcessorReading::PassiveRadar(update) = &**reading {
                    self.change(Topic::Df, |held| held.radar(node, update));
                }
            }
            ServerEvent::SatelliteUpdate { status } => {
                self.change(Topic::Satellites, |held| {
                    held.satellites
                        .insert(status.node.clone(), (**status).clone());
                });
            }
            _ => {}
        }
    }

    fn records(&self, records: &[DecodedRecord], now: i64) {
        let mut stations = false;
        for record in records {
            stations |= self.held.borrow_mut().decoded(record);
        }
        if stations {
            self.bump(Topic::Stations);
        }
        if self.held.borrow_mut().bearings(records, now) {
            self.bump(Topic::Df);
        }
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{
        EventOrigin,
        decode::{AdsbMessage, AisMessage},
        fusion::{BearingSource, DfBearing},
        radar::{RadarFix, RadarTrack},
    };

    use super::*;
    use crate::ui::kit_maps::iso_of;

    fn sample(lat: f64, received_at: i64) -> Sample {
        Sample {
            at: Geo::new(lat, 13.0),
            altitude_m: None,
            accuracy_m: None,
            received_at,
        }
    }

    #[test]
    fn a_fix_that_did_not_move_refreshes_the_last_sample() {
        let mut history = Vec::new();
        append(&mut history, sample(52.0, 1));
        append(&mut history, sample(52.0, 2));
        append(&mut history, sample(52.1, 3));
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].received_at, 2);
    }

    fn adsb(data: AdsbMessage, at: i64) -> DecodedRecord {
        DecodedRecord {
            origin: None,
            device_set: 0,
            channel: 0,
            at: iso_of(at),
            freq_hz: 1_090_000_000.0,
            event: DecoderEvent::Adsb(data),
            sinks: Vec::new(),
        }
    }

    #[test]
    fn a_station_keeps_what_earlier_frames_said() {
        let mut held = Held::default();
        let first = AdsbMessage {
            icao: "3c6444".to_owned(),
            df: 17,
            raw: "8d".to_owned(),
            callsign: Some("DLH123".to_owned()),
            ..AdsbMessage::default()
        };
        let second = AdsbMessage {
            lat: Some(52.5),
            lon: Some(13.4),
            ..first.clone()
        };
        let second = AdsbMessage {
            callsign: None,
            ..second
        };
        assert!(held.decoded(&adsb(first, 1_000)));
        assert!(held.decoded(&adsb(second, 2_000)));
        let station = &held.stations["adsb"]["3c6444"];
        assert_eq!(station.frames, 2);
        assert_eq!(station.last_seen, 2_000);
        let DecoderEvent::Adsb(merged) = &station.event else {
            panic!("an adsb event");
        };
        assert_eq!(merged.callsign.as_deref(), Some("DLH123"));
        assert_eq!(merged.lat, Some(52.5));
    }

    #[test]
    fn stations_age_out_and_only_positioned_kinds_are_held() {
        let mut held = Held::default();
        let ship = DecoderEvent::Ais(AisMessage {
            mmsi: 211_234_560,
            ..AisMessage::default()
        });
        assert_eq!(station_id(&ship).as_deref(), Some("211234560"));
        held.decoded(&adsb(
            AdsbMessage {
                icao: "a".to_owned(),
                ..AdsbMessage::default()
            },
            1_000,
        ));
        held.age_out(500, 2_000);
        assert!(held.stations["adsb"].is_empty());
    }

    fn bearing(node: &str, at: i64, deg: f32, lat: Option<f64>) -> DecodedRecord {
        DecodedRecord {
            origin: None,
            device_set: 0,
            channel: 0,
            at: iso_of(at),
            freq_hz: 433_920_000.0,
            event: DecoderEvent::Df(DfBearing {
                bearing_deg: deg,
                confidence: 0.9,
                lat,
                lon: Some(7.0),
                station_id: Some("roof".to_owned()),
                node: node.to_owned(),
                sigma_deg: 3.0,
                accuracy_m: None,
                heading_deg: None,
                heading_sigma_deg: None,
                relative_deg: None,
                mirror_deg: None,
                freq_hz: None,
                source: BearingSource::Array,
                moving: false,
                others: Vec::new(),
                likelihood: Vec::new(),
                snr_db: None,
            }),
            sinks: Vec::new(),
        }
    }

    #[test]
    fn a_bearing_record_carries_its_own_position_and_names_its_origin() {
        let mut record = bearing("df", 1_000, 45.0, Some(51.5));
        let sample = bearing_of(&record, 9).expect("a bearing");
        assert_eq!(sample.node, "df");
        assert_eq!(sample.from, Geo::new(51.5, 7.0));
        assert_eq!(sample.at, 1_000);
        assert_eq!(sample.station_id.as_deref(), Some("roof"));
        record.origin = Some(EventOrigin {
            node: "hunt".to_owned(),
            transmission: 1,
        });
        assert_eq!(
            bearing_of(&record, 9).map(|found| found.node).as_deref(),
            Some("hunt")
        );
        record.at = "not a time".to_owned();
        assert_eq!(bearing_of(&record, 9).map(|found| found.at), Some(9));
    }

    #[test]
    fn a_bearing_without_a_place_or_a_node_is_not_drawn() {
        assert!(bearing_of(&bearing("df", 0, 45.0, None), 0).is_none());
        assert!(bearing_of(&bearing("", 0, 45.0, Some(51.5)), 0).is_none());
        assert!(bearing_of(&adsb(AdsbMessage::default(), 0), 0).is_none());
    }

    #[test]
    fn bearings_keep_the_newest_within_five_minutes_without_repeats() {
        let mut held = Held::default();
        let now = BEARING_MAX_AGE_MS * 2;
        let records: Vec<DecodedRecord> = (0..(BEARING_HISTORY as i64 + 5))
            .map(|step| bearing("df", now - step, 10.0, Some(51.5)))
            .chain([bearing("df", 0, 10.0, Some(51.5))])
            .collect();
        assert!(held.bearings(&records, now));
        let kept = &held.bearings["df"];
        assert_eq!(kept.len(), BEARING_HISTORY);
        assert_eq!(kept.last().map(|sample| sample.at), Some(now));
        assert!(kept.windows(2).all(|pair| pair[0].at <= pair[1].at));
        assert!(!held.bearings(&records[..1], now));
        assert!(!held.bearings(&[adsb(AdsbMessage::default(), 0)], now));
    }

    fn radar(fixes: &[(u32, f64)]) -> RadarUpdate {
        RadarUpdate {
            tracks: fixes
                .iter()
                .map(|(id, lat)| RadarTrack {
                    id: *id,
                    fix: Some(RadarFix {
                        lat: *lat,
                        lon: 7.0,
                        ..RadarFix::default()
                    }),
                    ..RadarTrack::default()
                })
                .chain([RadarTrack {
                    id: 99,
                    ..RadarTrack::default()
                }])
                .collect(),
            ..RadarUpdate::default()
        }
    }

    #[test]
    fn a_fix_trail_grows_when_the_track_moves_and_forgets_lost_tracks() {
        let mut held = Held::default();
        held.radar("radar", &radar(&[(1, 51.0), (2, 52.0)]));
        held.radar("radar", &radar(&[(1, 51.0)]));
        held.radar("radar", &radar(&[(1, 51.1)]));
        let trails = &held.fix_trails["radar"];
        assert_eq!(trails.len(), 1);
        assert_eq!(trails[&1], [Geo::new(51.0, 7.0), Geo::new(51.1, 7.0)]);
        let mut long = None;
        for step in 0..(FIX_TRAIL_POINTS + 4) {
            long = Some(advance_trails(long.as_ref(), &radar(&[(1, step as f64)])));
        }
        let long = long.unwrap_or_default();
        assert_eq!(long[&1].len(), FIX_TRAIL_POINTS);
        assert_eq!(long[&1][0], Geo::new(4.0, 7.0));
    }

    #[test]
    fn a_moving_station_leaves_a_trail_that_ages_out_with_it() {
        let mut held = Held::default();
        let at = |lat: f64, ms: i64| {
            adsb(
                AdsbMessage {
                    icao: "abc".to_owned(),
                    lat: Some(lat),
                    lon: Some(8.0),
                    ..AdsbMessage::default()
                },
                ms,
            )
        };
        held.decoded(&at(50.0, 1_000));
        held.decoded(&at(50.1, 2_000));
        held.decoded(&at(50.3, 3_000));
        assert_eq!(held.trails["adsb"]["abc"].points.len(), 2);
        held.age_out(500, 10_000);
        assert!(held.trails["adsb"].is_empty());
    }
}
