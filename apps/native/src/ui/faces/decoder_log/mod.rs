mod columns;
mod detail;
mod footer;
mod groups;
mod list;
mod player;
mod state;

use zgui::prelude::*;
use zgui_ui::prelude::*;

use crate::{
    decoders::{
        groups::{LogView, VIEW_CHOICES},
        log::LIMIT_OPTIONS,
    },
    store::Store,
    ui::{kit_decoders, widgets::pick},
};

use self::state::Log;

const SHEET: &str = css!(
    r#"
.dlog { flex-direction: column; gap: 6px; min-width: 0; }
.dlog__bar { flex-direction: row; align-items: center; gap: 8px; }
.dlog__search { flex: 1 1 auto; min-width: 0; }
.dlog__search .native-input { height: 26px; width: 100%; padding: 3px 8px; font-family: var(--mono); font-size: 11px; }
.dlog__choice { flex: 0 0 auto; flex-direction: row; align-items: center; gap: 4px; }
.dlog__choice .pick__wrap { width: 104px; }
.dlog__table { flex-direction: column; border: 1px solid var(--line); border-radius: 5px; overflow: hidden; }
.dlog__head { flex-direction: row; background-color: var(--panel); border-bottom: 1px solid var(--line); }
.dlog__th {
    position: relative;
    flex: 0 1 auto;
    min-width: 36px;
    padding: 2px 6px;
    font-family: var(--mono);
    font-size: 9px;
    letter-spacing: 0.09em;
    text-transform: uppercase;
    color: var(--ink-faint);
    border-right: 1px solid var(--line);
    overflow: hidden;
}
.dlog__th.flex { flex: 1 1 0; border-right: 0; }
.dlog__th.num { text-align: right; }
.dlog__sort { cursor: pointer; }
.dlog__sort:hover, .dlog__sort:focus-visible, .dlog__sort.on { color: var(--ink); }
.dlog__grip { position: absolute; top: 0; bottom: 0; right: 0; width: 7px; cursor: col-resize; }
.dlog__grip:hover, .dlog__grip:focus-visible, .dlog__grip.on { background-color: var(--accent); }
.dlog__list { height: 260px; }
.dlog__row { flex-direction: row; height: 20px; align-items: center; font-family: var(--mono); font-size: 11px; cursor: pointer; }
.dlog__row:hover, .dlog__row:focus-visible { background-color: var(--panel-2); }
.dlog__row.on { background-color: var(--panel-3); }
.dlog__cell { flex: 0 1 auto; min-width: 36px; padding: 0 6px; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; color: var(--ink); }
.dlog__cell.dim { color: var(--ink-dim); }
.dlog__cell.num { text-align: right; }
.dlog__cell.flex { flex: 1 1 0; min-width: 0; }
.dlog__tip { width: 100%; min-width: 0; }
.dlog__empty { padding: 6px 8px; color: var(--ink-dim); font-size: 12px; }
.dlog__detail {
    position: relative;
    z-index: 2;
    height: 236px;
    margin: 0 4px;
    padding: 6px 8px;
    border: 1px solid var(--line);
    border-radius: 5px;
    background-color: var(--panel-2);
    overflow: auto;
    flex-direction: column;
    gap: 6px;
}
.dlog__foot { flex-direction: row; align-items: center; gap: 8px; }
.dlog__stats { flex: 1 1 auto; flex-direction: row; flex-wrap: wrap; gap: 10px; min-width: 0; }
.dlog__stat { flex-direction: row; gap: 4px; align-items: baseline; font-family: var(--mono); font-size: 11px; color: var(--ink-faint); white-space: nowrap; }
.dlog__stat_value { color: var(--ink); font-weight: 500; }
.dlog__stat.warn, .dlog__stat.warn .dlog__stat_value { color: oklch(0.8 0.14 85); }
.dlog__armed { color: var(--danger); border-color: var(--danger); }
.dlog__pop { position: relative; flex: 0 0 auto; }
.dlog__menu {
    position: absolute; right: 0; bottom: 30px; z-index: 70;
    width: 180px;
    flex-direction: column; gap: 2px; padding: 4px;
    border: 1px solid var(--line-strong); border-radius: 6px;
    background-color: var(--panel-3);
    box-shadow: 0 14px 36px rgba(0, 0, 0, 0.55);
}
.dlog__menu .btn { justify-content: flex-start; border-color: transparent; background-color: transparent; }
.dlog__player { flex-direction: row; align-items: center; gap: 8px; }
.dlog__player .dk-num { min-width: 80px; }
"#
);

pub fn face(store: Store, node: String) -> impl IntoView {
    panel(store, node)
}

pub fn panel(store: Store, sink: String) -> impl IntoView {
    kit_decoders::install();
    install_stylesheet("decoder-log", SHEET);
    let log = Log::new(store, sink);
    let body = move || {
        if log.group.get().is_some() {
            AnyView::new(groups::table(log))
        } else {
            AnyView::new(list::table(log))
        }
    };
    view! {
        column(class = "face dlog", {..kit_decoders::no_pan()}) {
            {toolbar(log)}
            {notices(log)}
            {body}
            {footer::footer(log)}
        }
    }
}

fn choice(label: &'static str, title: &'static str, body: AnyView) -> AnyView {
    kit_decoders::titled(
        title.to_owned(),
        "dlog__choice",
        AnyView::new(view! {
            row(class = "dlog__choice") {
                text(class = "legend") {{label}}
                {body}
            }
        }),
    )
}

fn toolbar(log: Log) -> impl IntoView {
    let limits: Vec<(u32, String)> = LIMIT_OPTIONS
        .iter()
        .map(|limit| (*limit, limit.to_string()))
        .collect();
    let views: Vec<(LogView, String)> = VIEW_CHOICES
        .iter()
        .map(|(view, label)| (*view, (*label).to_owned()))
        .collect();
    let limit = Signal::derive(move || Some(log.filter.get().limit));
    let view = Signal::derive(move || Some(LogView::of(log.group.get())));
    view! {
        row(class = "dlog__bar") {
            box(class = "dlog__search") {
                Input(
                    value = log.search,
                    class = "native-input",
                    label = "Search decoder log",
                    placeholder = "Search station or summary",
                )
            }
            {choice("Group", "One row per frequency or station", AnyView::new(pick(views, view, move |view: LogView| log.set_group(view.group()))))}
            {choice("Rows", "Row limit", AnyView::new(pick(limits, limit, move |limit| log.set_limit(limit))))}
        }
    }
}

fn notices(log: Log) -> impl IntoView {
    let rejected = move || {
        log.rejected.get().map(|error| {
            AnyView::new(kit_decoders::alert(
                format!("Rejected: {error}"),
                move || log.rejected.set(None),
            ))
        })
    };
    let unavailable = move || {
        log.unavailable.get().map(|error| {
            AnyView::new(
                view! { row(class = "dk-alert") { text {{format!("Log unavailable: {error}")}} } },
            )
        })
    };
    view! { column(class = "dk-pane") { {rejected} {unavailable} } }
}

pub(super) fn empty_hint(log: Log) -> Option<AnyView> {
    let totals = log.totals();
    (totals.shown == 0).then(|| {
        let said = if !totals.loaded {
            "Loading…"
        } else if log.filter.with(|filter| filter.filtered()) {
            "No rows match this filter."
        } else {
            "Nothing logged yet."
        };
        AnyView::new(view! { text(class = "dlog__empty") {{said}} })
    })
}
