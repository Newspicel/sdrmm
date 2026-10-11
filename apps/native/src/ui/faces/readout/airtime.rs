use sdrmm_wire::{
    airtime::{IsmKindLoad, WifiChannelLoad},
    decode::DecoderEvent,
};
use zgui::prelude::*;

use crate::{
    decoders::{
        detail::band_label,
        text::share,
        views::{DecoderScope, latest_record},
    },
    store::Store,
    ui::kit_decoders::{load_bar, titled_text},
};

use super::{
    frames_of, records,
    table::{self, cell, dash, empty},
};

fn level(value: Option<f32>) -> String {
    dash(value.map(|v| format!("{v:.0}")))
}

fn channel_row(channel: &WifiChannelLoad) -> AnyView {
    AnyView::new(table::row(vec![
        titled_text(
            channel.number.to_string(),
            band_label(channel.band).to_owned(),
            "dk-td",
        ),
        cell(share(channel.busy), ""),
        AnyView::new(view! { box(class = "dk-td") {{load_bar(channel.busy)}} }),
        cell(level(channel.level_dbfs), ""),
        cell(level(channel.floor_dbfs), ""),
    ]))
}

pub fn wifi_view(store: Store, scope: DecoderScope) -> impl IntoView {
    let frames = frames_of(store, "wifi_occupancy");
    move || {
        let latest = latest_record(&records(frames, scope));
        let Some(DecoderEvent::WifiOccupancy(report)) = latest.as_ref().map(|r| &r.event) else {
            return empty("First report after one second.");
        };
        if report.channels.is_empty() {
            return empty("No Wi-Fi channel in view.");
        }
        let rows: Vec<AnyView> = report.channels.iter().map(channel_row).collect();
        AnyView::new(view! {
            column(class = "dk-table") {
                {table::head(&[
                    ("Channel", None),
                    ("Busy", None),
                    ("", None),
                    ("Level dBFS", Some("Mean level while busy")),
                    ("Floor dBFS", None),
                ])}
                {rows}
            }
        })
    }
}

fn kind_row(load: &IsmKindLoad) -> AnyView {
    let centres: Vec<String> = load.centres_mhz.iter().map(f64::to_string).collect();
    AnyView::new(table::row(vec![
        cell(load.kind.label().to_owned(), ""),
        cell(load.bursts.to_string(), ""),
        cell(share(load.airtime), ""),
        cell(format!("{}", load.mean_us.round()), ""),
        cell(format!("{:.0}", load.peak_dbfs), ""),
        table::long_cell(centres.join(", "), ""),
    ]))
}

pub fn ism_view(store: Store, scope: DecoderScope) -> impl IntoView {
    let frames = frames_of(store, "ism_survey");
    move || {
        let latest = latest_record(&records(frames, scope));
        let Some(DecoderEvent::IsmSurvey(report)) = latest.as_ref().map(|r| &r.event) else {
            return empty("First report after one second.");
        };
        let kinds = if report.kinds.is_empty() {
            empty("Nothing on air.")
        } else {
            let rows: Vec<AnyView> = report.kinds.iter().map(kind_row).collect();
            AnyView::new(view! {
                column(class = "dk-table") {
                    {table::plain(&["Kind", "Bursts", "Airtime", "Length µs", "Peak dBFS", "Where MHz"])}
                    {rows}
                }
            })
        };
        AnyView::new(view! {
            column(class = "dk-pane") {
                row(class = "dk-line") {
                    text(class = "legend") {"Busy"}
                    text(class = "dk-num") {{share(report.busy)}}
                    {load_bar(report.busy)}
                }
                {kinds}
            }
        })
    }
}
