use zgui::prelude::*;
use zgui_ui::prelude::*;

pub fn entry(
    value: Signal<String>,
    label: String,
    optional: bool,
    commit: impl Fn(String) -> Result<(), String> + Clone + 'static,
) -> impl IntoView {
    let draft = RwSignal::new_local(value.get_untracked());
    let focused = RwSignal::new_local(false);
    let error = RwSignal::new_local(None::<String>);
    let sync = zgui::reactive::RenderEffect::new(move |_| {
        let current = value.get();
        if !focused.get() {
            draft.set(current);
        }
    });
    on_cleanup_local(move || drop(sync));
    let submit = move || {
        if draft.get_untracked() != value.get_untracked() {
            error.set(commit(draft.get_untracked()).err());
        }
    };
    let blur = submit.clone();
    view! {
        column(class = "entry", on:key_down = crate::ui::kit_shell::typing) {
            Input(
                value = draft,
                class = "native-input",
                label = label,
                placeholder = if optional { "Auto / off" } else { "" },
                invalid = Signal::derive_local(move || error.get().is_some()),
                on:focus_in = move |_| focused.set(true),
                on:focus_out = move |_| { blur(); focused.set(false); },
                on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                    match ev.key {
                        Key::Named(NamedKey::Enter) => { submit(); ev.prevent_default(); },
                        Key::Named(NamedKey::Escape) => { draft.set(value.get_untracked()); error.set(None); ev.prevent_default(); },
                        _ => {}
                    }
                },
            )
            if move || error.get().is_some() {
                text(class = "field__error") {{move || error.get().unwrap_or_default()}}
            }
        }
    }
}
