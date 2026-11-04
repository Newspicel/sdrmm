use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

use sdrmm_dsp::bearing_fix::{BearingLine, LineFix, cross};
use sdrmm_engine::Engine;
use sdrmm_wire::{
    BearingFix, DecodedRecord, DecoderEvent, EventOrigin, NO_CHANNEL, NodeBody, PatchGraph,
    ServerEvent,
    geo::{self, Enu, LatLon},
    present::BearingRay,
};
use tokio::sync::{
    broadcast::{self, error::RecvError},
    watch,
};

use crate::{AppState, decoded::Decoded, df_fusion::now_s, resolved::Resolved};

/// How long a station's newest bearing still counts toward the fix.
const HOLD_S: f64 = 30.0;
const FIX_EVERY_S: f64 = 1.0;
/// The spread a ray that states none is given, the same as a bare bearing gets.
const UNSTATED_SIGMA_DEG: f64 = 10.0;
const REFINES: usize = 2;
const MIN_REACH_M: f64 = 100.0;

type Station = (String, String);

struct Line {
    ray: BearingRay,
    at_s: f64,
}

#[derive(Default)]
struct Crossing {
    lines: BTreeMap<Station, Line>,
    unplaced: HashSet<Station>,
    sent_s: Option<f64>,
}

impl Crossing {
    fn observe(&mut self, station: Station, ray: BearingRay, now_s: f64) -> Option<BearingFix> {
        self.unplaced.remove(&station);
        self.lines.insert(station, Line { ray, at_s: now_s });
        self.lines.retain(|_, line| now_s - line.at_s <= HOLD_S);
        if self.sent_s.is_some_and(|sent| now_s - sent < FIX_EVERY_S) {
            return None;
        }
        let rays: Vec<BearingRay> = self.lines.values().map(|line| line.ray).collect();
        let fix = fix_of(&rays)?;
        self.sent_s = Some(now_s);
        Some(fix)
    }

    fn unplaced(&mut self, station: Station) -> bool {
        self.lines.remove(&station);
        self.unplaced.insert(station)
    }
}

fn fix_of(rays: &[BearingRay]) -> Option<BearingFix> {
    let mut plane = Enu::new(centre(rays)?);
    let from_origins: Vec<BearingLine> = rays.iter().map(|ray| laid(&plane, *ray)).collect();
    let mut fix = cross(&from_origins)?;
    for _ in 0..REFINES {
        let near = plane.to_latlon(fix.east_m, fix.north_m);
        plane = Enu::new(near);
        let passing: Vec<BearingLine> =
            rays.iter().map(|ray| passing(&plane, *ray, near)).collect();
        fix = cross(&passing)?;
    }
    Some(located(&plane, &fix, rays.len()))
}

fn centre(rays: &[BearingRay]) -> Option<LatLon> {
    let first = Enu::new(rays.first()?.from);
    let (east, north) = rays.iter().fold((0.0, 0.0), |(east, north), ray| {
        let (e, n) = first.to_enu(ray.from);
        (east + e, north + n)
    });
    let count = rays.len() as f64;
    Some(first.to_latlon(east / count, north / count))
}

fn laid(plane: &Enu, ray: BearingRay) -> BearingLine {
    let (east_m, north_m) = plane.to_enu(ray.from);
    BearingLine {
        east_m,
        north_m,
        bearing_deg: f64::from(ray.true_deg),
        sigma_deg: sigma_of(ray),
    }
}

/// The ray where it runs past `near`, headed as its great circle heads there, from an origin
/// as far back as the station so range weights hold. This keeps a station a hundred
/// kilometres off true where the flat plane is centred.
fn passing(plane: &Enu, ray: BearingRay, near: LatLon) -> BearingLine {
    let reach = geo::distance_m(ray.from, near);
    if reach < MIN_REACH_M {
        return laid(plane, ray);
    }
    let passing = geo::destination(ray.from, f64::from(ray.true_deg), reach);
    let heading = geo::bearing_deg(passing, ray.from) + 180.0;
    let (east, north) = plane.to_enu(passing);
    let (sin, cos) = heading.to_radians().sin_cos();
    BearingLine {
        east_m: east - reach * sin,
        north_m: north - reach * cos,
        bearing_deg: heading,
        sigma_deg: sigma_of(ray),
    }
}

fn sigma_of(ray: BearingRay) -> f64 {
    ray.sigma_deg
        .filter(|sigma| sigma.is_finite())
        .map_or(UNSTATED_SIGMA_DEG, f64::from)
}

