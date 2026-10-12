mod model;
mod rose;
mod sweep;

use sdrmm_wire::{
    HuntAction, HuntRequest, HuntSettings, HuntStatus, hunt::HuntSweepParams, patch::NodeBody,
    ws::ServerEvent,
};
use zgui::prelude::*;

use self::{
    model::{
        Bearing, HUNT_INTERVAL_MS, HuntTarget, bearing, format_hunt_db, format_mhz,
        format_strength, hunt_refusal, hunt_target, hunted_hz, live_hunt,
    },
    sweep::{sweep_panel, sweep_params},
};
use crate::{
    audio::use_audio,
    binding,
    store::Store,
    ui::{
        kit_audio::{self, Stat, Tone, edit_body, fault, readout, titled, titled_block},
        kit_channel::{self, button},
        widgets::{check, row_field},
    },
};

const SHEET: &str = css!(
    r#"
.hunt__meter { height: 12px; border-radius: 6px; background-color: var(--panel-3); overflow: hidden; }
.hunt__fill { height: 12px; border-radius: 6px; background-color: var(--accent-dim); }
.hunt__fill.near { background-color: var(--accent); }
.hunt__warm { color: var(--accent); }
"#
);

#[derive(Clone, Copy)]
struct Hunt {
    store: Store,
    target: Memo<Option<HuntTarget>>,
    status: Memo<Option<HuntStatus>>,
    pushed: RwSignal<Option<HuntStatus>>,
    busy: RwSignal<bool>,
    position: Memo<bool>,
    params: Memo<HuntSweepParams>,
}

fn hunt_of(store: Store, node: &str) -> Option<sdrmm_wire::patch::HuntNode> {
    store
        .graph
        .with(|graph| match graph.node(node).map(|found| &found.body) {
            Some(NodeBody::Hunt(hunt)) => Some(*hunt),
            _ => None,
        })
}

pub fn face(store: Store, node: String) -> impl IntoView {
    kit_audio::install();
    kit_channel::install();
    install_stylesheet("hunt", SHEET);
    install_stylesheet("hunt-rose", rose::SHEET);
    install_stylesheet("hunt-sweep", sweep::SHEET);
    let hunt = hunt_state(store, &node);
    let clicks = Signal::derive({
        let node = node.clone();
        move || hunt_of(store, &node).is_none_or(|hunt| hunt.clicks)
    });
    let strength = Signal::derive(move || hunt.status.get().map_or(0.0, |status| status.strength));
    let clicking = Signal::derive(move || hunt.status.get().is_some() && clicks.get());
    let edit_sweep = {
        let node = node.clone();
        move |change: Box<dyn FnOnce(&mut HuntSweepParams)>| {
            edit_body(store, &node, |body| {
                if let NodeBody::Hunt(hunt) = body {
                    change(&mut hunt.sweep);
                }
            });
        }
    };
    let act = {
        let node = node.clone();
        move |action| act(hunt, &node, action)
    };
    view! {
        column(class = "face") {
            if move || hunt.target.get().is_none() {
                text(class = "hint") {"Wire this node's control out to a decoder"}
            }
            {body(hunt, act.clone())}
            {kit_audio::alert(move || hunt.status.get().and_then(|status| status.error))}
            {titled_block("Geiger clicks", AnyView::new(row_field("Clicks", check(clicks, {
                let node = node.clone();
                move |on| set_clicks(store, &node, on)
            }))))}
            if move || hunt.position.get() {
                {sweep_params(hunt.params, edit_sweep.clone())}
            }
            if move || clicking.get() {
                {clicker(store, strength)}
            }
            row(class = "face__foot rec-foot") {
                {kit_audio::stats(move || hunt.status.get().map(|status| hunt_stats(&status)).unwrap_or_default())}
                {controls(hunt, act)}
            }
        }
    }
}

