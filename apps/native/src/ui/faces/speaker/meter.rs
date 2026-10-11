use std::{cell::Cell, rc::Rc, time::Duration};

use zgui::{
    geom::{Css, CssPx},
    prelude::*,
};

use crate::ui::widgets::{Point, track};

pub const METER_FLOOR_DB: f32 = -60.0;
pub const METER_WARN_DB: f32 = -12.0;
pub const METER_HOT_DB: f32 = -3.0;
const LEVEL_TICK: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeterTone {
    Ok,
    Warn,
    Danger,
}

#[must_use]
pub fn pcm_peak_db(pcm: &[f32]) -> f32 {
    let peak = pcm
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    if peak > 0.0 {
        20.0 * peak.log10()
    } else {
        f32::NEG_INFINITY
    }
}

#[must_use]
pub fn meter_unit(peak_db: Option<f32>) -> f32 {
    match peak_db {
        Some(db) if db.is_finite() && db > METER_FLOOR_DB => {
            ((db - METER_FLOOR_DB) / -METER_FLOOR_DB).min(1.0)
        }
        _ => 0.0,
    }
}

#[must_use]
pub fn meter_tone(peak_db: Option<f32>) -> MeterTone {
    match peak_db {
        Some(db) if db >= METER_HOT_DB => MeterTone::Danger,
        Some(db) if db >= METER_WARN_DB => MeterTone::Warn,
        _ => MeterTone::Ok,
    }
}

#[must_use]
pub fn format_peak(peak_db: Option<f32>) -> String {
    match peak_db {
        Some(db) if db.is_finite() && db > METER_FLOOR_DB => format!("{db:.1} dBFS"),
        _ => String::from("quiet"),
    }
}

pub fn peak_signal(
    playing: Signal<bool>,
) -> (Signal<Option<f32>>, impl Fn(&[f32], usize) + 'static) {
    let held = Rc::new(Cell::new(f32::NEG_INFINITY));
    let peak = RwSignal::new(None::<f32>);
    let flushed = held.clone();
    let ticking = set_interval(LEVEL_TICK, move || {
        let next = Some(flushed.replace(f32::NEG_INFINITY));
        if peak.try_get_untracked() != Some(next) {
            peak.try_set(next);
        }
    });
    on_cleanup_local(move || drop(ticking));
    let shown = Signal::derive(move || if playing.get() { peak.get() } else { None });
    let listen = move |pcm: &[f32], _channels: usize| {
        held.set(held.get().max(pcm_peak_db(pcm)));
    };
    (shown, listen)
}

pub fn gain_meter(
    label: String,
    volume: Signal<f64>,
    peak_db: Signal<Option<f32>>,
    on_change: impl Fn(f64) + Clone + 'static,
) -> impl IntoView {
    let rail = NodeRef::new();
    let held = RwSignal::new(false);
    let seek = {
        let on_change = on_change.clone();
        move |at: Point<CssPx, Css>| {
            if let Some(at) = track(rail.window_bounds(), at, rail.scale()) {
                on_change(f64::from(at));
            }
        }
    };
    let press = {
        let seek = seek.clone();
        move |ev: &mut EventCx<'_, events::PointerDown>| {
            if ev.button != Some(PointerButton::Primary) {
                return;
            }
            ev.stop_propagation();
            ev.capture_pointer();
            held.set(true);
            seek(ev.position);
        }
    };
    let drag = move |ev: &mut EventCx<'_, events::PointerMove>| {
        if held.get_untracked() {
            seek(ev.position);
        }
    };
    let keys = move |ev: &mut EventCx<'_, events::KeyDown>| {
        let now = volume.get_untracked();
        let next = match &ev.key {
            Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowDown) => now - 0.01,
            Key::Named(NamedKey::ArrowRight | NamedKey::ArrowUp) => now + 0.01,
            Key::Named(NamedKey::Home) => 0.0,
            Key::Named(NamedKey::End) => 1.0,
            _ => return,
        };
        ev.prevent_default();
        on_change(next.clamp(0.0, 1.0));
    };
    let level = move || format!("{}%", meter_unit(peak_db.get()) * 100.0);
    let thumb = move || format!("calc({}% - 2px)", volume.get().clamp(0.0, 1.0) * 100.0);
    let tone = move || meter_tone(peak_db.get());
    view! {
        box(
            node_ref = rail,
            class = "gm",
            tabindex = Focus::Sequential,
            a11y:role = Role::Slider,
            a11y:label = label,
            a11y:value = move || zgui::vocab::SharedString::from(format_peak(peak_db.get())),
            on:pointer_down = press,
            on:pointer_move = drag,
            on:pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
                ev.release_pointer();
                held.set(false);
            },
            on:pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| {
                ev.release_pointer();
                held.set(false);
            },
            on:key_down = keys
        ) {
            box(class = "gm__track") {
                box(
                    class = "gm__fill",
                    class:warn = move || tone() == MeterTone::Warn,
                    class:danger = move || tone() == MeterTone::Danger,
                    style:width = move || Some(level())
                )
            }
            box(class = "gm__thumb", style:left = move || Some(thumb()))
        }
    }
}

pub const SHEET: &str = css!(
    r#"
.gm { position: relative; flex: 1 1 auto; min-width: 60px; height: 20px; align-items: center; }
.gm__track {
    position: absolute; left: 0; right: 0; top: 6px; height: 8px; border-radius: 2px; overflow: hidden;
    border: 1px solid var(--line); background-color: var(--panel-2);
}
.gm__fill { position: absolute; left: 0; top: 0; bottom: 0; border-radius: 1px; background-color: var(--ok); }
.gm__fill.warn { background-color: oklch(0.8 0.14 85); }
.gm__fill.danger { background-color: var(--danger); }
.gm__thumb { position: absolute; top: 2px; width: 4px; height: 16px; border-radius: 2px; background-color: var(--ink); }
.gm:hover .gm__thumb { background-color: var(--accent); }
"#
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_peak_is_the_loudest_sample_in_dbfs() {
        assert!((pcm_peak_db(&[0.1, -0.5, 0.25]) - (-6.0206)).abs() < 1e-3);
        assert!(pcm_peak_db(&[1.0]).abs() < 1e-6);
        assert_eq!(pcm_peak_db(&[0.0, 0.0]), f32::NEG_INFINITY);
        assert_eq!(pcm_peak_db(&[]), f32::NEG_INFINITY);
    }

    #[test]
    fn the_meter_spans_the_floor_to_full_scale() {
        assert_eq!(meter_unit(None), 0.0);
        assert_eq!(meter_unit(Some(f32::NEG_INFINITY)), 0.0);
        assert_eq!(meter_unit(Some(-80.0)), 0.0);
        assert!((meter_unit(Some(-30.0)) - 0.5).abs() < 1e-6);
        assert_eq!(meter_unit(Some(6.0)), 1.0);
    }

    #[test]
    fn the_meter_warms_then_runs_hot_near_full_scale() {
        assert_eq!(meter_tone(None), MeterTone::Ok);
        assert_eq!(meter_tone(Some(-20.0)), MeterTone::Ok);
        assert_eq!(meter_tone(Some(-12.0)), MeterTone::Warn);
        assert_eq!(meter_tone(Some(-3.0)), MeterTone::Danger);
    }

    #[test]
    fn a_quiet_input_says_so() {
        assert_eq!(format_peak(None), "quiet");
        assert_eq!(format_peak(Some(-70.0)), "quiet");
        assert_eq!(format_peak(Some(-6.02)), "-6.0 dBFS");
    }
}
