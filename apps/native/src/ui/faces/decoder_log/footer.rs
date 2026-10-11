use zgui::prelude::*;

use crate::{
    decoders::log::{DOWNLOADS, Download, dropped_notice, export_name, export_path},
    ui::{kit_channel::Popover, kit_decoders},
};

use super::state::Log;

fn stat(label: &'static str, value: String, title: String, warn: bool) -> AnyView {
    kit_decoders::titled(
        title,
        "dlog__stat",
        AnyView::new(view! {
            row(class = "dlog__stat", class:warn = warn) {
                text {{label}}
                text(class = "dlog__stat_value") {{value}}
            }
        }),
    )
}

fn stats(log: Log) -> impl IntoView {
    move || {
        let totals = log.totals();
        let grouped = log.group.get().is_some();
        let mut shown = vec![
            stat(
                "Shown",
                totals.shown.to_string(),
                "Rows in this view".to_owned(),
                false,
            ),
            if grouped {
                stat(
                    "Groups",
                    totals.total.to_string(),
                    "Groups in the stored rows".to_owned(),
                    false,
                )
            } else {
                stat(
                    "Stored",
                    totals.total.to_string(),
                    "Rows stored for this node".to_owned(),
                    false,
                )
            },
        ];
        if let Some(cleared) = log.cleared.get() {
            shown.push(stat(
                "Cleared",
                cleared.to_string(),
                "Rows removed by the last clear".to_owned(),
                false,
            ));
        }
        if let Some(notice) = dropped_notice(totals.lost, totals.dropped) {
            shown.push(stat(
                "Dropped",
                (totals.lost + totals.dropped).to_string(),
                notice,
                true,
            ));
        }
        view! { row(class = "dlog__stats") {{shown}} }
    }
}

fn save(log: Log, download: Download) {
    let query = log
        .filter
        .get_untracked()
        .export_query(&log.sink.get_value());
    kit_decoders::save_download(
        log.store,
        export_path(download.format, &query),
        export_name(download.format, jiff::Timestamp::now()),
    );
}

fn downloads(log: Log) -> impl IntoView {
    let popover = Popover::new();
    let menu = move || {
        popover.open().then(|| {
            let items: Vec<AnyView> = DOWNLOADS
                .iter()
                .map(|download| {
                    let download = *download;
                    kit_decoders::titled(
                        download.hint.to_owned(),
                        "dlog__item",
                        AnyView::new(view! {
                            control(
                                class = "btn",
                                tabindex = Focus::Sequential,
                                a11y:role = Role::Button,
                                on:click:stop = move |_| {
                                    popover.close();
                                    save(log, download);
                                }
                            ) {{download.label}}
                        }),
                    )
                })
                .collect();
            AnyView::new(view! {
                column(
                    class = "dlog__menu",
                    on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation()
                ) {{items}}
            })
        })
    };
    view! {
        box(class = "dlog__pop") {
            control(
                class = "btn",
                class:on = move || popover.open(),
                tabindex = Focus::Sequential,
                a11y:role = Role::Button,
                on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                    ev.stop_propagation();
                    popover.toggle();
                }
            ) {"Download"}
            {menu}
        }
    }
}

fn clear_button(log: Log) -> impl IntoView {
    let button = move || {
        let armed = log.armed.get();
        AnyView::new(view! {
            control(
                class = "btn",
                class:dlog__armed = armed,
                state:disabled = move || log.clearing.get(),
                on:click:stop = move |_| log.press_clear()
            ) {
                {if armed { "Confirm clear" } else { "Clear" }}
            }
        })
    };
    move || {
        if log.armed.get() {
            kit_decoders::titled(
                "Removes every stored row this node can see".to_owned(),
                "dlog__item",
                button(),
            )
        } else {
            button()
        }
    }
}

pub fn footer(log: Log) -> impl IntoView {
    view! {
        row(class = "face__foot dlog__foot") {
            {stats(log)}
            {downloads(log)}
            {clear_button(log)}
        }
    }
}
