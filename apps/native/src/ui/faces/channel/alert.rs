use std::{collections::HashMap, sync::Arc, time::Duration};

use sdrmm_wire::workspace::PatchApplyReport;
use zgui::prelude::*;

use crate::{
    store::Store,
    ui::kit_channel::{CLOSE_ICON, icon},
};

pub const WIRE_REFUSAL_MS: i64 = 6_000;
pub const MAX_REFUSALS: usize = 8;
pub const NOT_CONNECTED: &str = "radio not connected";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusalSource {
    Apply,
    Wire,
    Action,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub reason: String,
    pub source: RefusalSource,
    pub at_ms: i64,
}

impl Refusal {
    #[must_use]
    pub fn expired(&self, now_ms: i64) -> bool {
        self.source == RefusalSource::Wire && now_ms - self.at_ms > WIRE_REFUSAL_MS
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Refusals {
    by_node: HashMap<String, Vec<Refusal>>,
}

impl Refusals {
    pub fn flag(&mut self, node: &str, refusal: Refusal) {
        let at = refusal.at_ms;
        let list = self.by_node.entry(node.to_owned()).or_default();
        list.retain(|held| !held.expired(at));
        list.push(refusal);
        let extra = list.len().saturating_sub(MAX_REFUSALS);
        list.drain(..extra);
    }

    pub fn absorb_report(&mut self, report: &PatchApplyReport, at_ms: i64) {
        for list in self.by_node.values_mut() {
            list.retain(|held| held.source != RefusalSource::Apply);
        }
        self.by_node.retain(|_, list| !list.is_empty());
        let refused = report
            .refused
            .iter()
            .map(|refusal| (refusal.node.as_str(), refusal.reason.clone()));
        let absent = report
            .absent
            .iter()
            .map(|node| (node.as_str(), NOT_CONNECTED.to_owned()));
        for (node, reason) in refused.chain(absent).collect::<Vec<_>>() {
            self.flag(
                node,
                Refusal {
                    reason,
                    source: RefusalSource::Apply,
                    at_ms,
                },
            );
        }
    }

    pub fn dismiss(&mut self, node: &str) {
        self.by_node.remove(node);
    }

    #[must_use]
    pub fn visible(&self, node: &str, now_ms: i64) -> Option<Refusal> {
        self.by_node
            .get(node)?
            .iter()
            .filter(|refusal| !refusal.expired(now_ms))
            .max_by_key(|refusal| refusal.at_ms)
            .cloned()
    }

    #[must_use]
    pub fn wire_shown_until(&self, node: &str) -> Option<i64> {
        self.by_node
            .get(node)?
            .iter()
            .filter(|refusal| refusal.source == RefusalSource::Wire)
            .map(|refusal| refusal.at_ms + WIRE_REFUSAL_MS)
            .max()
    }
}

fn now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

impl Store {
    pub fn flag_refusal(self, node: &str, reason: impl Into<String>, source: RefusalSource) {
        let refusal = Refusal {
            reason: reason.into(),
            source,
            at_ms: now_ms(),
        };
        self.refusals
            .update(|held| Arc::make_mut(held).flag(node, refusal));
    }

    pub fn fail_on(self, node: &str, context: &str, error: &anyhow::Error) {
        self.flag_refusal(node, format!("{context}: {error}"), RefusalSource::Action);
        self.fail(context, error);
    }

    pub fn refusals_from_report(self, report: &PatchApplyReport) {
        let at = now_ms();
        self.refusals
            .update(|held| Arc::make_mut(held).absorb_report(report, at));
    }

    pub fn dismiss_refusals(self, node: &str) {
        self.refusals
            .update(|held| Arc::make_mut(held).dismiss(node));
    }
}

fn wake_at(until: Option<i64>, now: RwSignal<i64>) -> Option<zgui::view::TimeoutHandle> {
    let wait = until? - now_ms() + 1;
    let clock = Timers::current()?;
    let wait = Duration::from_millis(u64::try_from(wait.max(0)).unwrap_or_default());
    Some(clock.set_timeout(wait, move || now.set(now_ms())))
}

pub fn face_alert(store: Store, node: String) -> impl IntoView {
    let now = RwSignal::new(now_ms());
    let held = StoredValue::new_local(None::<zgui::view::TimeoutHandle>);
    let until = {
        let node = node.clone();
        Memo::new(move |_| store.refusals.with(|all| all.wire_shown_until(&node)))
    };
    let timer = zgui::reactive::RenderEffect::new(move |_| {
        held.set_value(wake_at(until.get(), now));
    });
    on_cleanup_local(move || drop(timer));
    let shown = {
        let node = node.clone();
        Memo::new(move |_| {
            let at = now.get();
            store.refusals.with(|all| all.visible(&node, at))
        })
    };
    move || {
        let refusal = shown.get()?;
        let node = node.clone();
        let reason = refusal.reason;
        Some(view! {
            row(class = "kc-alert", a11y:role = Role::Alert) {
                text(class = "kc-alert__text") {{reason}}
                control(
                    class = "kc-icon",
                    tabindex = Focus::Sequential,
                    a11y:role = Role::Button,
                    a11y:label = "Dismiss",
                    on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                    on:click:stop = move |_| store.dismiss_refusals(&node)
                ) {
                    {icon(CLOSE_ICON, "kc-glyph kc-glyph--sm")}
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::workspace::PatchRefusal;

    use super::*;

    fn refusal(source: RefusalSource, at_ms: i64) -> Refusal {
        Refusal {
            reason: format!("{source:?} at {at_ms}"),
            source,
            at_ms,
        }
    }

    fn report(refused: &[(&str, &str)], absent: &[&str]) -> PatchApplyReport {
        PatchApplyReport {
            placement: None,
            bound: Vec::new(),
            opened: 0,
            created: 0,
            closed: 0,
            absent: absent.iter().map(|node| (*node).to_owned()).collect(),
            refused: refused
                .iter()
                .map(|(node, reason)| PatchRefusal {
                    node: (*node).to_owned(),
                    reason: (*reason).to_owned(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_wire_refusal_shows_for_six_seconds_and_others_stay() {
        let mut refusals = Refusals::default();
        refusals.flag("ch", refusal(RefusalSource::Wire, 1_000));
        assert!(refusals.visible("ch", 7_000).is_some());
        assert_eq!(refusals.visible("ch", 7_001), None);
        assert_eq!(refusals.wire_shown_until("ch"), Some(7_000));
        refusals.flag("ch", refusal(RefusalSource::Action, 2_000));
        assert_eq!(
            refusals.visible("ch", 99_000).map(|shown| shown.source),
            Some(RefusalSource::Action)
        );
        assert_eq!(refusals.visible("other", 0), None);
    }

    #[test]
    fn the_newest_refusal_is_the_one_shown_and_the_list_is_capped() {
        let mut refusals = Refusals::default();
        for at in 0..12 {
            refusals.flag("ch", refusal(RefusalSource::Action, at));
        }
        assert_eq!(refusals.by_node["ch"].len(), MAX_REFUSALS);
        assert_eq!(
            refusals.visible("ch", 20).map(|shown| shown.at_ms),
            Some(11)
        );
        refusals.dismiss("ch");
        assert_eq!(refusals.visible("ch", 20), None);
    }

    #[test]
    fn an_apply_report_replaces_the_last_apply_and_names_absent_radios() {
        let mut refusals = Refusals::default();
        refusals.absorb_report(&report(&[("ch", "too wide")], &[]), 10);
        refusals.flag("ch", refusal(RefusalSource::Action, 5));
        refusals.absorb_report(&report(&[], &["dev"]), 20);
        assert_eq!(
            refusals.visible("ch", 30).map(|shown| shown.source),
            Some(RefusalSource::Action)
        );
        assert_eq!(
            refusals.visible("dev", 30).map(|shown| shown.reason),
            Some(NOT_CONNECTED.to_owned())
        );
    }
}