fn located(plane: &Enu, fix: &LineFix, stations: usize) -> BearingFix {
    let at = plane.to_latlon(fix.east_m, fix.north_m);
    BearingFix {
        lat: at.lat,
        lon: at.lon,
        ellipse_major_m: 2.0 * fix.major_m,
        ellipse_minor_m: 2.0 * fix.minor_m,
        ellipse_bearing_deg: fix.major_bearing_deg,
        stations: u32::try_from(stations).unwrap_or(u32::MAX),
        spread_m: fix.spread_m,
    }
}

#[derive(Default)]
struct Fixes {
    nodes: HashMap<String, Crossing>,
}

impl Fixes {
    fn follow(&mut self, graph: &PatchGraph) {
        let wanted: HashSet<&str> = graph
            .nodes
            .iter()
            .filter(|node| matches!(node.body, NodeBody::BearingFix))
            .map(|node| node.id.as_str())
            .collect();
        self.nodes.retain(|node, _| wanted.contains(node.as_str()));
        for node in wanted {
            self.nodes.entry(node.to_owned()).or_default();
        }
    }

    fn deliver(&mut self, engine: &Engine, record: &DecodedRecord, now_s: f64) {
        if !record
            .sinks
            .iter()
            .any(|sink| self.nodes.contains_key(sink))
        {
            return;
        }
        let (station, ray) = sighted(record);
        for sink in &record.sinks {
            let Some(crossing) = self.nodes.get_mut(sink) else {
                continue;
            };
            match ray {
                Some(ray) => {
                    if let Some(fix) = crossing.observe(station.clone(), ray, now_s) {
                        engine.publish_decoded(fix_record(sink, record, fix));
                    }
                }
                None if crossing.unplaced(station.clone()) => {
                    engine.emit_event(ServerEvent::Error {
                        message: format!("Bearing fix: {} has no position", label(&station)),
                    });
                }
                None => {}
            }
        }
    }

    fn lost(&self, engine: &Engine, count: u64) {
        if !self.nodes.is_empty() {
            engine.emit_event(ServerEvent::Error {
                message: format!("Bearing fix missed {count} events"),
            });
        }
    }
}

fn sighted(record: &DecodedRecord) -> (Station, Option<BearingRay>) {
    let node = record
        .origin
        .as_ref()
        .map(|origin| origin.node.clone())
        .unwrap_or_default();
    let (entity, ray) = match &record.view {
        Some(view) => (view.entity.clone(), view.bearing),
        None => {
            let view = record.event.present();
            (view.entity, view.bearing)
        }
    };
    ((node, entity.unwrap_or_default()), ray)
}

fn label(station: &Station) -> &str {
    if station.1.is_empty() {
        &station.0
    } else {
        &station.1
    }
}

fn fix_record(node: &str, from: &DecodedRecord, fix: BearingFix) -> DecodedRecord {
    DecodedRecord {
        origin: Some(EventOrigin {
            node: node.to_owned(),
            transmission: 0,
        }),
        device_set: from.device_set,
        channel: NO_CHANNEL,
        at: from.at.clone(),
        freq_hz: 0.0,
        event: DecoderEvent::of(&fix),
        sinks: Vec::new(),
        callsign: None,
        view: None,
    }
}

pub(crate) fn start(state: &AppState) {
    let records = state.decoded.subscribe();
    let changes = state.resolved.subscribe();
    let engine = state.engine.clone();
    let _detached = crate::spawn_task("sdrmm-bearing-fix", move || run(engine, records, changes));
}

async fn run(
    engine: Arc<Engine>,
    mut records: broadcast::Receiver<Decoded>,
    mut changes: watch::Receiver<Arc<Resolved>>,
) {
    let mut fixes = Fixes::default();
    fixes.follow(&changes.borrow_and_update().graph);
    loop {
        tokio::select! {
            changed = changes.changed() => {
                if changed.is_err() {
                    break;
                }
                let resolved = changes.borrow_and_update().clone();
                fixes.follow(&resolved.graph);
            }
            record = records.recv() => match record {
                Ok(Decoded::Record(routed)) => fixes.deliver(&engine, &routed.record, now_s()),
                Ok(Decoded::Lost(count)) | Err(RecvError::Lagged(count)) => {
                    fixes.lost(&engine, count);
                }
                Err(RecvError::Closed) => break,
            },
        }
    }
}

#[cfg(test)]
mod tests;
