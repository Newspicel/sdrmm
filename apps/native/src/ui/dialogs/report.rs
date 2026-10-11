use sdrmm_wire::{about::AboutResponse, diagnostics::DiagnosticsReport};
use zgui::prelude::*;
use zgui_ui::prelude::*;

use crate::{
    shell::report::{
        BundleInput, bug_issue_url, build_bundle, feature_issue_url, issue_title, workspace_facts,
    },
    store::Store,
    ui::{
        dialogs::{Loaded, frame, legend, load, title},
        files::open_link,
        kit_channel::tip,
        kit_shell::{button, gated_button, typing},
        shell::Shell,
        widgets::check,
    },
};

const SHAPE_HINT: &str = "Node and edge counts per kind. No names, positions or frequencies.";

fn client() -> String {
    format!(
        "sdr-- native {} {}/{}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

pub fn dialog(store: Store, shell: Shell, seed: Option<String>) -> impl IntoView {
    let about: Loaded<AboutResponse> = load(store, String::from("/api/about"));
    let diagnostics: Loaded<DiagnosticsReport> = load(store, String::from("/api/diagnostics"));
    let typed = RwSignal::new_local(String::new());
    let include = RwSignal::new(false);
    let clipboard = use_clipboard();
    let version = move || {
        about
            .get()
            .and_then(Result::ok)
            .map(|about| about.version.clone())
            .or_else(|| {
                diagnostics
                    .get()
                    .and_then(Result::ok)
                    .map(|report| report.doctor.version.clone())
            })
            .unwrap_or_default()
    };
    let repository = move || {
        about
            .get()
            .and_then(Result::ok)
            .map(|about| about.repository.clone())
            .unwrap_or_default()
    };
    let bundle = move || {
        let report = diagnostics.get().and_then(Result::ok);
        let toasts = store.toasts.get();
        let events: Vec<_> = toasts.log.iter().cloned().collect();
        let facts = include.get().then(|| workspace_facts(&store.graph.get()));
        let collected = jiff::Timestamp::now().to_string();
        build_bundle(&BundleInput {
            version: &version(),
            client: &client(),
            collected: &collected,
            diagnostics: report.as_deref(),
            events: &events,
            dropped_events: toasts.dropped,
            workspace: facts.as_ref(),
        })
    };
    let placeholder = seed.clone();
    let headline = move || {
        let typed = typed.get();
        issue_title(if typed.is_empty() {
            seed.as_deref().unwrap_or_default()
        } else {
            &typed
        })
    };
    let copy = {
        let clipboard = clipboard.clone();
        move || {
            clipboard.set_text(ClipboardKind::Standard, bundle());
            store.note("Diagnostics copied to the clipboard");
        }
    };
    let issue = move || {
        let text = bundle();
        clipboard.set_text(ClipboardKind::Standard, text.clone());
        open_link(
            store,
            &bug_issue_url(&repository(), &headline(), &version(), &text),
        );
        shell.close();
    };
    let feature = move || {
        open_link(store, &feature_issue_url(&repository(), &version()));
        shell.close();
    };
    let has_repository = Signal::derive_local(move || !repository().is_empty());
    let summary_hint = placeholder.unwrap_or_else(|| String::from("What went wrong, in one line"));
    let shown = move || match diagnostics.get() {
        None => String::from("Collecting"),
        Some(_) => bundle(),
    };
    let aside = move || {
        let version = version();
        if version.is_empty() {
            String::from("Collecting")
        } else {
            format!("SDR-- {version}")
        }
    };
    let failed = move || matches!(diagnostics.get(), Some(Err(_)));
    frame(
        "wide",
        title("Report a problem"),
        move || legend(aside()),
        view! {
            row(class = "sk-toolbar") {
                text(class = "sk-text") {"Summary"}
                {summary(typed, summary_hint)}
            }
            row(class = "sk-toolbar") {
                {check(include.into(), move |on| include.set(on))}
                {tip(SHAPE_HINT, AnyView::new(view! { text(class = "sk-text") {"Include the workspace shape"} }))}
            }
            text(class = "sk-text") {"This is everything the report carries. Tokens, home paths and addresses are stripped: read it before you publish it."}
            {move || failed().then(|| view! { text(class = "sk-text bad") {"The server did not answer, so only this window's log is below."} })}
            text(class = "sk-pre") {{shown}}
        },
        view! {
            {gated_button("Request a feature instead", "quiet", has_repository, feature)}
            spacer() {}
            {button("Close", "", move || shell.close())}
            {button("Copy", "", copy)}
            {gated_button("Open a GitHub issue", "primary", has_repository, issue)}
        },
    )
}

fn summary(typed: RwSignal<String, LocalStorage>, placeholder: String) -> impl IntoView {
    view! {
        box(class = "sk-entry") {
            Input(
                class = "sk-field",
                value = typed,
                label = "Summary",
                placeholder = placeholder,
                on:key_down = typing
            )
        }
    }
}
