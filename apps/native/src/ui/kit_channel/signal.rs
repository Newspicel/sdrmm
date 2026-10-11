use sdrmm_wire::{channel::Squelch, state::ChannelLevel};
use zgui::{
    geom::{Css, CssPx},
    prelude::*,
};

use super::{
    LEVEL_FLOOR_DB, debounced, format_level, gate_db, gate_open, level_unit, tip, tip_text,
};
use crate::ui::{
    faces::channel::settings::{
        DEFAULT_SQUELCH_DB, DEFAULT_SQUELCH_MARGIN_DB, SQUELCH_MAX_DB, SQUELCH_MIN_DB,
        auto_squelch_tip, dragged_squelch, squelch_handle_db, squelch_meter_tip, squelch_switched,
    },
    widgets::{Point, track},
};

const SIGNAL_TIP: &str = "Signal in the channel, in dB below full scale";
const SEGMENTS: usize = 12;

fn percent(db: f32) -> f32 {
    level_unit(db, SQUELCH_MIN_DB) * 100.0
}

fn gaps() -> Vec<AnyView> {
    (1..SEGMENTS)
        .map(|at| {
            let left = format!("{}%", at as f32 * 100.0 / SEGMENTS as f32);
            AnyView::new(view! { box(class = "kc-meter__gap", style:left = Some(left)) })
        })
        .collect()
}

fn fill(level: Signal<Option<ChannelLevel>>, open: Signal<bool>) -> impl IntoView {
    let now = move || {
        percent(
            level
                .get()
                .map_or(f32::NEG_INFINITY, |level| level.level_db),
        )
    };
    let peak = move || percent(level.get().map_or(f32::NEG_INFINITY, |level| level.peak_db));
    view! {
        box(class = "kc-meter__fill", class:closed = move || !open.get(), style:width = move || Some(format!("{}%", now()))) {}
        {gaps()}
        {move || (peak() > 0.0).then(|| view! {
            box(class = "kc-meter__peak", style:left = Some(format!("calc({}% - 1px)", peak())))
        })}
    }
}

fn readout(level: Signal<Option<ChannelLevel>>, open: Signal<bool>) -> impl IntoView {
    let heard = move || {
        open.get()
            && level
                .get()
                .is_some_and(|level| level.level_db > LEVEL_FLOOR_DB)
    };
    view! {
        text(class = "kc-meter__read", class:heard = heard) {{move || format_level(level.get().map(|level| level.level_db))}}
    }
}

pub fn signal_row(level: Signal<Option<ChannelLevel>>) -> AnyView {
    let open = Signal::stored(true);
    let name = tip(
        SIGNAL_TIP,
        AnyView::new(view! { text(class = "kc-name kc-name--tip") {"Signal"} }),
    );
    AnyView::new(view! {
        row(class = "kc-signal", a11y:role = Role::Meter, a11y:label = "Signal level") {
            {name}
            box(class = "kc-meter__bar") {{fill(level, open)}}
            {readout(level, open)}
        }
    })
}

fn db_at(rail: NodeRef, at: Point<CssPx, Css>) -> Option<f32> {
    let unit = track(rail.window_bounds(), at, rail.scale())?;
    Some((SQUELCH_MIN_DB + unit * (SQUELCH_MAX_DB - SQUELCH_MIN_DB)).round())
}

fn key_step(key: &Key) -> Option<f32> {
    match key {
        Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowDown) => Some(-1.0),
        Key::Named(NamedKey::ArrowRight | NamedKey::ArrowUp) => Some(1.0),
        Key::Named(NamedKey::Home) => Some(f32::NEG_INFINITY),
        Key::Named(NamedKey::End) => Some(f32::INFINITY),
        _ => None,
    }
}

