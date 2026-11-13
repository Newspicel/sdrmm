use std::{collections::HashMap, time::Instant};

use jiff::Timestamp;
use sdrmm_dsp::radar::assign::Assignment;
use sdrmm_wire::{
    AoaState, DecoderEvent, PassiveRadarParams, ProcessorAction, ProcessorReading, RADAR_TX_PORT,
    RadarProblem, RadarSites, RadarTrack, RadarTrackEvent, RadarUpdate, ReferenceMode, TrackState,
    present::TrackFix,
};

use crate::array_processor::{EnrichCtx, Raised, Refusal, ReportEnricher};

mod sites;
mod truth;

#[cfg(test)]
mod tests;

use sites::Sites;
use truth::AircraftTable;

const OVERLOAD_CPIS: u32 = 3;
const SITES_EVERY: std::time::Duration = std::time::Duration::from_secs(1);

pub(super) fn enricher() -> Box<dyn ReportEnricher> {
    Box::new(RadarEnricher::default())
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Enrichment {
    bearing_deg: Option<f32>,
    lat: Option<f64>,
    lon: Option<f64>,
    target: Option<String>,
}

impl Enrichment {
    fn of(track: &RadarTrack) -> Self {
        Self {
            bearing_deg: track.aoa.and_then(|aoa| aoa.bearing_deg),
            lat: track.fix.map(|fix| fix.lat),
            lon: track.fix.map(|fix| fix.lon),
            target: track.truth.as_ref().map(|truth| truth.id.clone()),
        }
    }

    fn fill(&self, event: &mut RadarTrackEvent) {
        event.bearing_deg = self.bearing_deg;
        event.lat = self.lat;
        event.lon = self.lon;
        event.target.clone_from(&self.target);
    }
}

/// Sites, ADS-B truth and track events of a passive radar, kept between its reports.
struct RadarEnricher {
    aircraft: AircraftTable,
    solver: Assignment,
    overloaded: u32,
    lagged: u64,
    remembered: HashMap<u32, Enrichment>,
    sites: Option<(RadarSites, Instant)>,
}

impl Default for RadarEnricher {
    fn default() -> Self {
        Self {
            aircraft: AircraftTable::default(),
            solver: truth::solver(),
            overloaded: 0,
            lagged: 0,
            remembered: HashMap::new(),
            sites: None,
        }
    }
}

fn cpi_centre(update: &RadarUpdate) -> Timestamp {
    let end = update
        .at
        .parse::<Timestamp>()
        .unwrap_or_else(|_| Timestamp::now());
    let half_ms = (f64::from(update.axes.cpi_ms) / 2.0).round() as i64;
    Timestamp::from_millisecond(end.as_millisecond().saturating_sub(half_ms)).unwrap_or(end)
}

fn now_at() -> String {
    format!("{:.3}", Timestamp::now())
}

fn params_in(ctx: &EnrichCtx<'_>) -> PassiveRadarParams {
    super::params_of(ctx.params).copied().unwrap_or_default()
}

fn sites_raised(sites: &RadarSites) -> Raised {
    Raised {
        event: DecoderEvent::of(sites),
        at: now_at(),
        freq_hz: 0.0,
    }
}

impl RadarEnricher {
    fn sites_due(&mut self, update: &RadarUpdate, now: Instant) -> Option<RadarSites> {
        let next = RadarSites {
            geometry: update.geometry,
            ranges_km: update
                .tracks
                .iter()
                .filter(|track| track.state == TrackState::Confirmed)
                .map(|track| track.range_km)
                .collect(),
        };
        let due = match &self.sites {
            None => next.geometry.is_some(),
            Some((told, at)) => {
                told.geometry != next.geometry
                    || (told.ranges_km != next.ranges_km && now.duration_since(*at) >= SITES_EVERY)
            }
        };
        if !due {
            return None;
        }
        self.sites = Some((next.clone(), now));
        Some(next)
    }

    fn fill(&mut self, update: &mut RadarUpdate, sites: &Sites, params: &PassiveRadarParams) {
        update.problems.clear();
        sites.problems(&mut update.problems);
        update.geometry = sites.geometry();
        if let Some(heading_deg) = sites.heading_deg() {
            sites::true_bearings(update, heading_deg);
        }
        self.place(update, sites, params);
        self.pod_problems(update, params);
        update.health.lagged_updates = update.health.lagged_updates.saturating_add(self.lagged);
    }

    fn place(&mut self, update: &mut RadarUpdate, sites: &Sites, params: &PassiveRadarParams) {
        let now = cpi_centre(update);
        self.aircraft.prune(now);
        update.truth.clear();
        for track in &mut update.tracks {
            track.fix = None;
            track.truth = None;
        }
        let Some(baseline) = sites.both() else {
            return;
        };
        self.aircraft
            .truth(now, baseline, &update.axes, &mut update.truth);
        truth::associate(
            &mut self.solver,
            &mut update.truth,
            &mut update.tracks,
            &update.axes,
        );
        if sites.heading_deg().is_some() {
            sites::fix_tracks(update, baseline, params.assumed_altitude_m);
        }
    }

    fn pod_problems(&mut self, update: &mut RadarUpdate, params: &PassiveRadarParams) {
        let health = &update.health;
        if params.aoa && health.aoa == AoaState::PhaseUnknown {
            update.problems.push(RadarProblem::PhaseUnknown);
        }
        if params.aoa && health.table_out_of_range {
            update.problems.push(RadarProblem::TableOutOfRange);
        }
        self.overloaded = if health.load > 1.0 {
            self.overloaded.saturating_add(1)
        } else {
            0
        };
        if self.overloaded >= OVERLOAD_CPIS {
            update.problems.push(RadarProblem::Overloaded);
        }
        if health.reference.mode != ReferenceMode::Raw && !health.reference.locked {
            update.problems.push(RadarProblem::ReferenceLost);
        }
    }

    fn enrich(
        &mut self,
        events: Vec<RadarTrackEvent>,
        tracks: &[RadarTrack],
    ) -> Vec<RadarTrackEvent> {
        for track in tracks {
            self.remembered.insert(track.id, Enrichment::of(track));
        }
        let enriched: Vec<RadarTrackEvent> = events
            .into_iter()
            .map(|mut event| {
                if let Some(known) = self.remembered.get(&event.track_id) {
                    known.fill(&mut event);
                }
                event
            })
            .collect();
        self.remembered
            .retain(|id, _| tracks.iter().any(|track| track.id == *id));
        enriched
    }

    fn complete_update(&mut self, ctx: &EnrichCtx<'_>, update: &mut RadarUpdate) -> Vec<Raised> {
        let sites = Sites::of(ctx.array.as_ref(), ctx.position(RADAR_TX_PORT));
        let events = std::mem::take(&mut update.events);
        self.fill(update, &sites, &params_in(ctx));
        let enriched = self.enrich(events, &update.tracks);
        let at = if update.at.is_empty() {
            now_at()
        } else {
            update.at.clone()
        };
        let freq_hz = update.axes.carrier_hz;
        self.sites_due(update, Instant::now())
            .map(|sites| sites_raised(&sites))
            .into_iter()
            .chain(enriched.iter().map(|event| Raised {
                event: DecoderEvent::of(event),
                at: at.clone(),
                freq_hz,
            }))
            .collect()
    }
}

fn clear_tracks(update: &mut RadarUpdate) {
    update.tracks.clear();
    for detection in &mut update.detections {
        detection.track_id = None;
    }
    for sighting in &mut update.truth {
        sighting.track_id = None;
    }
}

impl ReportEnricher for RadarEnricher {
    fn complete(&mut self, ctx: &EnrichCtx<'_>, reading: &mut ProcessorReading) -> Vec<Raised> {
        match reading {
            ProcessorReading::PassiveRadar(update) => self.complete_update(ctx, update),
            _ => Vec::new(),
        }
    }

    fn refused(&mut self, refusal: &Refusal) -> (ProcessorReading, Vec<Raised>) {
        let problem = match refusal {
            Refusal::NoArray => RadarProblem::NoArray,
            Refusal::Refused(text) => RadarProblem::Refused(text.clone()),
        };
        let update = RadarUpdate {
            at: now_at(),
            problems: vec![problem],
            ..RadarUpdate::default()
        };
        self.remembered.clear();
        let raised = self
            .sites_due(&update, Instant::now())
            .map(|sites| sites_raised(&sites))
            .into_iter()
            .collect();
        (ProcessorReading::PassiveRadar(update), raised)
    }

    fn waiting(&mut self, ctx: &EnrichCtx<'_>) -> ProcessorReading {
        let sites = Sites::of(ctx.array.as_ref(), ctx.position(RADAR_TX_PORT));
        let mut update = RadarUpdate {
            at: now_at(),
            ..RadarUpdate::default()
        };
        self.fill(&mut update, &sites, &params_in(ctx));
        self.remembered.clear();
        ProcessorReading::PassiveRadar(update)
    }

    fn observe(&mut self, at: Timestamp, fix: &TrackFix) {
        self.aircraft.observe(at, fix);
    }

    fn act(&mut self, action: ProcessorAction, latest: Option<&mut ProcessorReading>) {
        match action {
            ProcessorAction::ClearTracks => {
                self.remembered.clear();
                if let Some(ProcessorReading::PassiveRadar(update)) = latest {
                    clear_tracks(update);
                }
            }
        }
    }

    fn lagged(&mut self, count: u64) {
        self.lagged = self.lagged.saturating_add(count);
    }
}
