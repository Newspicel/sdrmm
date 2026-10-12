use std::collections::{BTreeMap, HashMap, HashSet};

use sdrmm_wire::{
    DecodedRecord, LatLon, PropagationCell, PropagationNode, PropagationPath, PropagationSummary,
    PropagationView,
    geo::distance_m,
    present::{Spot, grid_lat_lon, square_of},
};

pub(crate) const EARTH_RADIUS_KM: f64 = 6_371.0;
pub(crate) const MIN_MUF_PATH_KM: f64 = 500.0;
pub(crate) const OBSERVATION_CAPACITY: usize = 20_000;
pub(crate) const DECAYS_KEPT: f64 = 8.0;
pub(crate) const PATH_CAP: usize = 400;
const BAND_HZ: f64 = 1e5;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Heard {
    pub key: String,
    pub at_ms: i64,
    pub at: String,
    pub callsign: String,
    pub grid: String,
    pub from: LatLon,
    pub freq_hz: f64,
    pub snr_db: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Observation {
    pub heard: Heard,
    pub distance_km: f64,
    pub hops: u32,
    pub muf3000_mhz: Option<f64>,
    pub control: Vec<LatLon>,
}

pub(crate) fn at_ms(at: &str) -> Option<i64> {
    at.parse::<jiff::Timestamp>()
        .ok()
        .map(jiff::Timestamp::as_millisecond)
}

pub(crate) fn heard_of(record: &DecodedRecord, spot: &Spot) -> Option<Heard> {
    Some(Heard {
        key: format!(
            "{}|{}|{}|{}",
            record.at,
            record.origin.as_ref().map_or("", |origin| origin.node.as_str()),
            spot.callsign,
            spot.grid
        ),
        at_ms: at_ms(&record.at)?,
        at: record.at.clone(),
        callsign: spot.callsign.clone(),
        grid: spot.grid.clone(),
        from: grid_lat_lon(&spot.grid)?,
        freq_hz: record.freq_hz,
        snr_db: spot.snr_db,
    })
}

pub(crate) fn great_circle_km(from: LatLon, to: LatLon) -> f64 {
    distance_m(from, to) / 1_000.0
}

pub(crate) fn along_great_circle(from: LatLon, to: LatLon, fraction: f64) -> LatLon {
    let (lat1, lon1) = (from.lat.to_radians(), from.lon.to_radians());
    let (lat2, lon2) = (to.lat.to_radians(), to.lon.to_radians());
    let d = great_circle_km(from, to) / EARTH_RADIUS_KM;
    if d < 1e-9 {
        return from;
    }
    let a = ((1.0 - fraction) * d).sin() / d.sin();
    let b = (fraction * d).sin() / d.sin();
    let x = a * lat1.cos() * lon1.cos() + b * lat2.cos() * lon2.cos();
    let y = a * lat1.cos() * lon1.sin() + b * lat2.cos() * lon2.sin();
    let z = a * lat1.sin() + b * lat2.sin();
    LatLon {
        lat: z.atan2(x.hypot(y)).to_degrees(),
        lon: y.atan2(x).to_degrees(),
    }
}

pub(crate) fn max_hop_km(height_km: f64) -> f64 {
    let r = EARTH_RADIUS_KM;
    2.0 * r * (r / (r + height_km)).min(1.0).acos()
}

pub(crate) fn hop_count(distance_km: f64, height_km: f64) -> u32 {
    let reach = max_hop_km(height_km);
    if reach.is_nan() || reach <= 0.0 || distance_km.is_nan() || distance_km <= 0.0 {
        return 1;
    }
    ((distance_km / reach).ceil() as u32).max(1)
}

pub(crate) fn obliquity_factor(hop_km: f64, height_km: f64) -> f64 {
    let r = EARTH_RADIUS_KM;
    let delta = hop_km / (2.0 * r);
    let denominator = r + height_km - r * delta.cos();
    if denominator.is_nan() || denominator <= 0.0 {
        return 1.0;
    }
    1.0_f64.hypot(r * delta.sin() / denominator)
}

pub(crate) fn muf3000_mhz(freq_hz: f64, distance_km: f64, height_km: f64) -> Option<f64> {
    if !freq_hz.is_finite() || freq_hz <= 0.0 || distance_km < MIN_MUF_PATH_KM {
        return None;
    }
    let hops = hop_count(distance_km, height_km);
    let factor = obliquity_factor(distance_km / f64::from(hops), height_km);
    (factor > 0.0).then(|| freq_hz / 1e6 * obliquity_factor(3_000.0, height_km) / factor)
}

pub(crate) fn control_points(from: LatLon, to: LatLon, hops: u32) -> Vec<LatLon> {
    (0..hops)
        .map(|hop| along_great_circle(from, to, f64::from(2 * hop + 1) / f64::from(2 * hops)))
        .collect()
}

pub(crate) fn observe(heard: &Heard, receiver: LatLon, height_km: f64) -> Observation {
    let distance_km = great_circle_km(receiver, heard.from);
    let hops = hop_count(distance_km, height_km);
    Observation {
        heard: heard.clone(),
        distance_km,
        hops,
        muf3000_mhz: muf3000_mhz(heard.freq_hz, distance_km, height_km),
        control: control_points(receiver, heard.from, hops),
    }
}

pub(crate) fn decay_weight(age_ms: f64, half_life_minutes: u32) -> f64 {
    let half_life_ms = f64::from(half_life_minutes.max(1)) * 60_000.0;
    if age_ms <= 0.0 {
        return 1.0;
    }
    (-age_ms / half_life_ms).exp2()
}

#[derive(Default)]
struct CellSum {
    weight: f64,
    decodes: u32,
    calls: HashSet<String>,
    distances: Vec<f64>,
    best_freq_hz: f64,
    best_snr_db: Option<f32>,
    muf: Option<f64>,
    last: Option<(i64, String)>,
}

impl CellSum {
    fn add(&mut self, observation: &Observation, weight: f64) {
        let heard = &observation.heard;
        self.weight += weight;
        self.decodes += 1;
        self.calls.insert(heard.callsign.clone());
        self.distances.push(observation.distance_km);
        self.best_freq_hz = self.best_freq_hz.max(heard.freq_hz);
        self.best_snr_db = Some(self.best_snr_db.map_or(heard.snr_db, |snr| snr.max(heard.snr_db)));
        if let Some(muf) = observation.muf3000_mhz {
            self.muf = Some(self.muf.map_or(muf, |held| held.max(muf)));
        }
        if self.last.as_ref().is_none_or(|(at, _)| *at < heard.at_ms) {
            self.last = Some((heard.at_ms, heard.at.clone()));
        }
    }

    fn cell(self, key: String, centre: LatLon) -> PropagationCell {
        PropagationCell {
            key,
            latitude: centre.lat,
            longitude: centre.lon,
            weight: self.weight,
            decodes: self.decodes,
            callsigns: self.calls.len() as u32,
            best_freq_hz: self.best_freq_hz,
            best_snr_db: self.best_snr_db.unwrap_or_default(),
            measured_muf3000_mhz: self.muf,
            median_distance_km: median(self.distances),
            last_seen: self.last.map(|(_, at)| at).unwrap_or_default(),
        }
    }
}

fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 1 {
        values[middle]
    } else {
        (values[middle - 1] + values[middle]) / 2.0
    }
}

