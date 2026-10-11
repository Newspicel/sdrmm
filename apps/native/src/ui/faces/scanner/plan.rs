use std::{cell::RefCell, collections::BTreeMap, sync::Arc};

use sdrmm_wire::{
    bandplan::{BandChannel, BandPlan, BandRegionsResponse},
    scan::{ScanRange, ScanSettings},
};
use zgui::{prelude::*, reactive::RenderEffect};

use crate::store::Store;

#[derive(Clone, Debug, PartialEq)]
pub struct ScanPreset {
    pub id: String,
    pub name: String,
    pub ranges: Vec<ScanRange>,
    pub frequencies: Vec<f64>,
}

struct Gathered {
    name: String,
    start: f64,
    stop: f64,
    step: Option<f64>,
    channels: BTreeMap<u64, BandChannel>,
}

fn gather(plan: &BandPlan) -> Vec<Gathered> {
    let mut found: Vec<Gathered> = Vec::new();
    for allocation in &plan.allocations {
        let step = allocation.channel_step_hz.filter(|step| *step > 0.0);
        let tunable = !allocation.channels.is_empty() || step.is_some();
        if allocation.aliases.is_empty() || !tunable {
            continue;
        }
        let at = match found.iter().position(|held| held.name == allocation.name) {
            Some(at) => at,
            None => {
                found.push(Gathered {
                    name: allocation.name.clone(),
                    start: allocation.start_hz,
                    stop: allocation.stop_hz,
                    step,
                    channels: BTreeMap::new(),
                });
                found.len() - 1
            }
        };
        let held = &mut found[at];
        held.start = held.start.min(allocation.start_hz);
        held.stop = held.stop.max(allocation.stop_hz);
        for channel in &allocation.channels {
            held.channels.insert(channel.hz.to_bits(), channel.clone());
        }
    }
    found.sort_by(|a, b| a.start.total_cmp(&b.start));
    found
}

#[must_use]
pub fn scan_presets(plan: Option<&BandPlan>) -> Vec<ScanPreset> {
    let Some(plan) = plan else {
        return Vec::new();
    };
    gather(plan)
        .into_iter()
        .map(|held| {
            let mut frequencies: Vec<f64> =
                held.channels.values().map(|channel| channel.hz).collect();
            frequencies.sort_by(f64::total_cmp);
            let ranges = match held.step {
                Some(step) if frequencies.is_empty() => vec![ScanRange {
                    start_hz: held.start,
                    stop_hz: held.stop,
                    step_hz: step,
                }],
                _ => Vec::new(),
            };
            ScanPreset {
                id: format!("{}:{}", held.name, held.start),
                name: held.name,
                ranges,
                frequencies,
            }
        })
        .collect()
}

pub fn apply_preset(settings: &mut ScanSettings, preset: &ScanPreset) {
    settings.ranges.clone_from(&preset.ranges);
    settings.frequencies.clone_from(&preset.frequencies);
}

#[must_use]
pub fn preset_in<'a>(settings: &ScanSettings, presets: &'a [ScanPreset]) -> Option<&'a ScanPreset> {
    presets.iter().find(|preset| {
        preset.ranges == settings.ranges && preset.frequencies == settings.frequencies
    })
}

#[must_use]
pub fn channel_name(plan: Option<&BandPlan>, hz: f64) -> Option<String> {
    plan?
        .allocations
        .iter()
        .flat_map(|allocation| allocation.channels.iter())
        .find(|channel| (channel.hz - hz).abs() < 1.0)
        .map(|channel| channel.name.clone())
}

thread_local! {
    static PLANS: RefCell<Vec<(String, Arc<BandPlan>)>> = const { RefCell::new(Vec::new()) };
    static DEFAULT_REGION: RefCell<Option<String>> = const { RefCell::new(None) };
}

async fn region_of(store: Store, chosen: Option<String>) -> Option<String> {
    if chosen.is_some() {
        return chosen;
    }
    if let Some(known) = DEFAULT_REGION.with(|region| region.borrow().clone()) {
        return Some(known);
    }
    match store
        .api()
        .get::<BandRegionsResponse>("/api/bandplan/regions")
        .await
    {
        Ok(listed) => {
            DEFAULT_REGION
                .with(|region| *region.borrow_mut() = Some(listed.default_region.clone()));
            Some(listed.default_region)
        }
        Err(error) => {
            tracing::debug!(%error, "no band regions");
            None
        }
    }
}

