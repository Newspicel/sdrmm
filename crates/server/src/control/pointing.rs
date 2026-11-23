use std::{cmp::Ordering, collections::HashMap};

use sdrmm_wire::SkyTrack;

/// The controller a pointed target follows, and the sky it asks for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Lead {
    pub(crate) from: String,
    pub(crate) sky: SkyTrack,
}

#[derive(Default)]
struct Contest {
    bids: Vec<Lead>,
    lead: Option<String>,
}

/// What every wired controller asks of each target it points. The best pass in view leads and
/// keeps the target until it sets: the one that climbs highest has the shortest path and the
/// strongest signal. A pass always beats a source with none, such as an aim or a sky scan. With
/// nothing in view, the first to rise leads, so the antenna waits at its rise.
#[derive(Default)]
pub(super) struct Pointing {
    contests: HashMap<String, Contest>,
}

impl Pointing {
    pub(super) fn bid(&mut self, target: &str, from: &str, sky: &SkyTrack) -> Option<Lead> {
        let contest = self.contests.entry(target.to_owned()).or_default();
        let bid = Lead {
            from: from.to_owned(),
            sky: sky.clone(),
        };
        match contest.bids.iter_mut().find(|held| held.from == from) {
            Some(held) => *held = bid,
            None => contest.bids.push(bid),
        }
        contest.choose()
    }

    pub(super) fn withdraw(&mut self, target: &str, from: &str) -> Option<Lead> {
        let contest = self.contests.get_mut(target)?;
        contest.bids.retain(|bid| bid.from != from);
        let lead = contest.choose();
        if contest.bids.is_empty() {
            self.contests.remove(target);
        }
        lead
    }

    pub(super) fn retain(&mut self, live: impl Fn(&str) -> bool) {
        self.contests.retain(|target, _| live(target));
    }
}

impl Contest {
    fn choose(&mut self) -> Option<Lead> {
        let held = self
            .lead
            .as_deref()
            .and_then(|from| self.bids.iter().find(|bid| bid.from == from))
            .filter(|bid| bid.sky.visible && bid.sky.next_pass.is_some());
        let lead = held
            .or_else(|| {
                self.bids
                    .iter()
                    .filter(|bid| bid.sky.visible)
                    .max_by(|a, b| better_in_view(a, b))
            })
            .or_else(|| self.bids.iter().min_by(|a, b| sooner_to_rise(a, b)))
            .cloned();
        self.lead = lead.as_ref().map(|lead| lead.from.clone());
        lead
    }
}

fn better_in_view(a: &Lead, b: &Lead) -> Ordering {
    a.sky
        .next_pass
        .is_some()
        .cmp(&b.sky.next_pass.is_some())
        .then(height(&a.sky).total_cmp(&height(&b.sky)))
        .then(b.from.cmp(&a.from))
}

fn sooner_to_rise(a: &Lead, b: &Lead) -> Ordering {
    rises(&a.sky)
        .cmp(&rises(&b.sky))
        .then(height(&b.sky).total_cmp(&height(&a.sky)))
        .then(a.from.cmp(&b.from))
}

/// How high the pass climbs, or for a source with no pass, how high it points now.
fn height(sky: &SkyTrack) -> f64 {
    sky.next_pass
        .map_or(sky.look.elevation_deg, |pass| pass.max_elevation_deg)
}

fn rises(sky: &SkyTrack) -> i64 {
    sky.next_pass.and_then(|pass| pass.aos).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests;
