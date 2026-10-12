use std::rc::Rc;

use sdrmm_wire::{
    device::DeviceInfo,
    doctor::{CheckStatus, DoctorReport},
};
use zgui::prelude::*;

use super::{
    actions::Busy,
    devices::{
        NETWORK_BACKENDS, SourceTab, address_title, claimed_devices, device_hint, group_devices,
        network_device_id, show_synthetic, source_tabs, unclaimed_devices, visible_devices,
    },
    saved::{Shelf, save, saved_radios, shelf},
};
use crate::{
    store::Store,
    ui::{
        kit_sources::{
            Tone, button, collapsible_titled, draft_field, icons, tip, titled_icon_button,
            titled_segmented,
        },
        widgets::pick,
    },
};

pub const DOCTOR_TIP: &str =
    "Run the checks behind sdrmm --doctor: drivers, permissions, and what the USB bus reports";

pub struct Choices {
    pub on_choose: Box<dyn Fn(DeviceInfo)>,
    pub on_network: Box<dyn Fn(String)>,
}

pub fn device_choices(store: Store, node: String, busy: Busy, choices: Choices) -> impl IntoView {
    let tab = RwSignal::new(SourceTab::Radios);
    let found = Signal::derive(move || {
        let visible = visible_devices(&store.devices.get(), show_synthetic());
        let claimed = claimed_devices(&store.graph.get(), &node);
        let free = unclaimed_devices(&visible, &claimed);
        (visible.len() - free.len(), group_devices(&free))
    });
    let tabs = Memo::new(move |_| source_tabs(found.with(|(_, (_, synthetic))| synthetic.len())));
    let shown = Signal::derive(move || {
        let wanted = tab.get();
        if tabs.with(|tabs| tabs.iter().any(|(value, _, _)| *value == wanted)) {
            wanted
        } else {
            SourceTab::Radios
        }
    });
    let choose = Rc::new(choices.on_choose);
    let network: Rc<dyn Fn(String)> = Rc::new(choices.on_network);
    let saved = shelf(store);
    let body = move || match shown.get() {
        SourceTab::Radios => {
            let choose = choose.clone();
            AnyView::new(view! {
                column(class = "kit-list-wrap") {
                    {radio_list(Signal::derive(move || found.get().1.0), busy, choose)}
                    {move || radios_said(store, found.get().0, found.with(|(_, (radios, _))| radios.is_empty()))}
                    {collapsible_titled(|| "Check hardware".to_owned(), "kit-fold__link", Some(DOCTOR_TIP), move || AnyView::new(doctor(store)))}
                }
            })
        }
        SourceTab::Network => AnyView::new(view! {
            column(class = "kit-choices") {
                {saved_radios(store, saved, busy, network.clone())}
                {network_form(store, saved, busy, network.clone())}
            }
        }),
        SourceTab::Virtual => AnyView::new(radio_list(
            Signal::derive(move || found.get().1.1),
            busy,
            choose.clone(),
        )),
    };
    view! {
        column(class = "kit-choices") {
            {move || titled_segmented(tabs.get(), shown, move |next| tab.set(next))}
            {move || busy.error.get().map(|error| AnyView::new(view! { text(class = "kit-alert") {{error}} }))}
            {body}
        }
    }
}

#[must_use]
pub fn empty_said(elsewhere: usize) -> (&'static str, &'static str) {
    if elsewhere > 0 {
        (
            "Every radio found is already open on another node.",
            "Plug another radio in, or move that node's wires here",
        )
    } else {
        (
            "No radios found.",
            "Plug a radio in, or pick a recording or a network radio above",
        )
    }
}

fn radios_said(store: Store, elsewhere: usize, empty: bool) -> Option<AnyView> {
    if store.devices.with(|devices| devices.is_empty()) && !store.connected.get() {
        return Some(AnyView::new(
            view! { text(class = "kit-note") {"Looking for radios…"} },
        ));
    }
    empty.then(|| {
        let (said, hint) = empty_said(elsewhere);
        tip(
            move || hint.to_owned(),
            AnyView::new(view! { text(class = "kit-note") {{said}} }),
        )
    })
}