fn hunt_state(store: Store, node: &str) -> Hunt {
    let target = {
        let node = node.to_owned();
        Memo::new(move |_| hunt_target(&store.graph.get(), &store.state.get().device_sets, &node))
    };
    let pushed = RwSignal::new(None::<HuntStatus>);
    store.on_event(move |event| {
        if let ServerEvent::HuntUpdate { device_set, status } = event
            && target.with_untracked(|target| {
                target.as_ref().is_some_and(|target| {
                    target.set.id == *device_set && target.channel.id == status.settings.channel
                })
            })
        {
            pushed.set(Some((**status).clone()));
        }
    });
    let status = Memo::new(move |_| {
        let target = target.get();
        live_hunt(
            target.as_ref().map(|target| &target.set),
            target.as_ref().map(|target| target.channel.id),
            pushed.get().as_ref(),
        )
    });
    let position = {
        let node = node.to_owned();
        Memo::new(move |_| !binding::sources_of(&store.graph.get(), &node, "position").is_empty())
    };
    let params = {
        let node = node.to_owned();
        Memo::new(move |_| {
            hunt_of(store, &node)
                .map(|hunt| hunt.sweep)
                .unwrap_or_default()
        })
    };
    Hunt {
        store,
        target,
        status,
        pushed,
        busy: RwSignal::new(false),
        position,
        params,
    }
}

#[must_use]
fn hunt_stats(status: &HuntStatus) -> Vec<Stat> {
    let mut shown = vec![Stat::new(
        "Readings",
        status.readings.to_string(),
        "Level readings taken",
    )];
    if status.pose_drops > 0 {
        shown.push(
            Stat::new(
                "Pose drops",
                status.pose_drops.to_string(),
                "Poses refused, the queue was full",
            )
            .tone(Tone::Danger),
        );
    }
    shown
}

fn set_clicks(store: Store, node: &str, on: bool) {
    edit_body(store, node, |body| {
        if let NodeBody::Hunt(hunt) = body {
            hunt.clicks = on;
        }
    });
}

fn clicker(store: Store, strength: Signal<f32>) -> impl IntoView {
    match use_audio().map(|audio| audio.hold_clicks(strength)) {
        Some(Ok(())) => {}
        Some(Err(error)) => store.say(format!("no clicks: {error}")),
        None => store.say("no clicks: audio is off"),
    }
}

fn body(hunt: Hunt, act: impl Fn(HuntAction) + Clone + 'static) -> impl IntoView {
    let running = Memo::new(move |_| hunt.status.get().is_some());
    move || {
        if !running.get() {
            return AnyView::new(idle(hunt.target));
        }
        let sweep = Memo::new(move |_| hunt.status.get().and_then(|status| status.sweep));
        let panel = hunt
            .position
            .get()
            .then(|| AnyView::new(sweep_panel(sweep, hunt.busy.into(), act.clone())));
        AnyView::new(view! {
            {running_view(hunt.status)}
            {panel}
        })
    }
}

fn running_view(status: Memo<Option<HuntStatus>>) -> impl IntoView {
    let heading = Signal::derive(move || bearing(status.get().as_ref()));
    let fill = move || {
        let strength = status.get().map_or(0.0, |status| status.strength);
        Some(format!("{}%", (strength.clamp(0.0, 1.0) * 100.0).round()))
    };
    let near = move || matches!(heading.get(), Bearing::Closing | Bearing::Steady);
    let read = move |pick: fn(&HuntStatus) -> String| {
        AnyView::new(move || status.get().as_ref().map(pick).unwrap_or_default())
    };
    view! {
        column(class = "audio-controls") {
            box(class = "hunt__meter", a11y:role = Role::Meter, a11y:label = "Distance to the transmitter") {
                box(class = "hunt__fill", class:near = near, style:width = fill)
            }
            {readout(vec![
                ("Hunting", read(|status| format_mhz(hunted_hz(Some(status), None)))),
                ("Trend", AnyView::new(view! {
                    text(class:hunt__warm = move || heading.get() == Bearing::Closing) {
                        {move || heading.get().label()}
                    }
                })),
                ("Strength", read(|status| format_strength(Some(status)))),
                ("Level", read(|status| format_hunt_db(status.level_db))),
                ("Smoothed", read(|status| format_hunt_db(status.smooth_db))),
                ("Walked", read(|status| format!("{} \u{2192} {}", format_hunt_db(status.floor_db), format_hunt_db(status.best_db)))),
            ])}
        }
    }
}

