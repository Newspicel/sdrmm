use std::{
    collections::HashMap,
    f64::consts::TAU,
    time::{SystemTime, UNIX_EPOCH},
};

use sdrmm_wire::{
    fusion::{DfEstimate, DfFusionState, DfStation, NavTarget},
    patch::{NodeBody, PatchGraph},
    processor::ProcessorReading,
    radar::{RadarTrack, RadarUpdate, TrackState},
};
use zgui::{prelude::*, vocab::SharedString};

use crate::{
    arrays::{Book, Heard},
    ui::{
        kit_maps::{
            feed::{BEARING_MAX_AGE_MS, BearingSample, Held},
            trail::legend_row,
            wiring::event_sources,
        },
        map::{
            ACCENT, Geo,
            geo::{bearing_deg, destination, great_circle_km},
            overlay::{Dot, Line, Overlay, Polygon, WHITE},
        },
    },
};

pub const RAY_LENGTH_M: f64 = 25_000.0;
pub const ELLIPSE_POINTS: usize = 48;
const STALE_SLACK_MS: i64 = 1_000;
const CONVERGED: u32 = 0x3f_ae_7a;
pub const STATION: u32 = 0xb0_7d_e0;
pub const TRACK: u32 = 0x7f_b2_e0;
pub const TRANSMITTER: u32 = 0xe0_a4_58;
const UNPLACED: u32 = 0xef_62_62;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub from: Geo,
    pub bearing_deg: f64,
    pub confidence: f64,
    pub sigma_deg: f64,
    pub age_ms: i64,
}

#[must_use]
pub fn ray_of(sample: &BearingSample, now: i64) -> Ray {
    Ray {
        from: sample.from,
        bearing_deg: sample.true_deg,
        confidence: sample.confidence,
        sigma_deg: sample.sigma_deg,
        age_ms: (now - sample.at).max(0),
    }
}

#[must_use]
pub fn ray_weight(ray: &Ray, max_age_ms: i64) -> Option<f64> {
    (ray.age_ms <= max_age_ms)
        .then(|| (ray.confidence * (1.0 - ray.age_ms as f64 / max_age_ms.max(1) as f64)).max(0.05))
}

#[must_use]
pub fn ray_line(ray: &Ray) -> [Geo; 2] {
    [
        ray.from,
        destination(ray.from, ray.bearing_deg, RAY_LENGTH_M),
    ]
}

fn ring(centre: Geo, along: f64, across: f64, axis_deg: f64) -> Vec<Geo> {
    let (sin, cos) = axis_deg.to_radians().sin_cos();
    (0..=ELLIPSE_POINTS)
        .map(|step| {
            let angle = step as f64 / ELLIPSE_POINTS as f64 * TAU;
            let forward = along * angle.cos();
            let sideways = across * angle.sin();
            let east = forward * sin + sideways * cos;
            let north = forward * cos - sideways * sin;
            destination(centre, east.atan2(north).to_degrees(), east.hypot(north))
        })
        .collect()
}

#[must_use]
pub fn ellipse(estimate: &DfEstimate) -> Vec<Geo> {
    ring(
        Geo::new(estimate.lat, estimate.lon),
        estimate.ellipse_major_m / 2.0,
        estimate.ellipse_minor_m / 2.0,
        estimate.ellipse_bearing_deg,
    )
}

#[must_use]
pub fn bistatic_ring(receiver: Geo, illuminator: Geo, range_km: f64) -> Option<Vec<Geo>> {
    let range_m = range_km * 1_000.0;
    if range_m.is_nan() || range_m <= 0.0 {
        return None;
    }
    let baseline_m = great_circle_km(receiver, illuminator) * 1_000.0;
    let along = (baseline_m + range_m) / 2.0;
    let across = (range_m * (range_m + 2.0 * baseline_m)).sqrt() / 2.0;
    let axis = bearing_deg(receiver, illuminator);
    let centre = destination(receiver, axis, baseline_m / 2.0);
    Some(ring(centre, along, across, axis))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sources {
    pub finders: Vec<String>,
    pub hunts: Vec<String>,
    pub crossings: Vec<String>,
    pub radars: Vec<String>,
    pub labels: HashMap<String, String>,
}

impl Sources {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.finders.is_empty()
            && self.hunts.is_empty()
            && self.crossings.is_empty()
            && self.radars.is_empty()
    }

    fn label(&self, node: &str) -> String {
        self.labels
            .get(node)
            .cloned()
            .unwrap_or_else(|| node.to_owned())
    }
}

