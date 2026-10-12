mod about;
mod auto_off;
mod help;
mod report;
mod serial;

use std::sync::Arc;

use zgui::prelude::*;

use crate::{
    store::Store,
    ui::{
        kit_shell::scrim,
        shell::{Dialog, Shell},
    },
};

pub fn layer(store: Store, shell: Shell) -> impl IntoView {
    let dialog = move || {
        let open = shell.dialog.get()?;
        let body = match open {
            Dialog::Help => AnyView::new(help::dialog(store, shell)),
            Dialog::About => AnyView::new(about::dialog(store, shell)),
            Dialog::Report(seed) => AnyView::new(report::dialog(store, shell, seed)),
        };
        Some(AnyView::new(scrim(move || shell.close(), body)))
    };
    let asking = move || {
        shell.asking_auto_off.get().then(|| {
            AnyView::new(scrim(
                move || shell.cancel_auto_off(),
                auto_off::dialog(shell),
            ))
        })
    };
    view! {
        {dialog}
        {asking}
        {serial::layer(store, shell)}
    }
}

type Loaded<T> = RwSignal<Option<Result<Arc<T>, String>>>;

fn load<T: serde::de::DeserializeOwned + Send + Sync + 'static>(
    store: Store,
    path: String,
) -> Loaded<T> {
    let into: Loaded<T> = RwSignal::new(None);
    zgui::task::spawn_local(async move {
        let result = store.api().get::<T>(&path).await;
        into.set(Some(
            result.map(Arc::new).map_err(|error| error.to_string()),
        ));
    });
    into
}

pub fn title(text: impl IntoView + 'static) -> impl IntoView {
    view! { text(class = "sk-dialog__title") {{text}} }
}

pub fn legend(text: impl IntoView + 'static) -> impl IntoView {
    view! { text(class = "legend sk-dialog__aside") {{text}} }
}

pub fn frame(
    class: &'static str,
    head: impl IntoView + 'static,
    aside: impl IntoView + 'static,
    body: impl IntoView + 'static,
    foot: impl IntoView + 'static,
) -> impl IntoView {
    view! {
        column(
            class = format!("sk-dialog {class}"),
            a11y:role = Role::Dialog,
            on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation()
        ) {
            row(class = "sk-dialog__head") {
                {head}
                spacer() {}
                {aside}
            }
            column(class = "sk-dialog__body") {{body}}
            row(class = "sk-dialog__foot") {{foot}}
        }
    }
}