fn radio_list(
    devices: Signal<Vec<DeviceInfo>>,
    busy: Busy,
    choose: Rc<Box<dyn Fn(DeviceInfo)>>,
) -> impl IntoView {
    move || {
        devices
            .get()
            .into_iter()
            .map(|info| {
                let choose = choose.clone();
                let label = info.label.clone();
                let hint = device_hint(&info);
                let pressed = AnyView::new(button(
                    move || label.clone(),
                    Tone::Plain,
                    busy.busy.into(),
                    move || choose(info.clone()),
                ));
                match hint {
                    Some(hint) => tip(move || hint.to_owned(), pressed),
                    None => pressed,
                }
            })
            .collect::<Vec<_>>()
    }
}

fn network_form(
    store: Store,
    saved: Shelf,
    busy: Busy,
    on_add: Rc<dyn Fn(String)>,
) -> impl IntoView {
    let driver = RwSignal::new(NETWORK_BACKENDS[0].driver);
    let address = RwSignal::new_local(String::new());
    let target = Signal::derive_local(move || network_device_id(driver.get(), &address.get()));
    let options: Vec<(&'static str, String)> = NETWORK_BACKENDS
        .iter()
        .map(|backend| (backend.driver, backend.label.to_owned()))
        .collect();
    let backend = move || {
        NETWORK_BACKENDS
            .iter()
            .find(|backend| backend.driver == driver.get())
            .unwrap_or(&NETWORK_BACKENDS[0])
    };
    let add = move || {
        if let Some(id) = target.get_untracked() {
            on_add(id);
        }
    };
    let submit = add.clone();
    let blocked = Signal::derive(move || busy.busy.get() || target.with(Option::is_none));
    let unsaveable = Signal::derive(move || target.with(Option::is_none));
    let keep = move || {
        if let Some(id) = target.get_untracked() {
            save(store, saved, id);
        }
    };
    view! {
        column(class = "kit-choices") {
            row(class = "kit-pop__row") {
                text(class = "kit-legend") {"Via"}
                {pick(options, Signal::derive(move || Some(driver.get())), move |next| driver.set(next))}
            }
            row(class = "kit-pop__row") {
                {move || {
                    let submit = submit.clone();
                    let backend = backend();
                    tip(move || address_title(backend), AnyView::new(draft_field("Radio address", address, backend.placeholder, Signal::derive_local(|| false), submit)))
                }}
                {titled_icon_button(icons::BOOKMARK_PLUS, "Save radio", || "Save for later".to_owned(), Signal::stored(false), unsaveable, keep)}
                {button(|| "Add".to_owned(), Tone::Plain, blocked, add)}
            }
        }
    }
}

fn doctor(store: Store) -> impl IntoView {
    let report = RwSignal::new(None::<Result<DoctorReport, String>>);
    zgui::task::spawn_local(async move {
        let fetched = store
            .api()
            .get::<DoctorReport>("/api/doctor")
            .await
            .map_err(|error| error.to_string());
        report.try_set(Some(fetched));
    });
    move || match report.get() {
        None => AnyView::new(view! { text(class = "kit-note") {"Checking…"} }),
        Some(Err(error)) => AnyView::new(
            view! { text(class = "kit-alert") {{format!("Diagnostics failed: {error}")}} },
        ),
        Some(Ok(report)) => AnyView::new(view! {
            column(class = "kit-doctor") {
                {report.checks.into_iter().map(|check| {
                    let status = match check.status {
                        CheckStatus::Ok => "ok",
                        CheckStatus::Warn => "warn",
                        CheckStatus::Fail => "fail",
                    };
                    let detail = match check.hint {
                        Some(hint) => format!("{}\n→ {hint}", check.detail),
                        None => check.detail,
                    };
                    view! {
                        column(class = "kit-check") {
                            row(class = "kit-check__head") {
                                text(class = "kit-check__status", attr:data-status = status) {{format!("[{status}]")}}
                                text(class = "kit-mono") {{check.name}}
                            }
                            text(class = "kit-check__detail") {{detail}}
                        }
                    }
                }).collect::<Vec<_>>()}
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_list_says_why_and_what_to_do() {
        assert_eq!(empty_said(0).0, "No radios found.");
        assert!(empty_said(2).1.contains("another radio"));
    }
}
