use std::sync::Arc;

use sdrmm_wire::decode::{DecodedRecord, DecoderEvent};
use zgui::prelude::*;

use crate::{
    decoders::{
        text::{
            celsius, climb_rate, hectopascal, metres, metres_per_second, millimetres, whole_percent,
        },
        views::{
            AprsWeatherStation, DecoderScope, RadiosondeStation, aprs_weather_stations,
            format_clock, format_position, radiosonde_log, radiosonde_stations, wind,
        },
    },
    store::Store,
};

use super::{
    frames_of, records,
    table::{self, age, cell, dash, empty, ticking},
};

const SONDE_LOG_LIMIT: usize = 50;

fn aprs_row(station: AprsWeatherStation, now: i64) -> AnyView {
    let (heard, tone) = age(now, station.at_ms);
    let weather = &station.weather;
    let range = station
        .min_temperature_c
        .zip(station.max_temperature_c)
        .map(|(low, high)| {
            format!(
                "{} / {}",
                dash(celsius(Some(low))),
                dash(celsius(Some(high)))
            )
        });
    AnyView::new(table::row(vec![
        cell(station.source.clone(), tone),
        cell(dash(celsius(weather.temperature_c)), tone),
        cell(dash(range), tone),
        cell(dash(wind(weather)), tone),
        cell(
            dash(metres_per_second(station.max_gust_ms.map(f64::from))),
            tone,
        ),
        cell(
            dash(whole_percent(weather.humidity_pct.map(f64::from))),
            tone,
        ),
        cell(dash(hectopascal(weather.pressure_hpa)), tone),
        cell(dash(millimetres(weather.rain_1h_mm)), tone),
        cell(heard, tone),
    ]))
}

pub fn aprs_view(store: Store, scope: DecoderScope) -> impl IntoView {
    let frames = frames_of(store, "aprs");
    let now = ticking();
    move || {
        let stations = aprs_weather_stations(&records(frames, scope));
        if stations.is_empty() {
            return empty("No weather station heard yet.");
        }
        let at = now.get();
        let rows: Vec<AnyView> = stations.into_iter().map(|s| aprs_row(s, at)).collect();
        AnyView::new(view! {
            column(class = "dk-table") {
                {table::head(&[
                    ("Station", None),
                    ("Temp", None),
                    ("Min / max", Some("Lowest and highest temperature in the kept reports")),
                    ("Wind", None),
                    ("Gust", Some("Strongest gust in the kept reports")),
                    ("RH", Some("Relative humidity")),
                    ("Pressure", None),
                    ("Rain", Some("Rain in the last hour")),
                    ("Heard", None),
                ])}
                {rows}
            }
        })
    }
}

fn serial_cell(serial: String, chosen: bool, selected: RwSignal<Option<String>>) -> AnyView {
    let picked = serial.clone();
    AnyView::new(view! {
        control(
            class = "dk-td dk-sel",
            class:on = chosen,
            tabindex = Focus::Sequential,
            a11y:role = Role::Button,
            a11y:label = format!("Show frames of {serial}"),
            on:click:stop = move |_| selected.set(Some(picked.clone()))
        ) {
            {serial.clone()}
        }
    })
}

fn sonde_row(
    sonde: RadiosondeStation,
    now: i64,
    chosen: bool,
    selected: RwSignal<Option<String>>,
) -> AnyView {
    let (heard, tone) = age(now, sonde.at_ms);
    let max = metres(sonde.max_altitude_m).map(|top| {
        if sonde.burst {
            format!("{top} burst")
        } else {
            top
        }
    });
    AnyView::new(table::row(vec![
        serial_cell(sonde.serial, chosen, selected),
        cell(sonde.sonde.label().to_owned(), tone),
        cell(dash(metres(sonde.altitude_m)), tone),
        cell(dash(max), tone),
        cell(dash(climb_rate(sonde.climb_ms)), tone),
        cell(dash(celsius(sonde.temperature_c)), tone),
        cell(dash(whole_percent(sonde.humidity_pct.map(f64::from))), tone),
        cell(dash(hectopascal(sonde.pressure_hpa)), tone),
        cell(heard, tone),
        cell(dash(sonde.frame), tone),
    ]))
}

fn log_row(record: &Arc<DecodedRecord>) -> Option<AnyView> {
    let DecoderEvent::Radiosonde(frame) = &record.event else {
        return None;
    };
    Some(AnyView::new(table::cells(vec![
        format_clock(&record.at),
        dash(frame.frame),
        dash(metres(frame.altitude_m)),
        dash(climb_rate(frame.climb_ms)),
        dash(celsius(frame.temperature_c)),
        format_position(frame.lat, frame.lon),
    ])))
}

pub fn radiosonde_view(store: Store, scope: DecoderScope) -> impl IntoView {
    let frames = frames_of(store, "radiosonde");
    let now = ticking();
    let selected = RwSignal::new(None::<String>);
    move || {
        let held = records(frames, scope);
        let sondes = radiosonde_stations(&held);
        let wanted = selected.get();
        let Some(open) = sondes
            .iter()
            .find(|sonde| Some(&sonde.serial) == wanted.as_ref())
            .or_else(|| sondes.first())
            .map(|sonde| sonde.serial.clone())
        else {
            return empty("No sonde heard yet.");
        };
        let at = now.get();
        let rows: Vec<AnyView> = sondes
            .into_iter()
            .map(|sonde| {
                let chosen = sonde.serial == open;
                sonde_row(sonde, at, chosen, selected)
            })
            .collect();
        let log: Vec<AnyView> = radiosonde_log(&held, &open, SONDE_LOG_LIMIT)
            .iter()
            .filter_map(log_row)
            .collect();
        AnyView::new(view! {
            column(class = "dk-pane") {
                column(class = "dk-table") {
                    {table::head(&[
                        ("Serial", None),
                        ("Type", None),
                        ("Altitude", None),
                        ("Max", Some("Highest altitude reached")),
                        ("Climb", None),
                        ("Temp", None),
                        ("RH", Some("Relative humidity")),
                        ("Pressure", None),
                        ("Heard", None),
                        ("Frame", None),
                    ])}
                    {rows}
                }
                text(class = "legend") {{open.clone()}}
                scroll(class = "dk-table dk-scroll") {
                    {table::plain(&["Time", "Frame", "Altitude", "Climb", "Temp", "Position"])}
                    {log}
                }
            }
        })
    }
}
