use std::path::PathBuf;

use zgui::prelude::*;

use crate::{
    binding,
    store::Store,
    ui::{
        kit_audio::{self, button, titled},
        kit_channel::{self, Popover},
    },
};

const SHEET: &str = css!(
    r#"
.ex-menu { width: 168px; padding: 4px; gap: 0; }
.ex-choice { padding: 4px 8px; border-radius: 4px; font-size: 12px; color: var(--ink); }
.ex-choice:hover { background-color: var(--panel-2); }
"#
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Csv,
    Json,
}

const CHOICES: [Format; 2] = [Format::Csv, Format::Json];

impl Format {
    const fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Csv => "CSV",
            Self::Json => "JSON",
        }
    }

    const fn hint(self) -> &'static str {
        match self {
            Self::Csv => "One row per decoded frame",
            Self::Json => "Every field of every row",
        }
    }
}

#[must_use]
fn export_path(format: Format, sink: &str) -> String {
    format!(
        "/api/decoderlog/export/{}?sink={}",
        format.extension(),
        encode(sink)
    )
}

#[must_use]
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(byte).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

fn target(format: Format, sink: &str) -> PathBuf {
    let folder = dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir);
    folder.join(format!(
        "decoder-log-{}.{}",
        encode(sink),
        format.extension()
    ))
}

fn download(store: Store, format: Format, sink: String) {
    zgui::task::spawn_local(async move {
        let bytes = match store.api().bytes(&export_path(format, &sink)).await {
            Ok(bytes) => bytes,
            Err(error) => {
                store.say(format!("cannot export the log: {error}"));
                return;
            }
        };
        let path = target(format, &sink);
        match std::fs::write(&path, bytes) {
            Ok(()) => store.say(format!("Saved {}", path.display())),
            Err(error) => store.say(format!("cannot save {}: {error}", path.display())),
        }
    });
}

fn choice(store: Store, sink: String, format: Format, popover: Popover) -> AnyView {
    titled(
        format.hint(),
        AnyView::new(view! {
            control(
                class = "ex-choice",
                tabindex = Focus::Sequential,
                a11y:role = Role::MenuItem,
                on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                on:click:stop = move |_| {
                    popover.close();
                    download(store, format, sink.clone());
                }
            ) {
                {format.label()}
            }
        }),
    )
}

fn download_menu(store: Store, sink: String, idle: Signal<bool>) -> impl IntoView {
    let popover = Popover::new();
    let menu = move || {
        popover.open().then(|| {
            let choices: Vec<AnyView> = CHOICES
                .into_iter()
                .map(|format| choice(store, sink.clone(), format, popover))
                .collect();
            view! {
                column(class = "kc-pop__panel ex-menu", a11y:role = Role::Menu) {{choices}}
            }
        })
    };
    view! {
        box(class = "kc-pop") {
            {button(
                || String::from("Download"),
                Signal::stored(false),
                idle,
                move || {
                    if !idle.get_untracked() {
                        popover.toggle();
                    }
                },
            )}
            {menu}
        }
    }
}

pub fn face(store: Store, node: String) -> impl IntoView {
    kit_audio::install();
    kit_channel::install();
    install_stylesheet("export-face", SHEET);
    let wired = {
        let node = node.clone();
        Signal::derive(move || !binding::sources_of(&store.graph.get(), &node, "events").is_empty())
    };
    let idle = Signal::derive(move || !wired.get());
    view! {
        column(class = "face") {
            text(class = "hint") {{move || if wired.get() { "Every logged row, as one file" } else { "Wire decoders in" }}}
            row(class = "face__foot rec-foot") {
                spacer() {}
                {download_menu(store, node, idle)}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_export_asks_for_the_rows_of_its_own_sink() {
        assert_eq!(
            export_path(Format::Csv, "export-1"),
            "/api/decoderlog/export/csv?sink=export-1"
        );
        assert_eq!(
            export_path(Format::Json, "a b&c"),
            "/api/decoderlog/export/json?sink=a%20b%26c"
        );
    }

    #[test]
    fn every_format_is_offered_with_a_hint() {
        let offered: Vec<_> = CHOICES.iter().map(|format| format.label()).collect();
        assert_eq!(offered, vec!["CSV", "JSON"]);
        assert!(CHOICES.iter().all(|format| !format.hint().is_empty()));
    }

    #[test]
    fn a_saved_file_is_named_by_sink_and_format() {
        let path = target(Format::Csv, "export-1");
        assert!(path.ends_with("decoder-log-export-1.csv"));
    }
}
