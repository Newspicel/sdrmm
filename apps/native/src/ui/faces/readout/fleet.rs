use zgui::prelude::*;

use crate::{
    decoders::views::{
        BleDevice, DecoderScope, Drone, LoraStation, ble_devices, lora_stations, remote_id_drones,
    },
    store::Store,
};

use super::{
    frames_of, records,
    table::{self, age, cell, dash, empty, long_cell, ticking},
};

fn table_of(columns: &[table::Column], rows: Vec<AnyView>) -> AnyView {
    AnyView::new(view! {
        column(class = "dk-table") {
            {table::head(columns)}
            {rows}
        }
    })
}

fn lora_row(station: LoraStation, now: i64) -> AnyView {
    let (heard, tone) = age(now, station.at_ms);
    AnyView::new(table::row(vec![
        cell(station.key, tone),
        cell(dash(station.name), tone),
        cell(station.protocol.to_owned(), tone),
        long_cell(station.message, tone),
        cell(format!("{:.1}", station.snr_db), tone),
        cell(station.frames.to_string(), tone),
        cell(heard, tone),
    ]))
}

pub fn lora_view(store: Store, scope: DecoderScope) -> impl IntoView {
    let frames = frames_of(store, "lora");
    let now = ticking();
    move || {
        let stations = lora_stations(&records(frames, scope));
        if stations.is_empty() {
            return empty("No LoRa nodes heard yet.");
        }
        let at = now.get();
        let rows = stations.into_iter().map(|s| lora_row(s, at)).collect();
        table_of(
            &[
                ("Node", None),
                ("Name", None),
                ("Protocol", None),
                ("Last", None),
                ("SNR dB", None),
                ("Frames", None),
                ("Heard", None),
            ],
            rows,
        )
    }
}

fn drone_row(drone: Drone, now: i64) -> AnyView {
    let (heard, tone) = age(now, drone.at_ms);
    AnyView::new(table::row(vec![
        cell(drone.key, tone),
        cell(drone.kind.to_owned(), tone),
        cell(dash(drone.height_m.map(|m| format!("{m:.0}"))), tone),
        cell(dash(drone.speed_mps.map(|v| format!("{v:.1}"))), tone),
        cell(dash(drone.pilot_m.map(|m| format!("{m:.0}"))), tone),
        long_cell(dash(drone.operator), tone),
        cell(drone.links.join(", "), tone),
        cell(format!("{:.0}", drone.level_dbfs), tone),
        cell(heard, tone),
    ]))
}

pub fn remote_id_view(store: Store, scope: DecoderScope) -> impl IntoView {
    let frames = frames_of(store, "remote_id");
    let now = ticking();
    move || {
        let drones = remote_id_drones(&records(frames, scope));
        if drones.is_empty() {
            return empty("No drones heard yet.");
        }
        let at = now.get();
        let rows = drones.into_iter().map(|d| drone_row(d, at)).collect();
        table_of(
            &[
                ("Drone", None),
                ("Type", None),
                ("Height m", None),
                ("Speed m/s", None),
                ("Pilot m", Some("Distance from drone to pilot")),
                ("Operator", None),
                ("Link", None),
                ("Level dBFS", None),
                ("Heard", None),
            ],
            rows,
        )
    }
}

fn ble_row(device: BleDevice, now: i64) -> AnyView {
    let (heard, tone) = age(now, device.at_ms);
    AnyView::new(table::row(vec![
        cell(device.key, tone),
        long_cell(dash(device.name), tone),
        cell(dash(device.vendor), tone),
        cell(dash(device.beacon), tone),
        cell(dash(device.channel), tone),
        cell(format!("{:.0}", device.level_dbfs), tone),
        cell(device.heard.to_string(), tone),
        cell(heard, tone),
    ]))
}

pub fn ble_view(store: Store, scope: DecoderScope) -> impl IntoView {
    let frames = frames_of(store, "ble");
    let now = ticking();
    move || {
        let devices = ble_devices(&records(frames, scope));
        if devices.is_empty() {
            return empty("No Bluetooth devices heard yet.");
        }
        let at = now.get();
        let rows = devices.into_iter().map(|d| ble_row(d, at)).collect();
        table_of(
            &[
                ("Address", None),
                ("Name", None),
                ("Vendor", None),
                ("Beacon", None),
                ("Ch", None),
                ("Level dBFS", None),
                ("Adverts", Some("Adverts heard, repeats included")),
                ("Heard", None),
            ],
            rows,
        )
    }
}
