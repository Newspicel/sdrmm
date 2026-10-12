use zgui::prelude::*;

use super::{popover_slot, tip};

#[derive(Clone)]
pub struct ChipFace {
    pub label: String,
    pub value: Signal<String>,
    pub quiet: Signal<bool>,
    pub title: String,
}

impl ChipFace {
    #[must_use]
    pub fn new(label: impl Into<String>, value: Signal<String>, title: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value,
            quiet: Signal::stored(false),
            title: title.into(),
        }
    }

    #[must_use]
    pub fn quiet(self, quiet: Signal<bool>) -> Self {
        Self { quiet, ..self }
    }
}

pub fn chips(items: Vec<AnyView>) -> impl IntoView {
    view! { row(class = "kit-chips") {{items}} }
}

pub fn chip_field(label: impl Into<String>, body: impl IntoView + 'static) -> impl IntoView {
    let label = label.into();
    view! {
        column(class = "kit-chip-field") {
            text(class = "kit-legend") {{label}}
            row(class = "kit-chip-field__body") {{body}}
        }
    }
}

fn face_view(label: String, value: Signal<String>, quiet: Signal<bool>) -> impl IntoView {
    view! {
        text(class = "kit-chip__label") {{label}}
        text(class = "kit-chip__value", class:quiet = quiet) {{move || value.get()}}
    }
}

pub fn setting_chip(face: ChipFace, body: impl Fn() -> AnyView + 'static) -> AnyView {
    let ChipFace {
        label,
        value,
        quiet,
        title,
    } = face;
    let (open, toggle, _) = popover_slot();
    let trigger = AnyView::new(view! {
        control(
            class = "kit-chip",
            class:open = open,
            tabindex = Focus::Sequential,
            a11y:role = Role::Button,
            a11y:label = title.clone(),
            on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
            on:click:stop = move |_| toggle()
        ) {
            {face_view(label, value, quiet)}
        }
    });
    let panel = move || {
        open.get().then(|| {
            AnyView::new(view! {
                column(class = "kit-pop kit-pop--chip", on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation()) {
                    {body()}
                }
            })
        })
    };
    AnyView::new(view! {
        box(class = "kit-pop-wrap") {
            {tip(move || title.clone(), trigger)}
            {panel}
        }
    })
}

pub fn toggle_chip(
    label: impl Into<String>,
    on: Signal<bool>,
    title: impl Into<String>,
    on_change: impl Fn(bool) + 'static,
) -> AnyView {
    let label = label.into();
    let title: String = title.into();
    let said = title.clone();
    let chip = AnyView::new(view! {
        control(
            class = "kit-chip",
            class:pressed = on,
            tabindex = Focus::Sequential,
            a11y:role = Role::Button,
            a11y:label = said,
            on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
            on:click:stop = move |_| on_change(!on.get_untracked())
        ) {
            text(class = "kit-chip__label") {{label}}
            text(class = "kit-chip__value", class:quiet = move || !on.get(), class:accent = on) {{move || if on.get() { "on" } else { "off" }}}
        }
    });
    tip(move || title.clone(), chip)
}

pub fn readout_chip(
    label: impl Into<String>,
    value: Signal<String>,
    title: impl Into<String>,
) -> AnyView {
    let label = label.into();
    let title: String = title.into();
    let chip = AnyView::new(view! {
        row(class = "kit-chip kit-chip--readout") {
            {face_view(label, value, Signal::stored(false))}
        }
    });
    tip(move || title.clone(), chip)
}
