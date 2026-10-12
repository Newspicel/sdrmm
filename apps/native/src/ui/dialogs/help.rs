use sdrmm_wire::about::AboutResponse;
use zgui::prelude::*;

use crate::{
    shell::{
        help::{DISCORD, api_docs, docs_page, host, releases},
        hotkeys::{BINDINGS, Binding, grouped, key_caps},
    },
    store::Store,
    ui::{
        dialogs::{Loaded, frame, legend, load, title},
        files::open_link,
        kit_shell::{button, small_icon},
        shell::{Dialog, Shell},
    },
};

mod glyph {
    pub const BOOK: &str = r#"<path d="M12 7v14"/><path d="M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z"/>"#;
    pub const RADIO: &str = r#"<path d="M16.247 7.761a6 6 0 0 1 0 8.478"/><path d="M19.075 4.933a10 10 0 0 1 0 14.134"/><path d="M4.925 19.067a10 10 0 0 1 0-14.134"/><path d="M7.753 16.239a6 6 0 0 1 0-8.478"/><circle cx="12" cy="12" r="2"/>"#;
    pub const WORKFLOW: &str = r#"<rect width="8" height="8" x="3" y="3" rx="2"/><path d="M7 11v4a2 2 0 0 0 2 2h4"/><rect width="8" height="8" x="13" y="13" rx="2"/>"#;
    pub const WAVEFORM: &str = r#"<path d="M2 13a2 2 0 0 0 2-2V7a2 2 0 0 1 4 0v13a2 2 0 0 0 4 0V4a2 2 0 0 1 4 0v13a2 2 0 0 0 4 0v-4a2 2 0 0 1 2-2"/>"#;
    pub const BRACES: &str = r#"<path d="M8 3H7a2 2 0 0 0-2 2v5a2 2 0 0 1-2 2 2 2 0 0 1 2 2v5c0 1.1.9 2 2 2h1"/><path d="M16 21h1a2 2 0 0 0 2-2v-5c0-1.1.9-2 2-2a2 2 0 0 1-2-2V5a2 2 0 0 0-2-2h-1"/>"#;
    pub const BUOY: &str = r#"<circle cx="12" cy="12" r="10"/><path d="m4.93 4.93 4.24 4.24"/><path d="m14.83 9.17 4.24-4.24"/><path d="m14.83 14.83 4.24 4.24"/><path d="m9.17 14.83-4.24 4.24"/><circle cx="12" cy="12" r="4"/>"#;
    pub const CHAT: &str = r#"<path d="M14 9a2 2 0 0 1-2 2H6l-4 4V4a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2z"/><path d="M18 9h2a2 2 0 0 1 2 2v11l-4-4h-6a2 2 0 0 1-2-2v-1"/>"#;
    pub const BUG: &str = r#"<path d="m8 2 1.88 1.88"/><path d="M14.12 3.88 16 2"/><path d="M9 7.13v-1a3.003 3.003 0 1 1 6 0v1"/><path d="M12 20c-3.3 0-6-2.7-6-6v-3a4 4 0 0 1 4-4h4a4 4 0 0 1 4 4v3c0 3.3-2.7 6-6 6"/><path d="M12 20v-9"/><path d="M6.53 9C4.6 8.8 3 7.1 3 5"/><path d="M6 13H2"/><path d="M3 21c0-2.1 1.7-3.9 3.8-4"/><path d="M20.97 5c0 2.1-1.6 3.8-3.5 4"/><path d="M22 13h-4"/><path d="M17.2 17c2.1.1 3.8 1.9 3.8 4"/>"#;
    pub const CODE: &str = r#"<path d="m16 18 6-6-6-6"/><path d="m8 6-6 6 6 6"/>"#;
    pub const TAG: &str = r#"<path d="M12.586 2.586A2 2 0 0 0 11.172 2H4a2 2 0 0 0-2 2v7.172a2 2 0 0 0 .586 1.414l8.704 8.704a2.426 2.426 0 0 0 3.42 0l6.58-6.58a2.426 2.426 0 0 0 0-3.42z"/><circle cx="7.5" cy="7.5" r=".5"/>"#;
    pub const SCALE: &str = r#"<path d="m16 16 3-8 3 8c-.87.65-1.92 1-3 1s-2.13-.35-3-1Z"/><path d="m2 16 3-8 3 8c-.87.65-1.92 1-3 1s-2.13-.35-3-1Z"/><path d="M7 21h10"/><path d="M12 3v18"/><path d="M3 7h2c2 0 5-1 7-2 2 1 5 2 7 2h2"/>"#;
}