pub fn band_plan(store: Store) -> RwSignal<Option<Arc<BandPlan>>> {
    let plan = RwSignal::new(None::<Arc<BandPlan>>);
    let chosen = Memo::new(move |_| store.settings.with(|settings| settings.band_region.clone()));
    let loading = RenderEffect::new(move |_| {
        let chosen = chosen.get();
        zgui::task::spawn_local(async move {
            let Some(region) = region_of(store, chosen).await else {
                return;
            };
            let cached = PLANS.with(|plans| {
                plans
                    .borrow()
                    .iter()
                    .find(|(id, _)| *id == region)
                    .map(|(_, plan)| Arc::clone(plan))
            });
            if let Some(cached) = cached {
                plan.try_set(Some(cached));
                return;
            }
            let path = format!("/api/bandplan/regions/{region}");
            match store.api().get::<BandPlan>(&path).await {
                Ok(fetched) => {
                    let fetched = Arc::new(fetched);
                    PLANS.with(|plans| plans.borrow_mut().push((region, Arc::clone(&fetched))));
                    plan.try_set(Some(fetched));
                }
                Err(error) => tracing::debug!(%error, "no band plan"),
            }
        });
    });
    on_cleanup_local(move || drop(loading));
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(allocations: serde_json::Value) -> BandPlan {
        serde_json::from_value(serde_json::json!({
            "region": { "id": "de", "name": "Germany", "itu_region": "r1", "layers": ["world"], "overlays": [] },
            "layers": [],
            "allocations": allocations,
            "lanes": []
        }))
        .expect("a band plan")
    }

    fn allocation(
        name: &str,
        start: f64,
        stop: f64,
        extra: serde_json::Value,
    ) -> serde_json::Value {
        let mut held = serde_json::json!({
            "id": name, "layer": "world", "start_hz": start, "stop_hz": stop,
            "service": "other", "name": name, "official_name": ""
        });
        if let (Some(held), Some(extra)) = (held.as_object_mut(), extra.as_object()) {
            held.extend(extra.clone());
        }
        held
    }

    fn sample() -> BandPlan {
        plan(serde_json::json!([
            allocation(
                "PMR446",
                446.1e6,
                446.2e6,
                serde_json::json!({
                    "aliases": ["pmr"],
                    "channels": [{ "name": "PMR 2", "hz": 446018750.0 }, { "name": "PMR 1", "hz": 446006250.0 }]
                })
            ),
            allocation(
                "Airband",
                118e6,
                137e6,
                serde_json::json!({
                    "aliases": ["air"], "channel_step_hz": 8333.0
                })
            ),
            allocation(
                "Quiet",
                1e6,
                2e6,
                serde_json::json!({ "channel_step_hz": 1000.0 })
            ),
            allocation("Broad", 3e6, 4e6, serde_json::json!({ "aliases": ["b"] }))
        ]))
    }

    #[test]
    fn presets_come_from_named_tunable_bands_in_frequency_order() {
        let presets = scan_presets(Some(&sample()));
        let names: Vec<_> = presets.iter().map(|preset| preset.name.as_str()).collect();
        assert_eq!(names, vec!["Airband", "PMR446"]);
        assert_eq!(
            presets[0].ranges,
            vec![ScanRange {
                start_hz: 118e6,
                stop_hz: 137e6,
                step_hz: 8333.0
            }]
        );
        assert!(presets[0].frequencies.is_empty());
        assert!(presets[1].ranges.is_empty());
        assert_eq!(presets[1].frequencies, vec![446_006_250.0, 446_018_750.0]);
        assert!(scan_presets(None).is_empty());
    }

    #[test]
    fn a_preset_is_recognised_only_while_the_scan_matches_it() {
        let presets = scan_presets(Some(&sample()));
        let mut settings = ScanSettings::for_channel(1);
        assert_eq!(preset_in(&settings, &presets), None);
        apply_preset(&mut settings, &presets[1]);
        assert_eq!(
            preset_in(&settings, &presets).map(|preset| preset.name.as_str()),
            Some("PMR446")
        );
        settings.frequencies.pop();
        assert_eq!(preset_in(&settings, &presets), None);
    }

    #[test]
    fn a_listed_frequency_reads_its_channel_name() {
        let plan = sample();
        assert_eq!(
            channel_name(Some(&plan), 446_006_250.4).as_deref(),
            Some("PMR 1")
        );
        assert_eq!(channel_name(Some(&plan), 446_000_000.0), None);
        assert_eq!(channel_name(None, 446_006_250.0), None);
    }
}
