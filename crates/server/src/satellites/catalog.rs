use std::{collections::HashMap, time::SystemTime};

use sdrmm_orbit::Tle;
use sdrmm_wire::{
    CatalogSatellite, MAX_CATALOG_RESULTS, MAX_SATELLITE_QUERY_LEN, MAX_TRANSMITTER_ID_LEN,
    SATELLITE_CATALOG_SOURCE, SATELLITE_CATALOG_URL, SatelliteCatalogResponse, TRANSMITTER_SOURCE,
    Transmitter, TransmittersResponse,
};
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::reference::{Reference, Satellites, Snapshot, Transmitters, fetch};

const FRESH_FOR: std::time::Duration = std::time::Duration::from_secs(2 * 3_600);
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_CACHED: usize = 64;

#[derive(Default)]
pub(crate) struct Catalog {
    searches: Mutex<HashMap<String, Snapshot<Satellites>>>,
}

#[derive(Debug, Deserialize)]
struct SatnogsTransmitter {
    #[serde(default)]
    uuid: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    alive: bool,
    #[serde(default)]
    status: String,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    downlink_low: Option<f64>,
    #[serde(default)]
    uplink_low: Option<f64>,
    #[serde(default)]
    norad_cat_id: Option<u64>,
}

impl Catalog {
    pub(crate) async fn search(
        &self,
        reference: &Reference,
        query: &str,
    ) -> Result<SatelliteCatalogResponse, String> {
        let query = checked_query(query)?;
        let listed = reference.satellites().await;
        if let Ok(listed) = &listed {
            let found = matching(&listed.value, &query);
            if !found.is_empty() || query.is_empty() {
                return Ok(satellite_response(found, listed));
            }
        }
        if query.is_empty() {
            return listed.map(|listed| satellite_response(Vec::new(), &listed));
        }
        self.search_live(query).await
    }

    async fn search_live(&self, query: String) -> Result<SatelliteCatalogResponse, String> {
        if let Some(fresh) = self.cached(&query).await {
            return Ok(fresh);
        }
        let params: [(&str, &str); 2] = if query.bytes().all(|b| b.is_ascii_digit()) {
            [("CATNR", query.as_str()), ("FORMAT", "TLE")]
        } else {
            [("NAME", query.as_str()), ("FORMAT", "TLE")]
        };
        let body = fetch(SATELLITE_CATALOG_URL, &params, MAX_BODY_BYTES).await?;
        let found: Snapshot<Satellites> = Snapshot {
            value: element_sets(&String::from_utf8_lossy(&body))
                .into_iter()
                .take(MAX_CATALOG_RESULTS)
                .collect(),
            fetched: SystemTime::now(),
        };
        let response = satellite_response(found.value.clone(), &found);
        self.remember(query, found).await;
        Ok(response)
    }

    async fn cached(&self, query: &str) -> Option<SatelliteCatalogResponse> {
        self.searches
            .lock()
            .await
            .get(query)
            .filter(|found| fresh(found))
            .map(|found| satellite_response(found.value.clone(), found))
    }

    async fn remember(&self, query: String, found: Snapshot<Satellites>) {
        let mut searches = self.searches.lock().await;
        searches.retain(|_, found| fresh(found));
        if searches.len() >= MAX_CACHED {
            searches.clear();
        }
        searches.insert(query, found);
    }

    pub(crate) async fn transmitters(
        &self,
        reference: &Reference,
        catalog: &str,
    ) -> Result<TransmittersResponse, String> {
        if catalog.is_empty() || catalog.len() > 9 || !catalog.bytes().all(|b| b.is_ascii_digit()) {
            return Err("a catalog number is digits only".to_owned());
        }
        let listed = reference.transmitters().await?;
        Ok(TransmittersResponse {
            transmitters: listed
                .value
                .get(catalog.trim_start_matches('0'))
                .cloned()
                .unwrap_or_default(),
            source: TRANSMITTER_SOURCE.to_owned(),
            fetched: listed.fetched_unix(),
        })
    }
}

fn fresh(found: &Snapshot<Satellites>) -> bool {
    found.fetched.elapsed().is_ok_and(|age| age < FRESH_FOR)
}

fn satellite_response(
    satellites: Satellites,
    snapshot: &Snapshot<Satellites>,
) -> SatelliteCatalogResponse {
    SatelliteCatalogResponse {
        satellites,
        source: SATELLITE_CATALOG_SOURCE.to_owned(),
        fetched: snapshot.fetched_unix(),
    }
}

fn matching(listed: &[CatalogSatellite], query: &str) -> Satellites {
    let catalog = query.trim_start_matches('0');
    let numeric = !query.is_empty() && query.bytes().all(|b| b.is_ascii_digit());
    listed
        .iter()
        .filter(|satellite| {
            if numeric {
                satellite.catalog.trim_start_matches('0') == catalog
            } else {
                satellite.name.to_uppercase().contains(query)
            }
        })
        .take(MAX_CATALOG_RESULTS)
        .cloned()
        .collect()
}

pub(crate) fn parse_satellites(body: &[u8]) -> Result<Satellites, String> {
    let text = std::str::from_utf8(body).map_err(|_| "element sets that are not text")?;
    let sets = element_sets(text);
    if sets.is_empty() {
        return Err("no element sets".to_owned());
    }
    Ok(sets)
}