const SHEET: &str = css!(
    r#"
.sk-dialog.help { width: 896px; }
.help__grid { display: grid; grid-template-columns: 224px 1fr; min-height: 0; border-top: 1px solid var(--line); border-bottom: 1px solid var(--line); }
.help__nav { flex-direction: column; gap: 16px; padding: 12px 8px 8px 8px; }
.help__section { flex-direction: column; }
.help__heading { padding: 0 8px 4px 8px; }
.help__row { display: flex; flex-direction: row; align-items: center; gap: 8px; height: 28px; padding: 0 8px; border-radius: 3px; color: var(--ink); }
.help__row:hover { background-color: var(--panel-2); }
.help__row:disabled { opacity: 0.45; }
.help__glyph { color: var(--ink-faint); }
.help__row:hover .help__glyph { color: var(--accent); }
.help__label { flex: 1 1 auto; min-width: 0; font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.help__host { flex: 0 1 auto; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.help__keys { flex-direction: column; gap: 12px; padding: 16px; border-left: 1px solid var(--line); background-color: var(--bg); }
.help__keys-head { align-items: baseline; }
.help__all:hover { color: var(--accent); }
.help__groups { display: grid; grid-template-columns: 1fr 1fr; column-gap: 32px; row-gap: 16px; }
.help__group { flex-direction: column; gap: 6px; }
.help__group-title { font-size: 11px; font-weight: 500; color: var(--ink-dim); }
.help__keymap { display: grid; grid-template-columns: 104px 1fr; column-gap: 12px; row-gap: 6px; align-items: center; }
.help__caps { justify-content: flex-end; align-items: center; flex-wrap: wrap; gap: 4px; }
.help__part { font-family: var(--mono); font-size: 10.5px; color: var(--ink-faint); }
.help__part.cap { min-width: 20px; height: 20px; padding: 0 4px; justify-content: center; align-items: center; text-align: center; border: 1px solid var(--line); border-bottom: 2px solid var(--line-strong); border-radius: 3px; background-color: var(--panel-3); color: var(--ink); }
.help__what { font-size: 12px; color: var(--ink-dim); }
"#
);

pub fn dialog(store: Store, shell: Shell) -> impl IntoView {
    install_stylesheet("shell-help", SHEET);
    let about: Loaded<AboutResponse> = load(store, String::from("/api/about"));
    let version = move || match about.get() {
        Some(Ok(about)) => format!("SDR-- {}", about.version),
        _ => String::new(),
    };
    frame(
        "help",
        title("Help"),
        move || legend(version()),
        view! {
            box(class = "help__grid") {
                {links(store, shell, about)}
                {keyboard(store)}
            }
        },
        view! {
            spacer() {}
            {button("Close", "", move || shell.close())}
        },
    )
}

fn links(store: Store, shell: Shell, about: Loaded<AboutResponse>) -> impl IntoView {
    let repository = move || match about.get() {
        Some(Ok(about)) => about.repository.clone(),
        _ => String::new(),
    };
    let api = api_docs(store.api().base());
    view! {
        column(class = "help__nav", a11y:label = "Help links") {
            {section("Learn", view! {
                {link_row(store, glyph::BOOK, "Guide", docs_page(""))}
                {link_row(store, glyph::RADIO, "First receiver", docs_page("getting-started/first-receiver"))}
                {link_row(store, glyph::WORKFLOW, "Nodes and wires", docs_page("getting-started/workspace"))}
                {link_row(store, glyph::WAVEFORM, "Decoders", docs_page("user-guide/decoders"))}
                {link_row(store, glyph::BRACES, "API", api)}
            })}
            {section("Get help", view! {
                {link_row(store, glyph::BUOY, "Troubleshooting", docs_page("troubleshooting"))}
                {link_row(store, glyph::CHAT, "Ask on Discord", DISCORD.to_owned())}
                {action_row(glyph::BUG, "Report a problem", move || shell.open(Dialog::Report(None)))}
            })}
            {section("Project", view! {
                {move || AnyView::new(link_row(store, glyph::CODE, "Source code", repository()))}
                {move || AnyView::new(link_row(store, glyph::TAG, "Releases", releases(&repository())))}
                {action_row(glyph::SCALE, "Licenses", move || shell.open(Dialog::About))}
            })}
        }
    }
}

fn section(title: &'static str, rows: impl IntoView + 'static) -> impl IntoView {
    view! {
        column(class = "help__section") {
            text(class = "legend help__heading") {{title}}
            {rows}
        }
    }
}

fn link_row(store: Store, glyph: &'static str, label: &'static str, href: String) -> impl IntoView {
    let ready = !href.is_empty();
    let shown = host(&href);
    view! {
        control(
            class = "help__row",
            state:disabled = move || !ready,
            tabindex = Focus::Sequential,
            a11y:role = Role::Link,
            a11y:label = label,
            on:click:stop = move |_| if ready { open_link(store, &href) }
        ) {
            box(class = "help__glyph") {{small_icon(glyph)}}
            text(class = "help__label") {{label}}
            text(class = "legend help__host") {{shown}}
        }
    }
}

fn action_row(
    glyph: &'static str,
    label: &'static str,
    on_press: impl Fn() + 'static,
) -> impl IntoView {
    view! {
        control(
            class = "help__row",
            tabindex = Focus::Sequential,
            a11y:role = Role::Button,
            on:click:stop = move |_| on_press()
        ) {
            box(class = "help__glyph") {{small_icon(glyph)}}
            text(class = "help__label") {{label}}
        }
    }
}

fn keyboard(store: Store) -> impl IntoView {
    let apple = cfg!(target_os = "macos");
    let groups: Vec<AnyView> = grouped(BINDINGS)
        .into_iter()
        .map(|(group, bindings)| AnyView::new(key_group(group.label(), &bindings, apple)))
        .collect();
    let all_keys = docs_page("user-guide/keyboard");
    view! {
        column(class = "help__keys", a11y:label = "Keyboard") {
            row(class = "help__keys-head") {
                text(class = "legend") {"Keyboard"}
                spacer() {}
                control(
                    class = "legend help__all",
                    tabindex = Focus::Sequential,
                    a11y:role = Role::Link,
                    on:click:stop = move |_| open_link(store, &all_keys)
                ) {
                    "All keys"
                }
            }
            box(class = "help__groups") {{groups}}
        }
    }
}

fn key_group(label: &'static str, bindings: &[Binding], apple: bool) -> impl IntoView {
    let rows: Vec<AnyView> = bindings
        .iter()
        .flat_map(|binding| {
            [
                AnyView::new(caps(binding.keys, apple)),
                AnyView::new(view! { text(class = "help__what") {{binding.what}} }),
            ]
        })
        .collect();
    view! {
        column(class = "help__group") {
            text(class = "help__group-title") {{label}}
            box(class = "help__keymap") {{rows}}
        }
    }
}

fn caps(keys: &str, apple: bool) -> impl IntoView {
    let parts: Vec<AnyView> = key_caps(keys, apple)
        .into_iter()
        .map(|(part, cap)| {
            AnyView::new(view! { text(class = "help__part", class:cap = cap) {{part}} })
        })
        .collect();
    view! { row(class = "help__caps") {{parts}} }
}
