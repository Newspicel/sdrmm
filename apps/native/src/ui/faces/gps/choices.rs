use sdrmm_wire::{
    phone::{Phone, PhonesResponse},
    position::{DEFAULT_GPSD_ADDRESS, NmeaDeviceInfo, NmeaDevicesResponse, PositionSource},
    ws::{ServerEvent, StateScope},
};
use zgui::prelude::*;

use super::source::{
    GpsTab, filter_nmea_devices, gps_tabs, nmea_detail, nmea_source, tab_hint, valid_gpsd_address,
};
use crate::{
    store::Store,
    ui::{
        kit_audio::{titled, titled_block, titled_fill},
        kit_sources::{NumberSpec, Tone, button, draft_field, number_field},
    },
};

const SHEET: &str = css!(
    r#"
.gps-dot { width: 8px; height: 8px; border-radius: 999px; flex: 0 0 auto; background-color: var(--ink-faint); }
.gps-dot.on { background-color: var(--ok); }
"#
);

const SEARCH_FROM: usize = 4;

pub type Choose = std::rc::Rc<dyn Fn(PositionSource)>;

pub type Receivers = RwSignal<Option<Result<Vec<NmeaDeviceInfo>, String>>>;

pub fn receivers(store: Store) -> Receivers {
    let found: Receivers = RwSignal::new(None);
    zgui::task::spawn_local(async move {
        let fetched = store
            .api()
            .get::<NmeaDevicesResponse>("/api/position/nmea-devices")
            .await
            .map(|response| response.devices)
            .map_err(|error| error.to_string());
        found.try_set(Some(fetched));
    });
    found
}

pub type Phones = RwSignal<Option<Result<Vec<Phone>, String>>>;

fn fetch_phones(store: Store, found: Phones) {
    zgui::task::spawn_local(async move {
        let fetched = store
            .api()
            .get::<PhonesResponse>("/api/phones")
            .await
            .map(|response| response.phones)
            .map_err(|error| error.to_string());
        found.try_set(Some(fetched));
    });
}

pub fn phones(store: Store) -> Phones {
    let found: Phones = RwSignal::new(None);
    fetch_phones(store, found);
    store.on_event(move |event| {
        if matches!(
            event,
            ServerEvent::StateChanged {
                scope: StateScope::Phones
            }
        ) {
            fetch_phones(store, found);
        }
    });
    found
}

fn tabs(tab: RwSignal<GpsTab>) -> impl IntoView {
    let items: Vec<AnyView> = gps_tabs()
        .into_iter()
        .map(|(value, label)| {
            titled_fill(
                tab_hint(value),
                AnyView::new(view! {
                    control(
                        class = "seg__item",
                        tabindex = Focus::Sequential,
                        a11y:role = Role::Tab,
                        class:on = move || tab.get() == value,
                        on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                        on:click:stop = move |_| tab.set(value)
                    ) {
                        {label}
                    }
                }),
            )
        })
        .collect();
    view! { row(class = "seg kit-tabs", a11y:role = Role::TabList) {{items}} }
}

pub fn gps_choices(store: Store, choose: Choose) -> impl IntoView {
    install_stylesheet("gps-choices", SHEET);
    let tab = RwSignal::new(GpsTab::Receiver);
    let found = receivers(store);
    let body = move || match tab.get() {
        GpsTab::Receiver => AnyView::new(receiver_choices(found, choose.clone())),
        GpsTab::Network => AnyView::new(gpsd_form(choose.clone())),
        GpsTab::Fixed => AnyView::new(fixed_form(choose.clone())),
        GpsTab::Phone => AnyView::new(phone_choices(phones(store), choose.clone())),
    };
    view! {
        column(class = "kit-choices") {
            {tabs(tab)}
            {body}
        }
    }
}

fn receiver_choices(found: Receivers, choose: Choose) -> impl IntoView {
    let query = RwSignal::new_local(String::new());
    let listed = Signal::derive(move || found.get().and_then(Result::ok).unwrap_or_default());
    let shown = Signal::derive_local(move || filter_nmea_devices(&listed.get(), &query.get()));
    let pick = choose.clone();
    let list = move || {
        shown
            .get()
            .into_iter()
            .map(|device| {
                let detail = nmea_detail(&device);
                let path = device.path.clone();
                let choose = pick.clone();
                AnyView::new(view! {
                    control(
                        class = "kit-choice",
                        tabindex = Focus::Sequential,
                        a11y:role = Role::Button,
                        on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                        on:click:stop = move |_| choose(nmea_source(&path))
                    ) {
                        text {{device.path.clone()}}
                        text(class = "kit-choice__meta", hidden = detail.is_empty()) {{detail}}
                    }
                })
            })
            .collect::<Vec<_>>()
    };
    let said = move || match found.get() {
        None => Some("Looking for receivers…".to_owned()),
        Some(Err(_)) => Some("Serial device discovery failed".to_owned()),
        Some(Ok(all)) if shown.with(Vec::is_empty) => Some(if all.is_empty() {
            "No serial receiver found.".to_owned()
        } else {
            "No receiver matches that.".to_owned()
        }),
        Some(Ok(_)) => None,
    };
    view! {
        column(class = "kit-choices") {
            box(hidden = move || listed.with(Vec::len) < SEARCH_FROM) {
                {draft_field("Search receivers", query, "Search receivers", Signal::derive_local(|| false), || {})}
            }
            column(class = "kit-list") {{list}}
            {move || said().map(|said| AnyView::new(view! { text(class = "kit-note") {{said}} }))}
            {path_form(choose)}
        }
    }
}