fn idle(target: Memo<Option<HuntTarget>>) -> impl IntoView {
    let hunting = move || {
        target.get().map_or_else(String::new, |target| {
            format!(
                "{} at {}",
                target.channel.settings.params.type_id(),
                format_mhz(hunted_hz(None, Some(&target.channel)))
            )
        })
    };
    let refusal = move || {
        hunt_refusal(target.get().as_ref())
            .map(|refusal| fault(refusal.label.to_owned(), Some(refusal.title.to_owned())))
    };
    view! {
        column(class = "audio-controls") {
            if move || target.get().is_some() {
                {readout(vec![("Hunting", AnyView::new(hunting))])}
            }
            {refusal}
        }
    }
}

fn request_for(hunt: Hunt, node: &str, action: HuntAction, channel: u32) -> HuntRequest {
    let settings = matches!(action, HuntAction::Start | HuntAction::Sweep).then(|| HuntSettings {
        interval_ms: HUNT_INTERVAL_MS,
        node: Some(node.to_owned()),
        sweep: hunt.params.get_untracked(),
        ..HuntSettings::for_channel(channel)
    });
    HuntRequest { action, settings }
}

fn act(hunt: Hunt, node: &str, action: HuntAction) {
    let Some(target) = hunt.target.get_untracked() else {
        return;
    };
    let request = request_for(hunt, node, action, target.channel.id);
    let store = hunt.store;
    hunt.busy.set(true);
    zgui::task::spawn_local(async move {
        let path = format!(
            "/api/devicesets/{}/channels/{}/hunt",
            target.set.id, target.channel.id
        );
        match store.api().post::<_, HuntStatus>(&path, &request).await {
            Ok(_) if request.action == HuntAction::Stop => hunt.pushed.set(None),
            Ok(_) => {}
            Err(error) => store.say(format!("cannot drive the hunt: {error}")),
        }
        hunt.busy.try_set(false);
        store.refresh_state();
    });
}

fn controls(hunt: Hunt, act: impl Fn(HuntAction) + Clone + 'static) -> impl IntoView {
    let running = Memo::new(move |_| hunt.status.get().is_some());
    let blocked = Signal::derive(move || {
        let target = hunt.target.get();
        hunt.busy.get() || target.is_none() || hunt_refusal(target.as_ref()).is_some()
    });
    move || {
        let act = act.clone();
        if running.get() {
            return AnyView::new(button(
                "Stop hunt",
                "kc-btn kc-btn--danger",
                hunt.busy,
                move || act(HuntAction::Stop),
            ));
        }
        let turn = {
            let act = act.clone();
            hunt.position.get().then(|| {
                titled(
                    "Turn slowly all the way round",
                    AnyView::new(button("Sweep", "kc-btn", blocked, move || {
                        act(HuntAction::Sweep)
                    })),
                )
            })
        };
        AnyView::new(view! {
            {turn}
            {button("Start hunt", "kc-btn kc-btn--primary", blocked, move || act(HuntAction::Start))}
        })
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::hunt::HuntSweep;

    use super::*;

    fn status(pose_drops: u64) -> HuntStatus {
        HuntStatus {
            settings: HuntSettings::for_channel(1),
            freq_hz: 0.0,
            bw_hz: 0.0,
            level_db: None,
            smooth_db: None,
            floor_db: None,
            best_db: None,
            strength: 0.0,
            closing: false,
            readings: 12,
            at_ms: 0,
            pose_drops,
            sweep: Some(HuntSweep::default()),
            error: None,
        }
    }

    #[test]
    fn pose_drops_join_the_footer_in_danger_only_when_some_were_refused() {
        assert_eq!(hunt_stats(&status(0)).len(), 1);
        let shown = hunt_stats(&status(3));
        assert_eq!(shown[1].label, "Pose drops");
        assert_eq!(shown[1].tone, Tone::Danger);
    }
}