pub(crate) fn cells(
    observations: &[Observation],
    half_life_minutes: u32,
    now_ms: i64,
) -> Vec<PropagationCell> {
    let mut sums: BTreeMap<String, CellSum> = BTreeMap::new();
    for observation in observations {
        let weight = decay_weight((now_ms - observation.heard.at_ms) as f64, half_life_minutes);
        if weight <= 0.0 {
            continue;
        }
        for point in &observation.control {
            sums.entry(square_of(*point))
                .or_default()
                .add(observation, weight);
        }
    }
    let mut cells: Vec<PropagationCell> = sums
        .into_iter()
        .filter_map(|(key, sum)| {
            let centre = grid_lat_lon(&key)?;
            Some(sum.cell(key, centre))
        })
        .collect();
    cells.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    cells
}

pub(crate) fn paths(
    observations: &[Observation],
    receiver: LatLon,
    half_life_minutes: u32,
    now_ms: i64,
) -> Vec<PropagationPath> {
    let mut newest: HashMap<(String, i64), &Observation> = HashMap::new();
    for observation in observations {
        let band = (observation.heard.freq_hz / BAND_HZ).round() as i64;
        let key = (observation.heard.grid.clone(), band);
        if newest
            .get(&key)
            .is_none_or(|held| held.heard.at_ms < observation.heard.at_ms)
        {
            newest.insert(key, observation);
        }
    }
    let mut kept: Vec<&Observation> = newest.into_values().collect();
    kept.sort_by(|a, b| b.heard.at_ms.cmp(&a.heard.at_ms));
    kept.into_iter()
        .take(PATH_CAP)
        .map(|observation| PropagationPath {
            key: observation.heard.key.clone(),
            from: receiver,
            to: observation.heard.from,
            weight: decay_weight(
                (now_ms - observation.heard.at_ms) as f64,
                half_life_minutes,
            ),
            freq_hz: observation.heard.freq_hz,
        })
        .collect()
}

