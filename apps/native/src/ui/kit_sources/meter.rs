use zgui::{
    geom::{Css, CssPx},
    prelude::*,
};

use crate::ui::widgets::{Point, track};

pub const METER_FLOOR_DB: f32 = -60.0;
pub const METER_WARN_DB: f32 = -12.0;
pub const METER_HOT_DB: f32 = -3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeterTone {
    Ok,
    Warn,
    Danger,
}

impl MeterTone {
    #[must_use]
    pub const fn class(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Danger => "danger",
        }
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
pub fn meter_tone(peak_db: Option<f32>, clipping: bool) -> MeterTone {
    let peak = peak_db.filter(|db| db.is_finite());
    if clipping || peak.is_some_and(|db| db >= METER_HOT_DB) {
        MeterTone::Danger
    } else if peak.is_some_and(|db| db >= METER_WARN_DB) {
        MeterTone::Warn
    } else {
        MeterTone::Ok
    }
}

#[must_use]
pub fn format_peak(peak_db: Option<f32>) -> String {
    match peak_db {
        Some(db) if db.is_finite() && db > METER_FLOOR_DB => format!("{db:.1} dBFS"),
        _ => "quiet".to_owned(),
    }
}

#[derive(Clone)]
pub struct GainMeter {
    pub label: String,
    pub value: Signal<f64>,
    pub min: f64,
    pub max: f64,
    pub auto: Signal<bool>,
    pub disabled: Signal<bool>,
    pub peak: Signal<Option<f32>>,
    pub tone: Signal<MeterTone>,
}

fn percent(unit: f32) -> Option<String> {
    Some(format!("{}%", unit.clamp(0.0, 1.0) * 100.0))
}

fn headroom() -> impl IntoView {
    let warn = meter_unit(Some(METER_WARN_DB));
    let hot = meter_unit(Some(METER_HOT_DB));
    view! {
        box(class = "kit-meter__room kit-meter__room--warn", style:left = percent(warn), style:width = percent(hot - warn))
        box(class = "kit-meter__room kit-meter__room--hot", style:left = percent(hot), style:width = percent(1.0 - hot))
    }
}

pub fn gain_meter(spec: GainMeter, on_change: impl Fn(f64) + Clone + 'static) -> impl IntoView {
    let GainMeter {
        label,
        value,
        min,
        max,
        auto,
        disabled,
        peak,
        tone,
    } = spec;
    let span = if (max - min).abs() > f64::EPSILON {
        max - min
    } else {
        1.0
    };
    let fraction = move || ((value.get() - min) / span).clamp(0.0, 1.0) as f32;
    let rail = NodeRef::new();
    let held = RwSignal::new(false);
    let seek = {
        let on_change = on_change.clone();
        move |at: Point<CssPx, Css>| {
            if let Some(at) = track(rail.window_bounds(), at, rail.scale()) {
                on_change(min + span * f64::from(at));
            }
        }
    };
    let press = {
        let seek = seek.clone();
        move |ev: &mut EventCx<'_, events::PointerDown>| {
            if ev.button != Some(PointerButton::Primary) || disabled.get_untracked() {
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
    let release = move |ev: &mut EventCx<'_, events::PointerUp>| {
        ev.release_pointer();
        held.set(false);
    };
    let keys = move |ev: &mut EventCx<'_, events::KeyDown>| {
        if disabled.get_untracked() {
            return;
        }
        let step = span / 100.0;
        let now = value.get_untracked();
        let next = match &ev.key {
            Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowDown) => now - step,
            Key::Named(NamedKey::ArrowRight | NamedKey::ArrowUp) => now + step,
            Key::Named(NamedKey::Home) => min,
            Key::Named(NamedKey::End) => max,
            _ => return,
        };
        ev.prevent_default();
        on_change(next.clamp(min.min(max), max.max(min)));
    };
    let metered = Memo::new(move |_| peak.with(Option::is_some));
    let gauge = move || {
        if metered.get() {
            AnyView::new(view! {
                {headroom()}
                box(class = "kit-meter__level", attr:data-tone = move || Some(tone.get().class().to_owned()), style:width = move || percent(meter_unit(peak.get())))
            })
        } else {
            AnyView::new(
                view! { box(class = "kit-meter__set", style:width = move || percent(fraction())) },
            )
        }
    };
    view! {
        box(
            node_ref = rail,
            class = "kit-meter",
            class:held = held,
            class:off = disabled,
            tabindex = Focus::Sequential,
            a11y:role = Role::Slider,
            a11y:label = label,
            a11y:description = move || zgui::vocab::SharedString::from(format_peak(peak.get())),
            on:pointer_down = press,
            on:pointer_move = drag,
            on:pointer_up = release,
            on:pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| {
                ev.release_pointer();
                held.set(false);
            },
            on:key_down = keys
        ) {
            box(class = "kit-meter__rail") {}
            {gauge}
            box(class = "kit-meter__grip", class:auto = auto, style:left = move || percent(fraction()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_meter_spans_the_floor_to_full_scale() {
        assert_eq!(meter_unit(None), 0.0);
        assert_eq!(meter_unit(Some(f32::NEG_INFINITY)), 0.0);
        assert_eq!(meter_unit(Some(-70.0)), 0.0);
        assert_eq!(meter_unit(Some(-30.0)), 0.5);
        assert_eq!(meter_unit(Some(3.0)), 1.0);
    }

    #[test]
    fn the_tone_warns_near_full_scale_and_alarms_on_clipping() {
        assert_eq!(meter_tone(None, false), MeterTone::Ok);
        assert_eq!(meter_tone(Some(-20.0), false), MeterTone::Ok);
        assert_eq!(meter_tone(Some(-12.0), false), MeterTone::Warn);
        assert_eq!(meter_tone(Some(-3.0), false), MeterTone::Danger);
        assert_eq!(meter_tone(None, true), MeterTone::Danger);
        assert_eq!(meter_tone(Some(f32::NAN), false), MeterTone::Ok);
    }

    #[test]
    fn a_quiet_lane_reads_quiet() {
        assert_eq!(format_peak(None), "quiet");
        assert_eq!(format_peak(Some(-80.0)), "quiet");
        assert_eq!(format_peak(Some(-6.04)), "-6.0 dBFS");
    }
}
