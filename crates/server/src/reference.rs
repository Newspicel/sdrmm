use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::{Duration, SystemTime},
};

use sdrmm_wire::{Callsign, CatalogSatellite, SATELLITE_CATALOG_URL, TRANSMITTER_URL, Transmitter};

use crate::satellites::catalog;

pub(crate) mod feed;
mod prefixes;

use feed::Source;
pub(crate) use feed::{Feed, Snapshot, fetch};
use prefixes::Prefixes;

const CHECK_EVERY: Duration = Duration::from_secs(10 * 60);
const HOUR: u64 = 3_600;
const PREFIX_URL: &str = "https://www.country-files.com/cty/cty.dat";

const AMATEUR: Source = Source {
    file: "amateur.tle",
    url: SATELLITE_CATALOG_URL,
    params: &[("GROUP", "amateur"), ("FORMAT", "TLE")],
    refresh: Duration::from_secs(12 * HOUR),
    max_bytes: 1024 * 1024,
};

const WEATHER: Source = Source {
    file: "weather.tle",
    url: SATELLITE_CATALOG_URL,
    params: &[("GROUP", "weather"), ("FORMAT", "TLE")],
    refresh: Duration::from_secs(12 * HOUR),
    max_bytes: 1024 * 1024,
};

const TRANSMITTERS: Source = Source {
    file: "transmitters.json",
    url: TRANSMITTER_URL,
    params: &[("format", "json")],
    refresh: Duration::from_secs(24 * HOUR),
    max_bytes: 32 * 1024 * 1024,
};

const PREFIXES: Source = Source {
    file: "cty.dat",
    url: PREFIX_URL,
    params: &[],
    refresh: Duration::from_secs(7 * 24 * HOUR),
    max_bytes: 4 * 1024 * 1024,
};

pub(crate) type Satellites = Vec<CatalogSatellite>;
pub(crate) type Transmitters = HashMap<String, Vec<Transmitter>>;

pub(crate) struct Reference {
    dir: OnceLock<PathBuf>,
    amateur: Feed<Satellites>,
    weather: Feed<Satellites>,
    transmitters: Feed<Transmitters>,
    prefixes: Feed<Prefixes>,
}

impl Default for Reference {
    fn default() -> Self {
        Self {
            dir: OnceLock::new(),
            amateur: Feed::new(AMATEUR, catalog::parse_satellites),
            weather: Feed::new(WEATHER, catalog::parse_satellites),
            transmitters: Feed::new(TRANSMITTERS, catalog::parse_transmitters),
            prefixes: Feed::new(PREFIXES, Prefixes::parse),
        }
    }
}

impl Reference {
    pub(crate) fn attach(&self, dir: PathBuf) {
        if let Err(dir) = self.dir.set(dir) {
            tracing::warn!(dir = %dir.display(), "reference data already has a directory");
        }
    }

    pub(crate) async fn satellites(&self) -> Result<Snapshot<Satellites>, String> {
        let dir = self.dir();
        merged([self.amateur.get(dir).await, self.weather.get(dir).await])
    }

    pub(crate) async fn transmitters(&self) -> Result<Arc<Snapshot<Transmitters>>, String> {
        self.transmitters.get(self.dir()).await
    }

    pub(crate) fn callsign(&self, raw: &str) -> Option<Callsign> {
        self.prefixes.current()?.value.lookup(raw)
    }

    pub(crate) async fn run(self: Arc<Self>) {
        if let Some(dir) = self.dir() {
            load_cached(&self.amateur, dir).await;
            load_cached(&self.weather, dir).await;
            load_cached(&self.transmitters, dir).await;
            load_cached(&self.prefixes, dir).await;
        }
        let mut tick = tokio::time::interval(CHECK_EVERY);
        loop {
            tick.tick().await;
            self.refresh_due().await;
        }
    }

    async fn refresh_due(&self) {
        let dir = self.dir();
        for group in [&self.amateur, &self.weather] {
            if let Some(fresh) = refresh_due(group, dir).await {
                tracing::info!(
                    file = group.file(),
                    satellites = fresh.value.len(),
                    "satellite elements updated"
                );
            }
        }
        if let Some(fresh) = refresh_due(&self.transmitters, dir).await {
            tracing::info!(satellites = fresh.value.len(), "transmitters updated");
        }
        if let Some(fresh) = refresh_due(&self.prefixes, dir).await {
            tracing::info!(version = %fresh.value.version, "callsign prefixes updated");
        }
    }

    fn dir(&self) -> Option<&Path> {
        self.dir.get().map(PathBuf::as_path)
    }

    #[cfg(test)]
    pub(crate) async fn cached_in(dir: &Path) -> Arc<Self> {
        let reference = Self::default();
        reference.attach(dir.to_path_buf());
        load_cached(&reference.prefixes, dir).await;
        Arc::new(reference)
    }
}

async fn load_cached<T: Send + Sync + 'static>(feed: &Feed<T>, dir: &Path) {
    if let Err(error) = feed.load(dir).await {
        tracing::warn!(%error, file = feed.file(), "ignoring cached reference data");
    }
}

async fn refresh_due<T: Send + Sync + 'static>(
    feed: &Feed<T>,
    dir: Option<&Path>,
) -> Option<Arc<Snapshot<T>>> {
    if !feed.due(SystemTime::now()) {
        return None;
    }
    feed.refresh(dir)
        .await
        .inspect_err(|error| {
            tracing::warn!(%error, file = feed.file(), "could not update reference data");
        })
        .ok()
}

fn merged(
    groups: [Result<Arc<Snapshot<Satellites>>, String>; 2],
) -> Result<Snapshot<Satellites>, String> {
    let mut errors = Vec::new();
    let mut seen = HashSet::new();
    let mut value = Vec::new();
    let mut oldest: Option<SystemTime> = None;
    for group in groups {
        match group {
            Ok(group) => {
                oldest = Some(oldest.map_or(group.fetched, |at| at.min(group.fetched)));
                value.extend(
                    group
                        .value
                        .iter()
                        .filter(|satellite| seen.insert(satellite.catalog.clone()))
                        .cloned(),
                );
            }
            Err(error) => errors.push(error),
        }
    }
    oldest
        .map(|fetched| Snapshot { value, fetched })
        .ok_or_else(|| errors.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(catalogs: &[&str], age_s: u64) -> Result<Arc<Snapshot<Satellites>>, String> {
        Ok(Arc::new(Snapshot {
            value: catalogs
                .iter()
                .map(|catalog| CatalogSatellite {
                    name: format!("SAT {catalog}"),
                    catalog: (*catalog).to_owned(),
                    tle: String::new(),
                })
                .collect(),
            fetched: SystemTime::UNIX_EPOCH + Duration::from_secs(1_000 - age_s),
        }))
    }

    #[test]
    fn both_groups_are_searched_once_each_and_dated_by_the_older() {
        let both = merged([group(&["1", "2"], 10), group(&["2", "3"], 50)]).expect("merged");
        let catalogs: Vec<&str> = both.value.iter().map(|s| s.catalog.as_str()).collect();
        assert_eq!(catalogs, ["1", "2", "3"]);
        assert_eq!(
            both.fetched,
            SystemTime::UNIX_EPOCH + Duration::from_secs(950)
        );
    }

    #[test]
    fn one_missing_group_still_answers_but_two_do_not() {
        let one = merged([Err("down".to_owned()), group(&["3"], 0)]).expect("one");
        assert_eq!(one.value.len(), 1);
        let none = merged([Err("a".to_owned()), Err("b".to_owned())]).err();
        assert_eq!(none.as_deref(), Some("a; b"));
    }
}