pub(crate) fn summary(observations: &[Observation]) -> PropagationSummary {
    let mut grids = HashSet::new();
    let mut calls = HashSet::new();
    let mut bands = HashSet::new();
    let mut summary = PropagationSummary {
        decodes: observations.len() as u32,
        ..PropagationSummary::default()
    };
    for observation in observations {
        let heard = &observation.heard;
        grids.insert(heard.grid.as_str());
        calls.insert(heard.callsign.as_str());
        bands.insert((heard.freq_hz / BAND_HZ).round() as i64);
        summary.best_freq_hz = summary.best_freq_hz.max(heard.freq_hz);
        summary.farthest_km = summary.farthest_km.max(observation.distance_km);
        if let Some(muf) = observation.muf3000_mhz {
            summary.best_muf3000_mhz = Some(summary.best_muf3000_mhz.map_or(muf, |best| best.max(muf)));
        }
    }
    summary.grids = grids.len() as u32;
    summary.callsigns = calls.len() as u32;
    summary.bands = bands.len() as u32;
    summary
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Propagation {
    heard: Vec<Heard>,
    keys: HashSet<String>,
    cleared_ms: i64,
    pub settings: PropagationNode,
    pub receiver: Option<LatLon>,
    pub changed: bool,
}

impl Propagation {
    pub(crate) fn hear(&mut self, heard: Heard) {
        if heard.at_ms <= self.cleared_ms || !self.keys.insert(heard.key.clone()) {
            return;
        }
        let at = self
            .heard
            .partition_point(|held| held.at_ms <= heard.at_ms);
        self.heard.insert(at, heard);
        if self.heard.len() > OBSERVATION_CAPACITY {
            let gone = self.heard.remove(0);
            self.keys.remove(&gone.key);
        }
        self.changed = true;
    }

    pub(crate) fn holds_any(&self) -> bool {
        !self.heard.is_empty()
    }

    pub(crate) fn clear(&mut self, now_ms: i64) {
        self.heard.clear();
        self.keys.clear();
        self.cleared_ms = now_ms;
        self.changed = true;
    }

    fn live(&self, now_ms: i64) -> impl Iterator<Item = &Heard> {
        let kept = f64::from(self.settings.half_life_minutes) * 60_000.0 * DECAYS_KEPT;
        let cutoff = self.cleared_ms.max(now_ms - kept as i64);
        self.heard.iter().filter(move |heard| heard.at_ms >= cutoff)
    }

    pub(crate) fn view(&self, now_ms: i64) -> PropagationView {
        let Some(receiver) = self.receiver else {
            return PropagationView::default();
        };
        let height = f64::from(self.settings.reflection_height_km);
        let half_life = self.settings.half_life_minutes;
        let observations: Vec<Observation> = self
            .live(now_ms)
            .map(|heard| observe(heard, receiver, height))
            .collect();
        PropagationView {
            cells: cells(&observations, half_life, now_ms),
            paths: if self.settings.show_paths {
                paths(&observations, receiver, half_life, now_ms)
            } else {
                Vec::new()
            },
            summary: summary(&observations),
            receiver: Some(receiver),
        }
    }
}

#[cfg(test)]
mod tests;