#[must_use]
pub fn sources_of(graph: &PatchGraph, map: &str) -> Sources {
    let mut out = Sources::default();
    for id in event_sources(graph, map) {
        let Some(found) = graph.node(&id) else {
            continue;
        };
        let (bucket, name) = match &found.body {
            NodeBody::Df(_) => (&mut out.finders, "Direction finder"),
            NodeBody::Hunt(_) => (&mut out.hunts, "Signal hunt"),
            NodeBody::Triangulation(_) => (&mut out.crossings, "Triangulation"),
            NodeBody::PassiveRadar(_) => (&mut out.radars, "Passive radar"),
            _ => continue,
        };
        bucket.push(id.clone());
        let label = found.label.clone().unwrap_or_else(|| name.to_owned());
        out.labels.insert(id, label);
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct Echoes {
    pub receiver: Geo,
    pub illuminator: Geo,
    pub ranges_km: Vec<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Site {
    pub receiver: Geo,
    pub transmitter: Geo,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fix {
    pub id: u32,
    pub at: Geo,
    pub major_m: f64,
    pub minor_m: f64,
    pub orientation_deg: f64,
    pub trail: Vec<Geo>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Picture {
    pub rays: Vec<Ray>,
    pub estimate: Option<DfEstimate>,
    pub emitters: Vec<DfEstimate>,
    pub nav: Option<NavTarget>,
    pub stations: Vec<DfStation>,
    pub bistatic: Vec<Echoes>,
    pub sites: Vec<Site>,
    pub fixes: Vec<Fix>,
    pub unplaced: Vec<String>,
    pub from: Option<Geo>,
}

#[must_use]
pub fn millis_of_time(at: SystemTime) -> i64 {
    at.duration_since(UNIX_EPOCH).map_or(0, |since| {
        i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
    })
}

#[must_use]
pub fn radar_stale(received_ms: i64, hop_ms: f32, now: i64) -> bool {
    (now - received_ms) as f64 > 3.0 * f64::from(hop_ms) + STALE_SLACK_MS as f64
}

#[must_use]
pub fn finder_placed(heard: Option<&Heard>) -> bool {
    match heard.map(|held| &held.reading) {
        Some(ProcessorReading::Df(reading)) => {
            reading.station.is_some() && reading.azimuth_deg.is_some()
        }
        _ => true,
    }
}

#[must_use]
pub fn picture(
    sources: &Sources,
    held: &Held,
    book: &Book,
    now: i64,
    from: Option<Geo>,
) -> Option<Picture> {
    if sources.is_empty() {
        return None;
    }
    let rays = sources
        .finders
        .iter()
        .chain(&sources.hunts)
        .filter_map(|node| held.bearings.get(node))
        .flatten()
        .map(|sample| ray_of(sample, now))
        .collect();
    let mut out = Picture {
        rays,
        from,
        unplaced: sources
            .finders
            .iter()
            .filter(|node| !finder_placed(book.readings.get(*node)))
            .map(|node| sources.label(node))
            .collect(),
        ..Picture::default()
    };
    fusion_part(&mut out, &sources.crossings, &book.fusion);
    for node in &sources.radars {
        radar_part(&mut out, sources, node, held, book, now);
    }
    Some(out)
}

fn fusion_part(out: &mut Picture, crossings: &[String], fusion: &HashMap<String, DfFusionState>) {
    for state in crossings.iter().filter_map(|node| fusion.get(node)) {
        if out.estimate.is_none() && state.estimate.is_some() {
            out.estimate = state.estimate;
            out.nav = state.nav;
        }
        out.emitters.extend_from_slice(&state.emitters);
        out.stations.extend_from_slice(&state.stations);
    }
}

fn radar_part(
    out: &mut Picture,
    sources: &Sources,
    node: &str,
    held: &Held,
    book: &Book,
    now: i64,
) {
    let Some(heard) = book.readings.get(node) else {
        return;
    };
    let ProcessorReading::PassiveRadar(update) = &heard.reading else {
        return;
    };
    let Some(geometry) = &update.geometry else {
        out.unplaced.push(sources.label(node));
        return;
    };
    let receiver = Geo::new(geometry.receiver.lat, geometry.receiver.lon);
    let transmitter = Geo::new(geometry.transmitter.lat, geometry.transmitter.lon);
    out.sites.push(Site {
        receiver,
        transmitter,
    });
    if radar_stale(millis_of_time(heard.at), update.axes.hop_ms, now) {
        return;
    }
    out.bistatic.push(Echoes {
        receiver,
        illuminator: transmitter,
        ranges_km: confirmed_ranges(update),
    });
    let trails = held.fix_trails.get(node);
    out.fixes.extend(
        update
            .tracks
            .iter()
            .filter_map(|track| fix_of(track, trails.and_then(|held| held.get(&track.id)))),
    );
}

#[must_use]
pub fn confirmed_ranges(update: &RadarUpdate) -> Vec<f64> {
    update
        .tracks
        .iter()
        .filter(|track| track.state == TrackState::Confirmed)
        .map(|track| f64::from(track.range_km))
        .collect()
}

fn fix_of(track: &RadarTrack, trail: Option<&Vec<Geo>>) -> Option<Fix> {
    let fix = track.fix.as_ref()?;
    Some(Fix {
        id: track.id,
        at: Geo::new(fix.lat, fix.lon),
        major_m: f64::from(fix.major_m),
        minor_m: f64::from(fix.minor_m),
        orientation_deg: f64::from(fix.orientation_deg),
        trail: trail.cloned().unwrap_or_default(),
    })
}

fn same_spot(a: &DfEstimate, b: &DfEstimate) -> bool {
    a.lat == b.lat && a.lon == b.lon
}

#[must_use]
pub fn other_emitters<'a>(
    estimate: Option<&DfEstimate>,
    emitters: &'a [DfEstimate],
) -> Vec<&'a DfEstimate> {
    emitters
        .iter()
        .filter(|emitter| estimate.is_none_or(|estimate| !same_spot(emitter, estimate)))
        .collect()
}

#[must_use]
pub fn overlay(picture: &Picture) -> Overlay {
    let mut out = Overlay::default();
    bearing_layers(&mut out, picture);
    radar_layers(&mut out, picture);
    fusion_layers(&mut out, picture);
    out
}

fn bearing_layers(out: &mut Overlay, picture: &Picture) {
    for ray in &picture.rays {
        let Some(weight) = ray_weight(ray, BEARING_MAX_AGE_MS) else {
            continue;
        };
        out.lines.push(Line {
            points: ray_line(ray).to_vec(),
            colour: ACCENT,
            alpha: (0.08 + 0.82 * weight.clamp(0.0, 1.0)) as f32,
            width: 1.5,
            dash: None,
        });
    }
    let others = other_emitters(picture.estimate.as_ref(), &picture.emitters);
    for estimate in picture.estimate.iter().chain(others) {
        out.polygons.push(Polygon {
            ring: ellipse(estimate),
            fill: ACCENT,
            fill_alpha: 0.12,
            stroke: Some((ACCENT, 0.6, 1.0)),
        });
    }
}

fn radar_layers(out: &mut Overlay, picture: &Picture) {
    for echoes in &picture.bistatic {
        for range_km in &echoes.ranges_km {
            if let Some(points) = bistatic_ring(echoes.receiver, echoes.illuminator, *range_km) {
                out.lines.push(Line {
                    points,
                    colour: TRACK,
                    alpha: 0.7,
                    width: 1.0,
                    dash: Some([3.0, 2.0]),
                });
            }
        }
    }
    for fix in &picture.fixes {
        if fix.major_m > 0.0 && fix.minor_m > 0.0 {
            out.polygons.push(Polygon {
                ring: ring(fix.at, fix.major_m, fix.minor_m, fix.orientation_deg),
                fill: TRACK,
                fill_alpha: 0.15,
                stroke: None,
            });
        }
        if fix.trail.len() > 1 {
            out.lines.push(Line {
                points: fix.trail.clone(),
                colour: TRACK,
                alpha: 0.6,
                width: 1.5,
                dash: None,
            });
        }
        out.dots.push(Dot {
            stroke: WHITE,
            ..Dot::plain(fix.at, 4.0, TRACK)
        });
    }
    for site in &picture.sites {
        for (at, colour) in [(site.receiver, ACCENT), (site.transmitter, TRANSMITTER)] {
            out.dots.push(Dot {
                stroke: WHITE,
                stroke_width: 1.5,
                ..Dot::plain(at, 5.0, colour)
            });
        }
    }
}

fn fusion_layers(out: &mut Overlay, picture: &Picture) {
    if let Some(estimate) = &picture.estimate {
        let fill = if estimate.converged {
            CONVERGED
        } else {
            ACCENT
        };
        out.dots.push(Dot {
            stroke: WHITE,
            stroke_width: 1.5,
            ..Dot::plain(Geo::new(estimate.lat, estimate.lon), 6.0, fill)
        });
    }
    for station in &picture.stations {
        out.dots.push(Dot {
            stroke: WHITE,
            ..Dot::plain(Geo::new(station.lat, station.lon), 4.0, STATION)
        });
    }
    let Some(nav) = &picture.nav else {
        return;
    };
    let target = Geo::new(nav.lat, nav.lon);
    if let Some(from) = picture.from {
        out.lines.push(Line {
            points: vec![from, target],
            colour: TRANSMITTER,
            alpha: 1.0,
            width: 2.0,
            dash: Some([2.0, 2.0]),
        });
    }
    out.dots.push(Dot {
        alpha: 0.0,
        stroke: TRANSMITTER,
        stroke_width: 2.0,
        ..Dot::plain(target, 5.0, TRANSMITTER)
    });
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub bearings: usize,
    pub echoes: usize,
    pub tracks: usize,
    pub unplaced: Vec<String>,
}

#[must_use]
pub fn counts(picture: Option<&Picture>) -> Counts {
    let Some(picture) = picture else {
        return Counts::default();
    };
    Counts {
        bearings: picture
            .rays
            .iter()
            .filter(|ray| ray.age_ms <= BEARING_MAX_AGE_MS)
            .count(),
        echoes: picture
            .bistatic
            .iter()
            .map(|echoes| echoes.ranges_km.iter().filter(|km| **km > 0.0).count())
            .sum(),
        tracks: picture.fixes.len(),
        unplaced: picture.unplaced.clone(),
    }
}

#[must_use]
pub fn legend_rows(counts: &Counts) -> Vec<AnyView> {
    let mut rows: Vec<AnyView> = [
        (ACCENT, "Bearings", counts.bearings),
        (TRACK, "Echoes", counts.echoes),
        (TRACK, "Tracks", counts.tracks),
    ]
    .into_iter()
    .filter(|(_, _, count)| *count > 0)
    .map(|(colour, name, count)| AnyView::new(legend_row(colour, name, move || count.to_string())))
    .collect();
    if !counts.unplaced.is_empty() {
        rows.push(AnyView::new(unplaced_row(&counts.unplaced)));
    }
    rows
}

fn unplaced_row(unplaced: &[String]) -> impl IntoView + use<> {
    let count = unplaced.len().to_string();
    let names = SharedString::from(unplaced.join(", "));
    view! {
        row(class = "geo__legend-row geo__bad", a11y:description = names) {
            box(class = "geo__swatch", style:background-color = Some(format!("#{UNPLACED:06x}")))
            text(class = "geo__legend-name geo__bad") {"No position"}
            text(class = "geo__legend-count geo__bad") {{count}}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sdrmm_wire::{
        fusion::NavTargetKind,
        patch::{PatchEdge, PatchNode, PortRef, Position},
        processor::df::DfReading,
        radar::{RadarFix, RadarGeometry, RadarSite},
    };

    use super::*;

    const HOME: Geo = Geo::new(51.5, 7.0);
    const MAST: Geo = Geo::new(51.5, 7.3);

    fn ray(age_ms: i64) -> Ray {
        Ray {
            from: HOME,
            bearing_deg: 45.0,
            confidence: 0.9,
            sigma_deg: 3.0,
            age_ms,
        }
    }

    fn estimate(lat: f64, converged: bool) -> DfEstimate {
        DfEstimate {
            lat,
            lon: 7.05,
            ellipse_major_m: 800.0,
            ellipse_minor_m: 200.0,
            ellipse_bearing_deg: 45.0,
            converged,
            samples: 8,
            mass: 1.0,
        }
    }

    fn nav() -> NavTarget {
        NavTarget {
            lat: 51.51,
            lon: 7.02,
            kind: NavTargetKind::Probe,
            revision: 1,
            distance_m: 1_500.0,
            bearing_deg: 135.0,
        }
    }

    fn sample(node: &str, at: i64) -> BearingSample {
        BearingSample {
            node: node.to_owned(),
            true_deg: 90.0,
            confidence: 0.8,
            sigma_deg: 4.0,
            from: HOME,
            at,
            station_id: None,
        }
    }

    fn node(id: &str, kind: &str) -> PatchNode {
        PatchNode {
            id: id.to_owned(),
            body: NodeBody::default_for(kind).expect("a node kind"),
            position: Position { x: 0.0, y: 0.0 },
            size: None,
            label: None,
        }
    }

    fn events(from: &str, to: &str) -> PatchEdge {
        PatchEdge {
            from: PortRef {
                node: from.to_owned(),
                port: "events".to_owned(),
            },
            to: PortRef {
                node: to.to_owned(),
                port: "events".to_owned(),
            },
        }
    }

    fn radar_update(geometry: bool, hop_ms: f32) -> RadarUpdate {
        let site = |at: Geo| RadarSite {
            lat: at.lat,
            lon: at.lon,
            altitude_m: 0.0,
        };
        let mut update = RadarUpdate::default();
        update.axes.hop_ms = hop_ms;
        update.geometry = geometry.then(|| RadarGeometry {
            receiver: site(HOME),
            transmitter: site(MAST),
            baseline_km: 20.0,
            heading_deg: None,
        });
        update.tracks = vec![
            RadarTrack {
                id: 1,
                state: TrackState::Confirmed,
                range_km: 4.0,
                fix: Some(RadarFix {
                    lat: 51.52,
                    lon: 7.1,
                    major_m: 300.0,
                    minor_m: 100.0,
                    orientation_deg: 30.0,
                    ..RadarFix::default()
                }),
                ..RadarTrack::default()
            },
            RadarTrack {
                id: 2,
                state: TrackState::Coasting,
                range_km: 9.0,
                ..RadarTrack::default()
            },
        ];
        update
    }

    fn heard(reading: ProcessorReading, at_ms: u64) -> Heard {
        Heard {
            reading,
            at: UNIX_EPOCH + Duration::from_millis(at_ms),
        }
    }

    fn radar_sources() -> Sources {
        Sources {
            radars: vec!["radar".to_owned()],
            labels: HashMap::from([("radar".to_owned(), "Roof radar".to_owned())]),
            ..Sources::default()
        }
    }

    #[test]
    fn a_bearing_is_drawn_out_to_the_ray_length() {
        let [start, end] = ray_line(&ray(0));
        assert_eq!(start, HOME);
        assert_eq!(end, destination(HOME, 45.0, RAY_LENGTH_M));
        let drawn = overlay(&Picture {
            rays: vec![ray(0)],
            ..Picture::default()
        });
        assert_eq!(drawn.lines.len(), 1);
    }

    #[test]
    fn an_older_bearing_fades_and_one_past_its_age_is_dropped() {
        let fresh = ray_weight(&ray(0), 60_000).expect("fresh");
        let old = ray_weight(&ray(45_000), 60_000).expect("old");
        assert!(old < fresh);
        assert!(ray_weight(&ray(90_000), 60_000).is_none());
    }

    #[test]
    fn a_ray_starts_where_the_bearing_was_taken_and_ages_from_its_time() {
        let made = ray_of(&sample("df", 1_000), 4_000);
        assert_eq!(made.from, HOME);
        assert_eq!(made.age_ms, 3_000);
        assert!((made.sigma_deg - 4.0).abs() < 1e-9);
        assert_eq!(ray_of(&sample("df", 5_000), 4_000).age_ms, 0);
    }

    #[test]
    fn nothing_is_marked_until_there_is_an_estimate() {
        assert!(overlay(&Picture::default()).dots.is_empty());
        let marked = overlay(&Picture {
            estimate: Some(estimate(51.55, true)),
            ..Picture::default()
        });
        assert_eq!(marked.dots[0].fill, CONVERGED);
        assert_eq!(marked.dots[0].at, Geo::new(51.55, 7.05));
        assert_eq!(marked.polygons.len(), 1);
    }

    #[test]
    fn every_other_emitter_gets_its_own_ellipse_but_the_estimate_only_one() {
        let main = estimate(51.55, false);
        let drawn = overlay(&Picture {
            estimate: Some(main),
            emitters: vec![main, estimate(51.6, false)],
            ..Picture::default()
        });
        assert_eq!(drawn.polygons.len(), 2);
        assert_eq!(other_emitters(None, &[main]).len(), 1);
    }

    #[test]
    fn the_ellipse_closes_and_is_longer_along_its_major_axis() {
        let points = ellipse(&estimate(51.55, false));
        assert!(points.len() > 8);
        let (first, last) = (points[0], points[points.len() - 1]);
        assert!((first.lat - last.lat).abs() < 1e-9 && (first.lon - last.lon).abs() < 1e-9);
        let spans: Vec<f64> = points
            .iter()
            .map(|at| (at.lon - 7.05).hypot(at.lat - 51.55))
            .collect();
        let (low, high) = spans
            .iter()
            .fold((f64::INFINITY, 0.0f64), |(low, high), span| {
                (low.min(*span), high.max(*span))
            });
        assert!(high > low * 1.5);
    }

    #[test]
    fn every_station_that_reported_is_placed() {
        let drawn = overlay(&Picture {
            stations: vec![DfStation {
                station_id: "north".to_owned(),
                lat: 51.5,
                lon: 7.0,
                bearings: 3,
                last_seen: "now".to_owned(),
                ..DfStation::default()
            }],
            ..Picture::default()
        });
        assert_eq!(drawn.dots[0].fill, STATION);
    }

    #[test]
    fn the_nav_target_is_ringed_and_the_leg_to_it_needs_a_position() {
        let drawn = overlay(&Picture {
            from: Some(HOME),
            nav: Some(nav()),
            ..Picture::default()
        });
        assert_eq!(drawn.lines[0].points, [HOME, Geo::new(51.51, 7.02)]);
        assert_eq!(drawn.dots.len(), 1);
        assert!(drawn.dots[0].alpha.abs() < f32::EPSILON);
        assert_eq!(drawn.dots[0].stroke, TRANSMITTER);
        let alone = overlay(&Picture {
            nav: Some(nav()),
            ..Picture::default()
        });
        assert!(alone.lines.is_empty());
        assert_eq!(alone.dots.len(), 1);
    }

    #[test]
    fn every_point_of_a_bistatic_ring_adds_up_to_the_echo_path() {
        let points = bistatic_ring(HOME, MAST, 4.0).expect("a ring");
        assert!(points.len() > 8);
        let baseline = great_circle_km(HOME, MAST);
        for at in &points {
            let total = great_circle_km(HOME, *at) + great_circle_km(MAST, *at);
            assert!((total - (baseline + 4.0)).abs() < 0.05, "{total}");
        }
        let (first, last) = (points[0], points[points.len() - 1]);
        assert!((first.lat - last.lat).abs() < 1e-9 && (first.lon - last.lon).abs() < 1e-9);
        assert!(bistatic_ring(HOME, MAST, 0.0).is_none());
    }

    #[test]
    fn one_contour_per_positive_echo_range() {
        let drawn = overlay(&Picture {
            bistatic: vec![Echoes {
                receiver: HOME,
                illuminator: MAST,
                ranges_km: vec![4.0, 0.0],
            }],
            ..Picture::default()
        });
        assert_eq!(drawn.lines.len(), 1);
        assert_eq!(drawn.lines[0].colour, TRACK);
    }

    #[test]
    fn the_overlay_takes_its_sources_from_wired_event_producers() {
        let mut graph = PatchGraph {
            nodes: vec![
                node("df", "df"),
                node("hunt", "hunt"),
                node("cross", "triangulation"),
                node("radar", "passive_radar"),
                node("filter", "event_filter"),
                node("map", "map"),
            ],
            edges: vec![
                events("df", "map"),
                events("hunt", "filter"),
                events("filter", "map"),
                events("cross", "map"),
                events("radar", "map"),
            ],
        };
        graph.nodes[3].label = Some("Roof".to_owned());
        let found = sources_of(&graph, "map");
        assert_eq!(found.finders, ["df"]);
        assert_eq!(found.hunts, ["hunt"]);
        assert_eq!(found.crossings, ["cross"]);
        assert_eq!(found.radars, ["radar"]);
        assert_eq!(found.labels["df"], "Direction finder");
        assert_eq!(found.labels["radar"], "Roof");
        assert!(sources_of(&graph, "df").is_empty());
    }

    #[test]
    fn no_sources_draw_no_picture() {
        assert!(
            picture(
                &Sources::default(),
                &Held::default(),
                &Book::default(),
                0,
                None
            )
            .is_none()
        );
    }

    #[test]
    fn rays_come_from_finders_and_hunts_with_their_own_positions() {
        let mut held = Held::default();
        held.bearings
            .insert("df".to_owned(), vec![sample("df", 0), sample("df", 10)]);
        held.bearings
            .insert("hunt".to_owned(), vec![sample("hunt", 0)]);
        held.bearings
            .insert("other".to_owned(), vec![sample("other", 0)]);
        let sources = Sources {
            finders: vec!["df".to_owned()],
            hunts: vec!["hunt".to_owned()],
            ..Sources::default()
        };
        let drawn = picture(&sources, &held, &Book::default(), 20, None).expect("a picture");
        assert_eq!(drawn.rays.len(), 3);
        assert!(drawn.rays.iter().all(|ray| ray.from == HOME));
        assert_eq!(counts(Some(&drawn)).bearings, 3);
    }

    #[test]
    fn a_finder_without_a_station_or_heading_is_listed_unplaced() {
        let mut book = Book::default();
        book.readings.insert(
            "df".to_owned(),
            heard(ProcessorReading::Df(DfReading::default()), 0),
        );
        let sources = Sources {
            finders: vec!["df".to_owned(), "quiet".to_owned()],
            labels: HashMap::from([("df".to_owned(), "Mast DF".to_owned())]),
            ..Sources::default()
        };
        let drawn = picture(&sources, &Held::default(), &book, 0, None).expect("a picture");
        assert_eq!(drawn.unplaced, ["Mast DF"]);
        assert!(finder_placed(None));
    }

    #[test]
    fn the_first_estimate_leads_and_every_crossing_adds_emitters_and_stations() {
        let mut book = Book::default();
        let station = DfStation {
            station_id: "roof".to_owned(),
            ..DfStation::default()
        };
        book.fusion.insert(
            "a".to_owned(),
            DfFusionState {
                emitters: vec![estimate(51.7, false)],
                stations: vec![station.clone()],
                ..DfFusionState::default()
            },
        );
        book.fusion.insert(
            "b".to_owned(),
            DfFusionState {
                estimate: Some(estimate(51.55, true)),
                nav: Some(nav()),
                stations: vec![station],
                ..DfFusionState::default()
            },
        );
        let sources = Sources {
            crossings: vec!["a".to_owned(), "b".to_owned(), "gone".to_owned()],
            ..Sources::default()
        };
        let drawn = picture(&sources, &Held::default(), &book, 0, None).expect("a picture");
        assert_eq!(drawn.estimate.map(|found| found.lat), Some(51.55));
        assert_eq!(drawn.nav, Some(nav()));
        assert_eq!(drawn.emitters.len(), 1);
        assert_eq!(drawn.stations.len(), 2);
    }

    #[test]
    fn a_fresh_radar_draws_sites_confirmed_rings_and_fixes_with_trails() {
        let mut book = Book::default();
        book.readings.insert(
            "radar".to_owned(),
            heard(
                ProcessorReading::PassiveRadar(radar_update(true, 500.0)),
                1_000,
            ),
        );
        let mut held = Held::default();
        let trail = vec![Geo::new(51.51, 7.09), Geo::new(51.52, 7.1)];
        held.fix_trails
            .insert("radar".to_owned(), HashMap::from([(1, trail.clone())]));
        let drawn = picture(&radar_sources(), &held, &book, 1_200, None).expect("a picture");
        assert_eq!(
            drawn.sites,
            [Site {
                receiver: HOME,
                transmitter: MAST
            }]
        );
        assert_eq!(drawn.bistatic[0].ranges_km, [4.0]);
        assert_eq!(drawn.fixes.len(), 1);
        assert_eq!(drawn.fixes[0].trail, trail);
        let shown = counts(Some(&drawn));
        assert_eq!((shown.echoes, shown.tracks), (1, 1));
        let layers = overlay(&drawn);
        assert_eq!(layers.polygons.len(), 1);
        assert_eq!(layers.dots.len(), 3);
    }

    #[test]
    fn a_stale_radar_keeps_its_sites_but_drops_echoes_and_tracks() {
        let mut book = Book::default();
        book.readings.insert(
            "radar".to_owned(),
            heard(ProcessorReading::PassiveRadar(radar_update(true, 500.0)), 0),
        );
        let drawn =
            picture(&radar_sources(), &Held::default(), &book, 2_600, None).expect("a picture");
        assert_eq!(drawn.sites.len(), 1);
        assert!(drawn.bistatic.is_empty() && drawn.fixes.is_empty());
        assert!(!radar_stale(0, 500.0, 2_500));
        assert!(radar_stale(0, 500.0, 2_501));
    }

    #[test]
    fn a_radar_without_geometry_is_listed_unplaced() {
        let mut book = Book::default();
        book.readings.insert(
            "radar".to_owned(),
            heard(
                ProcessorReading::PassiveRadar(radar_update(false, 500.0)),
                0,
            ),
        );
        let drawn = picture(&radar_sources(), &Held::default(), &book, 0, None).expect("a picture");
        assert_eq!(drawn.unplaced, ["Roof radar"]);
        assert!(drawn.sites.is_empty());
        assert_eq!(counts(Some(&drawn)).unplaced, ["Roof radar"]);
        assert_eq!(counts(None), Counts::default());
    }

    #[test]
    fn a_time_before_the_epoch_reads_as_zero() {
        assert_eq!(millis_of_time(UNIX_EPOCH + Duration::from_millis(42)), 42);
        assert_eq!(millis_of_time(UNIX_EPOCH - Duration::from_secs(1)), 0);
    }
}