pub fn squelch_row(
    level: Signal<Option<ChannelLevel>>,
    squelch: Signal<Squelch>,
    on_squelch: impl Fn(Squelch) + Clone + 'static,
) -> AnyView {
    let held_db = RwSignal::new(DEFAULT_SQUELCH_DB);
    let gate =
        Signal::derive(move || gate_db(level.get().as_ref(), squelch.get().manual_level_db()));
    let handle = Signal::derive(move || {
        f64::from(squelch_handle_db(&squelch.get(), gate.get(), held_db.get()))
    });
    let commit = {
        let on_squelch = on_squelch.clone();
        move |db: f64| on_squelch(dragged_squelch(db as f32))
    };
    let (shown, change) = debounced(handle, commit);
    let dragging = RwSignal::new(false);
    let open = Signal::derive(move || {
        let held = squelch.get();
        held.is_off() || gate_open(level.get().as_ref(), held.manual_level_db())
    });
    let auto = Signal::derive(move || squelch.get().auto_margin_db().is_some());
    let move_to = {
        let change = change.clone();
        move |db: f32| {
            let db = db.clamp(SQUELCH_MIN_DB, SQUELCH_MAX_DB);
            if db > SQUELCH_MIN_DB {
                held_db.set(db);
            }
            change(f64::from(db));
        }
    };
    let rail = NodeRef::new();
    let press = {
        let move_to = move_to.clone();
        move |ev: &mut EventCx<'_, events::PointerDown>| {
            if ev.button != Some(PointerButton::Primary) {
                return;
            }
            ev.stop_propagation();
            ev.capture_pointer();
            dragging.set(true);
            if let Some(db) = db_at(rail, ev.position) {
                move_to(db);
            }
        }
    };
    let drag = {
        let move_to = move_to.clone();
        move |ev: &mut EventCx<'_, events::PointerMove>| {
            if dragging.get_untracked()
                && let Some(db) = db_at(rail, ev.position)
            {
                move_to(db);
            }
        }
    };
    let keys = move |ev: &mut EventCx<'_, events::KeyDown>| {
        if let Some(step) = key_step(&ev.key) {
            ev.prevent_default();
            ev.stop_propagation();
            move_to(shown.shown.get_untracked() as f32 + step);
        }
    };
    let at = move || {
        let unit = (shown.shown.get() as f32 - SQUELCH_MIN_DB) / (SQUELCH_MAX_DB - SQUELCH_MIN_DB);
        format!("calc({}% - 2px)", unit.clamp(0.0, 1.0) * 100.0)
    };
    let meter_tip = move || squelch_meter_tip(squelch.get().is_off());
    let auto_button = auto_toggle(level, squelch, held_db, on_squelch);
    let bar = AnyView::new(view! {
        box(
            node_ref = rail,
            class = "kc-meter__bar kc-meter__bar--drag",
            tabindex = Focus::Sequential,
            a11y:role = Role::Slider,
            a11y:label = "Squelch threshold",
            on:pointer_down = press,
            on:pointer_move = drag,
            on:pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| { ev.release_pointer(); dragging.set(false); },
            on:pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| { ev.release_pointer(); dragging.set(false); },
            on:key_down = keys
        ) {
            {fill(level, open)}
            box(class = "kc-meter__handle", class:auto = move || auto.get() && !dragging.get(), style:left = move || Some(at()))
        }
    });
    AnyView::new(view! {
        row(class = "kc-signal") {
            {tip_text(meter_tip, AnyView::new(view! { text(class = "kc-name kc-name--tip") {"Squelch"} }))}
            {bar}
            {readout(level, open)}
            {auto_button}
        }
    })
}

fn auto_toggle(
    level: Signal<Option<ChannelLevel>>,
    squelch: Signal<Squelch>,
    held_db: RwSignal<f32>,
    on_squelch: impl Fn(Squelch) + 'static,
) -> AnyView {
    let auto = Signal::derive(move || squelch.get().auto_margin_db().is_some());
    let flip = move || {
        let held = squelch.get_untracked();
        let level_db = gate_db(level.get_untracked().as_ref(), held.manual_level_db())
            .unwrap_or(held_db.get_untracked());
        let margin_db = held.auto_margin_db().unwrap_or(DEFAULT_SQUELCH_MARGIN_DB);
        on_squelch(squelch_switched(!auto.get_untracked(), level_db, margin_db));
    };
    tip_text(
        move || auto_squelch_tip(&squelch.get()),
        AnyView::new(view! {
            control(
                class = "kc-btn kc-btn--sm",
                class:on = auto,
                tabindex = Focus::Sequential,
                a11y:role = Role::Button,
                a11y:label = "Automatic squelch",
                on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                on:click:stop = move |_| flip()
            ) {
                "Auto"
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_nudge_by_a_decibel_and_home_turns_off() {
        assert_eq!(key_step(&Key::Named(NamedKey::ArrowUp)), Some(1.0));
        assert_eq!(key_step(&Key::Named(NamedKey::ArrowLeft)), Some(-1.0));
        assert_eq!(
            key_step(&Key::Named(NamedKey::Home)),
            Some(f32::NEG_INFINITY)
        );
        assert_eq!(key_step(&Key::Named(NamedKey::Enter)), None);
        assert_eq!(percent(-60.0), 50.0);
    }
}