fn phone_choices(found: Phones, choose: Choose) -> impl IntoView {
    let list = move || {
        found
            .get()
            .and_then(Result::ok)
            .unwrap_or_default()
            .into_iter()
            .map(|phone| {
                let id = phone.id.clone();
                let choose = choose.clone();
                let online = phone.online;
                let standing = if online { "Online" } else { "Offline" };
                AnyView::new(view! {
                    control(
                        class = "kit-choice",
                        tabindex = Focus::Sequential,
                        a11y:role = Role::Button,
                        on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                        on:click:stop = move |_| choose(PositionSource::Phone { phone: id.clone() })
                    ) {
                        box(class = "gps-dot", class:on = online)
                        text {{phone.name.clone()}}
                        text(class = "kit-choice__meta") {{standing}}
                    }
                })
            })
            .collect::<Vec<_>>()
    };
    let said = move || match found.get() {
        None => Some(AnyView::new(
            view! { text(class = "kit-note") {"Looking for phones\u{2026}"} },
        )),
        Some(Err(_)) => Some(AnyView::new(
            view! { text(class = "kit-alert") {"Phone list failed"} },
        )),
        Some(Ok(all)) if all.is_empty() => Some(titled(
            "Pair one in Library > Phones",
            AnyView::new(view! { text(class = "kit-note") {"No phones"} }),
        )),
        Some(Ok(_)) => None,
    };
    view! {
        column(class = "kit-choices") {
            column(class = "kit-list") {{list}}
            {said}
        }
    }
}

fn path_form(choose: Choose) -> impl IntoView {
    let path = RwSignal::new_local(String::new());
    let read = move || {
        let trimmed = path.get_untracked().trim().to_owned();
        if !trimmed.is_empty() {
            choose(nmea_source(&trimmed));
        }
    };
    let empty = Signal::derive(move || path.with(|path| path.trim().is_empty()));
    titled_block(
        "The serial port of a receiver that is not listed above",
        AnyView::new(view! {
            row(class = "kit-pop__row") {
                text(class = "kit-legend") {"Path"}
                {draft_field("Serial device path", path, "/dev/ttyUSB0", Signal::derive_local(|| false), read.clone())}
                {button(|| "Read".to_owned(), Tone::Plain, empty, read)}
            }
        }),
    )
}

fn gpsd_form(choose: Choose) -> impl IntoView {
    let address = RwSignal::new_local(DEFAULT_GPSD_ADDRESS.to_owned());
    let invalid = Signal::derive_local(move || !valid_gpsd_address(address.get().trim()));
    let read = move || {
        let typed = address.get_untracked().trim().to_owned();
        if valid_gpsd_address(&typed) {
            choose(PositionSource::Gpsd { address: typed });
        }
    };
    let blocked = Signal::derive(move || !valid_gpsd_address(address.get().trim()));
    titled_block(
        "Host and port of a gpsd daemon, on this machine or another one",
        AnyView::new(view! {
            row(class = "kit-pop__row") {
                text(class = "kit-legend") {"gpsd"}
                {draft_field("GPSD address", address, DEFAULT_GPSD_ADDRESS, invalid, read.clone())}
                {button(|| "Read".to_owned(), Tone::Plain, blocked, read)}
            }
        }),
    )
}

fn fixed_form(choose: Choose) -> impl IntoView {
    let lat = RwSignal::new(0.0);
    let lon = RwSignal::new(0.0);
    let latitude = NumberSpec::unit("°").within(-90.0, 90.0).step(0.00001);
    let longitude = NumberSpec::unit("°").within(-180.0, 180.0).step(0.00001);
    view! {
        row(class = "kit-pop__row") {
            text(class = "kit-legend") {"Lat"}
            {number_field("Latitude", lat.into(), latitude, Signal::stored(false), move |value| lat.set(value))}
            text(class = "kit-legend") {"Lon"}
            {number_field("Longitude", lon.into(), longitude, Signal::stored(false), move |value| lon.set(value))}
            {titled("Use these coordinates as the position", AnyView::new(button(|| "Set".to_owned(), Tone::Plain, Signal::stored(false), move || {
                choose(PositionSource::Fixed { lat: lat.get_untracked(), lon: lon.get_untracked(), altitude_m: None });
            })))}
        }
    }
}
