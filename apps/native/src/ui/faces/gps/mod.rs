pub mod choices;
pub mod source;

use std::{rc::Rc, time::Duration};

use sdrmm_wire::{
    patch::NodeBody,
    position::{DEFAULT_NMEA_UPDATE_INTERVAL_MS, PositionFix, PositionSource},
};
use zgui::prelude::*;

use self::{
    choices::{Phones, gps_choices, phones, receivers},
    source::{
        BAUDS, PhoneStanding, fix_age_label, heading_label, nmea_suggestion, phone_standing,
        shown_as_standing, tilt_label, valid_baud, valid_gpsd_address,
    },
};
use super::device::actions::edit_body;
use crate::{
    head::{self, NodeHead},
    position::{grid_locator, positions},
    store::Store,
    ui::{
        kit_audio::{self, fault, titled},
        kit_sources::{
            NumberSpec, Tone, button, footer, install, number_field, popover_slot, readout,
            text_field,
        },
        widgets::{pick, row_field},
    },
};

const FIX_TICK: Duration = Duration::from_secs(1);

const RATES: [(u32, &str); 5] = [
    (1_000, "1 Hz"),
    (500, "2 Hz"),
    (200, "5 Hz"),
    (100, "10 Hz"),
    (50, "20 Hz"),
];

fn source_of(store: Store, node: &str) -> Option<PositionSource> {
    store.graph.with(|graph| {
        graph.node(node).and_then(|found| match &found.body {
            NodeBody::Gps(gps) => gps.source.clone(),
            _ => None,
        })
    })
}