pub(crate) fn parse_transmitters(body: &[u8]) -> Result<Transmitters, String> {
    let listed: Vec<SatnogsTransmitter> = serde_json::from_slice(body)
        .map_err(|error| format!("a broken transmitter list: {error}"))?;
    let mut by_satellite: HashMap<String, Vec<SatnogsTransmitter>> = HashMap::new();
    for entry in listed {
        if let Some(catalog) = entry.norad_cat_id {
            by_satellite
                .entry(catalog.to_string())
                .or_default()
                .push(entry);
        }
    }
    let index: Transmitters = by_satellite
        .into_iter()
        .map(|(catalog, listed)| (catalog, transmitters(listed)))
        .filter(|(_, kept)| !kept.is_empty())
        .collect();
    if index.is_empty() {
        return Err("an empty transmitter list".to_owned());
    }
    Ok(index)
}

fn checked_query(query: &str) -> Result<String, String> {
    let query = query.trim();
    if query.len() > MAX_SATELLITE_QUERY_LEN {
        return Err(format!(
            "a search is at most {MAX_SATELLITE_QUERY_LEN} characters"
        ));
    }
    if !query
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || " -()/._+".contains(c))
    {
        return Err("a search holds letters, digits and - ( ) / . _ + only".to_owned());
    }
    Ok(query.to_uppercase())
}

fn element_sets(text: &str) -> Vec<CatalogSatellite> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect();
    lines
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|set| {
            let text = set.join("\n");
            let tle = Tle::parse(&text).ok()?;
            Some(CatalogSatellite {
                name: tle.label().to_owned(),
                catalog: tle.catalog,
                tle: text,
            })
        })
        .collect()
}

fn transmitters(listed: Vec<SatnogsTransmitter>) -> Vec<Transmitter> {
    let frequency = |hz: Option<f64>| hz.filter(|hz| hz.is_finite() && *hz > 0.0);
    let mut out: Vec<Transmitter> = listed
        .into_iter()
        .filter(|entry| entry.status != "invalid")
        .filter(|entry| !entry.uuid.is_empty() && entry.uuid.len() <= MAX_TRANSMITTER_ID_LEN)
        .filter(|entry| frequency(entry.downlink_low).is_some())
        .map(|entry| Transmitter {
            id: entry.uuid,
            description: entry.description,
            mode: entry.mode.filter(|mode| !mode.is_empty()),
            downlink_hz: frequency(entry.downlink_low),
            uplink_hz: frequency(entry.uplink_low),
            alive: entry.alive && entry.status == "active",
        })
        .collect();
    out.sort_by_key(|transmitter| !transmitter.alive);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = "ISS (ZARYA)
1 25544U 98067A   24001.50000000  .00016717  00000-0  30306-3 0  9999
2 25544  51.6416 247.4627 0006703 130.5360 325.0288 15.50377579432041
BROKEN
1 25544U 98067A   24001.50000000  .00016717  00000-0  30306-3 0  9990
2 25544  51.6416 247.4627 0006703 130.5360 325.0288 15.50377579432041
";

    #[test]
    fn a_feed_keeps_only_sets_that_check_out() {
        let sets = element_sets(FEED);
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].name, "ISS (ZARYA)");
        assert_eq!(sets[0].catalog, "25544");
        assert!(Tle::parse(&sets[0].tle).is_ok());
    }

    #[test]
    fn the_cached_list_is_searched_by_name_or_number() {
        let listed = element_sets(FEED);
        assert_eq!(matching(&listed, "ZARYA").len(), 1);
        assert_eq!(matching(&listed, "025544").len(), 1);
        assert_eq!(matching(&listed, "").len(), 1);
        assert!(matching(&listed, "NOAA").is_empty());
        assert!(matching(&listed, "2554").is_empty());
    }

    #[test]
    fn a_download_without_element_sets_is_refused() {
        assert!(parse_satellites(FEED.as_bytes()).is_ok());
        assert!(parse_satellites(b"No GP data found").is_err());
    }

    #[test]
    fn transmitters_are_indexed_by_catalog_number() {
        let index = parse_transmitters(
            br#"[
                {"uuid":"a","description":"Beacon","alive":true,"status":"active","downlink_low":145800000,"norad_cat_id":25544},
                {"uuid":"b","description":"Other","alive":true,"status":"active","downlink_low":437800000,"norad_cat_id":7530},
                {"uuid":"c","description":"Orphan","alive":true,"status":"active","downlink_low":437800000}
            ]"#,
        )
        .expect("parses");
        assert_eq!(index.len(), 2);
        assert_eq!(index["25544"][0].id, "a");
        assert!(parse_transmitters(b"[]").is_err());
        assert!(parse_transmitters(b"<html>").is_err());
    }

    #[test]
    fn nothing_found_is_an_empty_list() {
        assert!(element_sets("No GP data found\n").is_empty());
    }

    #[test]
    fn searches_refuse_what_could_not_be_a_name() {
        assert!(checked_query("ISS").is_ok());
        assert!(checked_query("AO-91 (FOX-1B)").is_ok());
        assert!(checked_query("a&b=c").is_err());
        assert!(checked_query(&"X".repeat(MAX_SATELLITE_QUERY_LEN + 1)).is_err());
    }

    #[test]
    fn live_transmitters_come_first_and_invalid_ones_are_dropped() {
        let listed: Vec<SatnogsTransmitter> = serde_json::from_str(
            r#"[
                {"uuid":"a","description":"Old beacon","alive":false,"status":"inactive","downlink_low":145800000},
                {"uuid":"b","description":"Bad","alive":true,"status":"invalid","downlink_low":1},
                {"uuid":"c","description":"FM voice","alive":true,"status":"active","mode":"FM","downlink_low":437800000,"uplink_low":145990000},
                {"uuid":"d","description":"No downlink","alive":true,"status":"active","downlink_low":null}
            ]"#,
        )
        .expect("parses");
        let kept = transmitters(listed);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].id, "c");
        assert_eq!(kept[0].description, "FM voice");
        assert_eq!(kept[0].uplink_hz, Some(145_990_000.0));
        assert!(!kept[1].alive);
    }
}
