use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError, Weak},
    time::Duration,
};

use sdrmm_engine::Engine;
use sdrmm_wire::{EVENTS_PORT, PortActivity, ServerEvent};
use tokio::{
    sync::watch,
    time::{MissedTickBehavior, interval},
};

use crate::{events::RouteOutcome, resolved::Resolved};

const PUBLISH_EVERY: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Counts {
    ports: BTreeMap<(String, String), PortActivity>,
    changed: bool,
    unattributed: u64,
}

#[derive(Default)]
pub(crate) struct Activity {
    counts: Mutex<Counts>,
}

impl Activity {
    fn lock(&self) -> MutexGuard<'_, Counts> {
        self.counts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn routed(&self, source: Option<&str>, outcome: RouteOutcome) {
        let Some(source) = source else {
            self.unattributed();
            return;
        };
        let mut counts = self.lock();
        counts.changed = true;
        let port = port_of(&mut counts, source, EVENTS_PORT);
        port.emitted += 1;
        match outcome {
            RouteOutcome::Delivered => port.delivered += 1,
            RouteOutcome::Ignored => port.ignored += 1,
            RouteOutcome::Unrouted | RouteOutcome::Unattributed => port.unrouted += 1,
        }
    }

    pub(crate) fn lost(&self, node: &str, port: &str, count: u64) {
        if count == 0 {
            return;
        }
        let mut counts = self.lock();
        counts.changed = true;
        port_of(&mut counts, node, port).lost += count;
    }

    fn unattributed(&self) {
        let mut counts = self.lock();
        counts.unattributed += 1;
        if counts.unattributed == 1 {
            tracing::error!("a decoded record came without its node and reaches nothing");
        }
    }

    #[cfg(test)]
    pub(crate) fn unattributed_count(&self) -> u64 {
        self.lock().unattributed
    }

    #[cfg(test)]
    pub(crate) fn port(&self, node: &str, port: &str) -> PortActivity {
        self.lock()
            .ports
            .get(&(node.to_owned(), port.to_owned()))
            .cloned()
            .unwrap_or_default()
    }

    fn retain(&self, keep: impl Fn(&str) -> bool) {
        let mut counts = self.lock();
        let before = counts.ports.len();
        counts.ports.retain(|(node, _), _| keep(node));
        if counts.ports.len() != before {
            counts.changed = true;
        }
    }

    fn take_changed(&self) -> Option<Vec<PortActivity>> {
        let mut counts = self.lock();
        if !std::mem::take(&mut counts.changed) {
            return None;
        }
        Some(counts.ports.values().cloned().collect())
    }
}

fn port_of<'a>(counts: &'a mut Counts, node: &str, port: &str) -> &'a mut PortActivity {
    counts
        .ports
        .entry((node.to_owned(), port.to_owned()))
        .or_insert_with(|| PortActivity {
            node: node.to_owned(),
            port: port.to_owned(),
            ..PortActivity::default()
        })
}

pub(crate) async fn run(
    engine: Weak<Engine>,
    activity: Arc<Activity>,
    mut resolved: watch::Receiver<Arc<Resolved>>,
) {
    let mut tick = interval(PUBLISH_EVERY);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            changed = resolved.changed() => {
                if changed.is_err() {
                    return;
                }
                let current = resolved.borrow_and_update().clone();
                activity.retain(|node| current.patch.node(node).is_some());
            }
            _ = tick.tick() => {
                let Some(engine) = engine.upgrade() else {
                    return;
                };
                if let Some(ports) = activity.take_changed() {
                    engine.emit_event(ServerEvent::NodeActivity { ports });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_count_on_the_source_events_port() {
        let activity = Activity::default();

        activity.routed(Some("adsb"), RouteOutcome::Delivered);
        activity.routed(Some("adsb"), RouteOutcome::Ignored);
        activity.routed(Some("adsb"), RouteOutcome::Unrouted);
        activity.lost("map", EVENTS_PORT, 3);

        let port = activity.port("adsb", EVENTS_PORT);
        assert_eq!(
            (port.emitted, port.delivered, port.ignored, port.unrouted),
            (3, 1, 1, 1)
        );
        assert_eq!(activity.port("map", EVENTS_PORT).lost, 3);
        let published = activity.take_changed().expect("counts changed");
        assert_eq!(published.len(), 2);
        assert!(
            activity.take_changed().is_none(),
            "published once per change"
        );
    }

    #[test]
    fn a_record_without_its_node_is_counted_apart() {
        let activity = Activity::default();

        activity.routed(None, RouteOutcome::Unattributed);
        activity.routed(None, RouteOutcome::Unattributed);

        assert_eq!(activity.unattributed_count(), 2);
        assert!(activity.take_changed().is_none());
    }

    #[test]
    fn nodes_that_left_lose_their_counts() {
        let activity = Activity::default();
        activity.routed(Some("gone"), RouteOutcome::Unrouted);
        activity.take_changed();

        activity.retain(|node| node != "gone");

        assert_eq!(activity.take_changed(), Some(Vec::new()));
    }
}