fn set_source(store: Store, node: String, next: Option<PositionSource>) {
    edit_body(store, node, move |body| {
        if let NodeBody::Gps(gps) = body {
            gps.source = next;
        }
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    None,
    Phone,
    Gpsd,
    Fixed,
    Nmea,
}

fn kind_of(source: Option<&PositionSource>) -> Kind {
    match source {
        None => Kind::None,
        Some(PositionSource::Phone { .. }) => Kind::Phone,
        Some(PositionSource::Gpsd { .. }) => Kind::Gpsd,
        Some(PositionSource::Fixed { .. }) => Kind::Fixed,
        Some(PositionSource::Nmea { .. }) => Kind::Nmea,
    }
}

pub fn face(store: Store, node: String) -> impl IntoView {
    install();
    kit_audio::install();
    let source = {
        let node = node.clone();
        Memo::new(move |_| source_of(store, &node))
    };
    let kind = Memo::new(move |_| source.with(|source| kind_of(source.as_ref())));
    move || {
        let node = node.clone();
        if kind.get() == Kind::None {
            let chosen = node.clone();
            let choose: choices::Choose =
                Rc::new(move |next| set_source(store, chosen.clone(), Some(next)));
            return AnyView::new(view! { column(class = "face") { {gps_choices(store, choose)} } });
        }
        AnyView::new(with_source(store, node, kind.get_untracked(), source))
    }
}

fn with_source(
    store: Store,
    node: String,
    kind: Kind,
    source: Memo<Option<PositionSource>>,
) -> impl IntoView {
    let change = {
        let node = node.clone();
        move |next: PositionSource| set_source(store, node.clone(), Some(next))
    };
    let state = {
        let node = node.clone();
        Signal::derive(move || positions().and_then(|positions| positions.of(&node)))
    };
    let settings = match kind {
        Kind::Gpsd => AnyView::new(gpsd_settings(source, change)),
        Kind::Fixed => AnyView::new(fixed_settings(source, change)),
        Kind::Nmea => AnyView::new(nmea_settings(store, source, change)),
        Kind::Phone | Kind::None => {
            let found = phones(store);
            head::show(store, node.clone(), move || {
                standing_head(phone_standing_of(found, source, state))
            });
            AnyView::new(phone_settings(found, source, change))
        }
    };
    let now = kit_audio::ticker(FIX_TICK);
    let phone = kind == Kind::Phone;
    let fix = move || {
        let state = state.get()?;
        match state.fix {
            Some(fix) => Some(AnyView::new(fix_readout(&fix, phone, now))),
            None => state
                .error
                .filter(|error| !shown_as_standing(source.get().as_ref(), error))
                .map(|error| AnyView::new(view! { text(class = "kit-alert") {{error}} })),
        }
    };
    view! {
        column(class = "face") {
            column(class = "kit-radio") {{settings}}
            {fix}
            {footer(button(|| "Forget source".to_owned(), Tone::Quiet, Signal::stored(false), move || set_source(store, node.clone(), None)))}
        }
    }
}

#[must_use]
pub fn standing_head(standing: Option<PhoneStanding>) -> NodeHead {
    match standing {
        None => NodeHead::default(),
        Some(PhoneStanding::NotPaired) => {
            NodeHead::said(PhoneStanding::NotPaired.label()).toned(head::Tone::Danger)
        }
        Some(standing) => NodeHead::said(standing.label()),
    }
}

fn phone_standing_of(
    found: Phones,
    source: Memo<Option<PositionSource>>,
    state: Signal<Option<crate::position::PositionState>>,
) -> Option<PhoneStanding> {
    let Some(PositionSource::Phone { phone }) = source.get() else {
        return None;
    };
    let listed = found.get().and_then(Result::ok);
    let error = state.get().and_then(|state| state.error);
    phone_standing(listed.as_deref(), &phone, error.as_deref())
}

#[must_use]
pub fn phone_options(listed: &[sdrmm_wire::phone::Phone], current: &str) -> Vec<(String, String)> {
    let mut options: Vec<(String, String)> = listed
        .iter()
        .map(|phone| (phone.id.clone(), phone.name.clone()))
        .collect();
    if !options.iter().any(|(id, _)| id == current) {
        options.push((current.to_owned(), current.to_owned()));
    }
    options
}

fn phone_settings(
    found: Phones,
    source: Memo<Option<PositionSource>>,
    change: impl Fn(PositionSource) + Clone + 'static,
) -> impl IntoView {
    let current = Memo::new(move |_| match source.get() {
        Some(PositionSource::Phone { phone }) => phone,
        _ => String::new(),
    });
    let options = Memo::new(move |_| {
        let listed = found.get().and_then(Result::ok).unwrap_or_default();
        phone_options(&listed, &current.get())
    });
    let picker = move || {
        let change = change.clone();
        titled(
            "Paired phone giving the position",
            AnyView::new(row_field(
                "Phone",
                pick(
                    options.get(),
                    Signal::derive(move || Some(current.get())),
                    move |phone| change(PositionSource::Phone { phone }),
                ),
            )),
        )
    };
    let failed = move || match found.get() {
        Some(Err(error)) => Some(fault(String::from("Phone list failed"), Some(error))),
        _ => None,
    };
    view! {
        {picker}
        {failed}
    }
}

fn fix_readout(fix: &PositionFix, phone: bool, now: RwSignal<i64>) -> impl IntoView {
    let mut rows = vec![
        (
            "Position".to_owned(),
            AnyView::new(format!("{:.6}, {:.6}", fix.latitude, fix.longitude)),
        ),
        (
            "Grid".to_owned(),
            AnyView::new(grid_locator(fix.latitude, fix.longitude)),
        ),
    ];
    if let Some(accuracy) = fix.accuracy_m {
        rows.push((
            "Accuracy".to_owned(),
            AnyView::new(format!("±{accuracy:.0} m")),
        ));
    }
    if let Some(speed) = fix.speed_mps {
        rows.push((
            "Speed".to_owned(),
            AnyView::new(format!("{:.1} km/h", speed * 3.6)),
        ));
    }
    if phone || fix.attitude.heading_deg.is_some() {
        rows.push((
            "Heading".to_owned(),
            titled("True north", AnyView::new(heading_label(fix))),
        ));
    }
    if let Some(tilt) = tilt_label(fix) {
        rows.push((
            "Tilt".to_owned(),
            titled("Pitch and roll", AnyView::new(tilt)),
        ));
    }
    if phone {
        let time = fix.time.clone();
        rows.push((
            "Fix".to_owned(),
            AnyView::new(move || fix_age_label(&time, now.get())),
        ));
    }
    readout(rows)
}

fn gpsd_settings(
    source: Memo<Option<PositionSource>>,
    change: impl Fn(PositionSource) + Clone + 'static,
) -> impl IntoView {
    let address = Signal::derive(move || match source.get() {
        Some(PositionSource::Gpsd { address }) => address,
        _ => String::new(),
    });
    titled(
        "Host and port of the gpsd daemon",
        AnyView::new(row_field(
            "GPSD address",
            text_field(
                "GPSD address",
                address,
                "",
                Signal::stored(false),
                |text| valid_gpsd_address(text.trim()),
                move |text| {
                    change(PositionSource::Gpsd {
                        address: text.trim().to_owned(),
                    });
                },
            ),
        )),
    )
}

fn fixed_settings(
    source: Memo<Option<PositionSource>>,
    change: impl Fn(PositionSource) + Clone + 'static,
) -> impl IntoView {
    let place = Signal::derive(move || match source.get() {
        Some(PositionSource::Fixed {
            lat,
            lon,
            altitude_m,
        }) => (lat, lon, altitude_m),
        _ => (0.0, 0.0, None),
    });
    let lat = Signal::derive(move || place.get().0);
    let lon = Signal::derive(move || place.get().1);
    let latitude = NumberSpec::unit("°").within(-90.0, 90.0).step(0.00001);
    let longitude = NumberSpec::unit("°").within(-180.0, 180.0).step(0.00001);
    let on_lon = change.clone();
    view! {
        column(class = "kit-radio") {
            {titled("Latitude", AnyView::new(row_field("Latitude", number_field("Latitude", lat, latitude, Signal::stored(false), move |value| {
                let (_, lon, altitude_m) = place.get_untracked();
                change(PositionSource::Fixed { lat: value, lon, altitude_m });
            }))))}
            {titled("Longitude", AnyView::new(row_field("Longitude", number_field("Longitude", lon, longitude, Signal::stored(false), move |value| {
                let (lat, _, altitude_m) = place.get_untracked();
                on_lon(PositionSource::Fixed { lat, lon: value, altitude_m });
            }))))}
            {row_field("Grid", view! { text(class = "kit-mono") {{move || grid_locator(lat.get(), lon.get())}} })}
        }
    }
}

fn nmea_settings(
    store: Store,
    source: Memo<Option<PositionSource>>,
    change: impl Fn(PositionSource) + Clone + 'static,
) -> impl IntoView {
    let found = receivers(store);
    let parts = Signal::derive(move || match source.get() {
        Some(PositionSource::Nmea {
            device,
            baud,
            update_interval_ms,
        }) => (device, baud, update_interval_ms),
        _ => (String::new(), 0, DEFAULT_NMEA_UPDATE_INTERVAL_MS),
    });
    let device = Signal::derive(move || parts.get().0);
    let baud = Signal::derive(move || parts.get().1.to_string());
    let interval = Signal::derive(move || Some(parts.get().2));
    let with = move |edit: &dyn Fn(&mut (String, u32, u32))| {
        let mut next = parts.get_untracked();
        edit(&mut next);
        PositionSource::Nmea {
            device: next.0,
            baud: next.1,
            update_interval_ms: next.2,
        }
    };
    let (on_device, on_pick, on_baud, on_rate, on_suggest) = (
        change.clone(),
        change.clone(),
        change.clone(),
        change.clone(),
        change,
    );
    let detected = move || {
        let devices = found.get().and_then(Result::ok).unwrap_or_default();
        if devices.is_empty() {
            let said = if matches!(found.get(), Some(Err(_))) {
                "Serial device discovery failed"
            } else {
                "No serial receiver detected: plug one in, or type its path."
            };
            return AnyView::new(view! { text(class = "kit-note") {{said}} });
        }
        let options: Vec<(String, String)> = devices
            .iter()
            .map(|device| {
                let (path, detail) = nmea_suggestion(device);
                let label =
                    detail.map_or_else(|| path.clone(), |detail| format!("{path} · {detail}"));
                (path, label)
            })
            .collect();
        let chosen = Signal::derive(move || Some(device.get()));
        let on_pick = on_pick.clone();
        AnyView::new(row_field(
            "Detected",
            pick(options, chosen, move |path| {
                on_pick(with(&|next| next.0.clone_from(&path)))
            }),
        ))
    };
    let rates: Vec<(u32, String)> = RATES
        .iter()
        .map(|(ms, label)| (*ms, (*label).to_owned()))
        .collect();
    view! {
        column(class = "kit-radio") {
            {titled("Serial port of the receiver", AnyView::new(row_field("Serial device", text_field("Serial device", device, "", Signal::stored(false), |text| !text.trim().is_empty(), move |text| {
                on_device(with(&|next| text.trim().clone_into(&mut next.0)));
            }))))}
            {detected}
            {titled("Serial baud rate", AnyView::new(row_field("Baud", view! {
                row(class = "kit-dial-row") {
                    {text_field("Baud", baud, "", Signal::stored(false), |text| valid_baud(&text).is_some(), move |text| {
                        if let Some(value) = valid_baud(&text) {
                            on_baud(with(&|next| next.1 = value));
                        }
                    })}
                    {baud_suggestions(move |value| on_suggest(with(&|next| next.1 = value)))}
                }
            })))}
            {titled("Update rate", AnyView::new(row_field("Update rate", pick(rates, interval, move |ms| on_rate(with(&|next| next.2 = ms))))))}
        }
    }
}

fn baud_suggestions(on_pick: impl Fn(u32) + Clone + 'static) -> impl IntoView {
    let (open, toggle, close) = popover_slot();
    let menu = move || {
        open.get().then(|| {
            let rows: Vec<AnyView> = BAUDS
                .into_iter()
                .map(|baud| {
                    let on_pick = on_pick.clone();
                    AnyView::new(view! {
                        control(
                            class = "menu__row",
                            tabindex = Focus::Sequential,
                            a11y:role = Role::Button,
                            on:pointer_down:stop = move |_| {
                                close();
                                on_pick(baud);
                            }
                        ) {
                            {baud.to_string()}
                        }
                    })
                })
                .collect();
            view! { column(class = "kit-pop kit-pop--chip") {{rows}} }
        })
    };
    view! {
        box(class = "kit-pop-wrap") {
            {titled("Common rates", AnyView::new(button(|| "\u{25be}".to_owned(), Tone::Quiet, Signal::stored(false), toggle)))}
            {menu}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_paired_phone_stays_listed_even_when_the_server_forgot_it() {
        let listed: Vec<sdrmm_wire::phone::Phone> = serde_json::from_value(serde_json::json!([
            { "id": "a", "name": "Pixel", "platform": "android", "created_at": "2026-01-01T00:00:00Z", "online": true }
        ]))
        .expect("phones");
        let options = phone_options(&listed, "gone");
        assert_eq!(options.len(), 2);
        assert_eq!(options.last().map(|(id, _)| id.as_str()), Some("gone"));
        assert_eq!(phone_options(&listed, "a").len(), 1);
    }

    #[test]
    fn the_header_names_how_the_phone_stands() {
        assert_eq!(standing_head(None), NodeHead::default());
        assert_eq!(
            standing_head(Some(PhoneStanding::Online)),
            NodeHead::said("Online")
        );
        assert_eq!(
            standing_head(Some(PhoneStanding::NotPaired)).tone,
            head::Tone::Danger
        );
    }
}
