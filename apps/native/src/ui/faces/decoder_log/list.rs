use zgui::prelude::*;
use zgui_ui::prelude::*;

use crate::{
    decoders::{
        groups::format_mhz,
        log::{DETAIL_SLOTS, LogRow, Slot, kind_label, slot_at, slot_count},
        views::format_clock,
    },
    ui::kit_decoders::titled_text,
};

use super::{columns, detail, empty_hint, state::Log};

pub const ROW_HEIGHT: f32 = 20.0;

pub fn table(log: Log) -> impl IntoView {
    let opened = Memo::new(move |_| {
        let key = log.opened.get()?;
        log.rows
            .with(|rows| rows.iter().position(|row| row.key == key))
    });
    let count =
        Signal::derive_local(move || slot_count(log.rows.with(|rows| rows.len()), opened.get()));
    view! {
        column(class = "dlog__table") {
            {columns::header(log)}
            {move || empty_hint(log)}
            VirtualList(
                count = count,
                row_size = ROW_HEIGHT,
                overscan = DETAIL_SLOTS + 2,
                class = "dlog__list",
                label = "Decoder log",
                row = move |index: usize| move || {
                    let rows = log.rows.get();
                    match slot_at(index, rows.len(), opened.get()) {
                        Slot::Row(at) => rows.get(at).cloned().map(|row| AnyView::new(row_view(log, row))),
                        Slot::Detail(at) => rows.get(at).map(|row| detail::panel(log.store, row)),
                        Slot::Spacer => None,
                    }
                }
            )
        }
    }
}

pub fn toggle_keys(toggle: impl Fn() + 'static) -> impl Fn(&mut EventCx<'_, events::KeyDown>) {
    move |ev: &mut EventCx<'_, events::KeyDown>| {
        let pressed = ev.key == Key::Named(NamedKey::Enter) || ev.key.inserted_text() == Some(" ");
        if pressed {
            ev.prevent_default();
            toggle();
        }
    }
}

pub fn tip_cell(
    text: String,
    title: String,
    class: &'static str,
    width: Option<Signal<f32>>,
) -> AnyView {
    let style = move || width.map(|width| format!("{}px", width.get()));
    AnyView::new(view! {
        box(class = class, style:width = style) {
            {titled_text(text, title, "dlog__tip")}
        }
    })
}

fn row_view(log: Log, row: LogRow) -> impl IntoView {
    let key = row.key.clone();
    let pressed = row.key.clone();
    let marked = row.key.clone();
    let widths = log.widths;
    let width = move |index: usize| Signal::derive(move || widths.get().0[index]);
    let station = row.station.clone().unwrap_or_else(|| "-".to_owned());
    let station_title = row.station.clone().unwrap_or_default();
    view! {
        row(
            class = "dlog__row",
            class:on = move || log.opened.get().as_deref() == Some(marked.as_str()),
            tabindex = Focus::Sequential,
            a11y:role = Role::Button,
            a11y:label = format!("{} {}", kind_label(&row.kind), row.summary),
            on:key_down = toggle_keys(move || log.toggle(&pressed)),
            on:click:stop = move |_| log.toggle(&key)
        ) {
            {tip_cell(format_clock(&row.at), row.at.clone(), "dlog__cell dim", Some(width(0)))}
            text(class = "dlog__cell dim", style:width = move || Some(format!("{}px", width(1).get()))) {{kind_label(&row.kind)}}
            text(class = "dlog__cell num", style:width = move || Some(format!("{}px", width(2).get()))) {{format_mhz(row.freq_hz)}}
            {tip_cell(station, station_title, "dlog__cell", Some(width(3)))}
            {tip_cell(row.summary.clone(), row.summary.clone(), "dlog__cell flex", None)}
        }
    }
}
