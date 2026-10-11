use std::sync::Arc;

use sdrmm_wire::rest::LogGroupKey;
use zgui::prelude::*;
use zgui_ui::prelude::*;

use crate::decoders::{
    groups::{GroupRow, GroupSort, format_airtime, has_airtime, sort_groups},
    log::{DETAIL_SLOTS, Slot, kind_label, slot_at, slot_count},
    views::format_clock,
};

use super::{
    detail, empty_hint,
    list::{ROW_HEIGHT, tip_cell, toggle_keys},
    state::Log,
};

struct Column {
    label: &'static str,
    width: Option<f32>,
    sort: Option<GroupSort>,
    right: bool,
}

fn columns(by: LogGroupKey, airtime: bool) -> Vec<Column> {
    let frequency = by == LogGroupKey::Frequency;
    let mut columns = vec![
        Column {
            label: if frequency { "Frequency" } else { "Station" },
            width: Some(if frequency { 116.0 } else { 152.0 }),
            sort: Some(GroupSort::Key),
            right: frequency,
        },
        Column {
            label: "Kind",
            width: Some(96.0),
            sort: None,
            right: false,
        },
        Column {
            label: "Hits",
            width: Some(76.0),
            sort: Some(GroupSort::Count),
            right: true,
        },
    ];
    if airtime {
        columns.push(Column {
            label: "Airtime",
            width: Some(72.0),
            sort: None,
            right: true,
        });
    }
    columns.push(Column {
        label: "Last",
        width: Some(84.0),
        sort: Some(GroupSort::Last),
        right: false,
    });
    columns.push(Column {
        label: "Latest",
        width: None,
        sort: None,
        right: false,
    });
    columns
}

fn width_of(column: &Column) -> Option<String> {
    column.width.map(|px| format!("{px}px"))
}

fn head_cell(log: Log, column: &Column) -> AnyView {
    let class = match (column.width, column.right) {
        (None, _) => "dlog__th flex",
        (Some(_), true) => "dlog__th num",
        (Some(_), false) => "dlog__th",
    };
    let Some(sort) = column.sort else {
        return AnyView::new(
            view! { text(class = class, style:width = width_of(column)) {{column.label}} },
        );
    };
    let arrow = move || {
        (log.group_sort.get() == sort).then_some(if sort == GroupSort::Key {
            " ↑"
        } else {
            " ↓"
        })
    };
    AnyView::new(view! {
        control(
            class = format!("{class} dlog__sort"),
            class:on = move || log.group_sort.get() == sort,
            style:width = width_of(column),
            tabindex = Focus::Sequential,
            a11y:role = Role::Button,
            a11y:label = format!("Sort by {}", column.label),
            on:click:stop = move |_| log.group_sort.set(sort)
        ) {
            {column.label}
            {arrow}
        }
    })
}

fn sorted(log: Log, by: LogGroupKey) -> Arc<Vec<GroupRow>> {
    let rows = log.groups.with(|page| {
        page.as_ref()
            .filter(|page| page.by == Some(by))
            .map(|page| Arc::clone(&page.rows))
            .unwrap_or_default()
    });
    Arc::new(sort_groups(&rows, log.group_sort.get(), by))
}

pub fn table(log: Log) -> impl IntoView {
    let by = Memo::new(move |_| log.group.get().unwrap_or(LogGroupKey::Frequency));
    let rows = Memo::new(move |_| sorted(log, by.get()));
    let airtime = Memo::new(move |_| rows.with(|rows| has_airtime(rows)));
    let opened = Memo::new(move |_| {
        let key = log.opened_group.get()?;
        rows.with(|rows| rows.iter().position(|row| row.key == key))
    });
    let count =
        Signal::derive_local(move || slot_count(rows.with(|rows| rows.len()), opened.get()));
    let head = move || {
        let cells: Vec<AnyView> = columns(by.get(), airtime.get())
            .iter()
            .map(|column| head_cell(log, column))
            .collect();
        view! { row(class = "dlog__head") {{cells}} }
    };
    view! {
        column(class = "dlog__table") {
            {head}
            {move || empty_hint(log)}
            VirtualList(
                count = count,
                row_size = ROW_HEIGHT,
                overscan = DETAIL_SLOTS + 2,
                class = "dlog__list",
                label = "Decoder log groups",
                row = move |index: usize| move || {
                    let rows = rows.get();
                    match slot_at(index, rows.len(), opened.get()) {
                        Slot::Row(at) => rows.get(at).cloned().map(|row| AnyView::new(group_row(log, row, by.get(), airtime.get()))),
                        Slot::Detail(at) => rows.get(at).map(|row| detail::panel(log.store, &row.latest)),
                        Slot::Spacer => None,
                    }
                }
            )
        }
    }
}

fn fixed(width: f32) -> Option<Signal<f32>> {
    Some(Signal::derive(move || width))
}

fn group_row(log: Log, row: GroupRow, by: LogGroupKey, airtime: bool) -> impl IntoView {
    let frequency = by == LogGroupKey::Frequency;
    let key = row.key.clone();
    let pressed = row.key.clone();
    let marked = row.key.clone();
    let airtime = airtime.then(|| {
        let shown = format_airtime(row.airtime_ms);
        view! { text(class = "dlog__cell num dim", style:width = Some("72px".to_owned())) {{shown}} }
    });
    let span = format!("First {}\nLast {}", row.first_at, row.last_at);
    view! {
        row(
            class = "dlog__row",
            class:on = move || log.opened_group.get().as_deref() == Some(marked.as_str()),
            tabindex = Focus::Sequential,
            a11y:role = Role::Button,
            a11y:label = format!("{} {}", row.label, row.latest.summary),
            on:key_down = toggle_keys(move || log.toggle_group(&pressed)),
            on:click:stop = move |_| log.toggle_group(&key)
        ) {
            {tip_cell(row.label.clone(), row.label.clone(), if frequency { "dlog__cell num" } else { "dlog__cell" }, fixed(if frequency { 116.0 } else { 152.0 }))}
            text(class = "dlog__cell dim", style:width = Some("96px".to_owned())) {{kind_label(&row.kind)}}
            text(class = "dlog__cell num", style:width = Some("76px".to_owned())) {{row.count.to_string()}}
            {airtime}
            {tip_cell(format_clock(&row.last_at), span, "dlog__cell dim", fixed(84.0))}
            {tip_cell(row.latest.summary.clone(), row.latest.summary.clone(), "dlog__cell flex", None)}
        }
    }
}
