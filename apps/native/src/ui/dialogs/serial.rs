use sdrmm_wire::{device::DeviceInfo, rest::WrittenSerial};
use zgui::prelude::*;

use crate::{
    shell::serial_prompt::{failure, mark_asked, next_to_ask, request},
    store::Store,
    ui::{
        dialogs::{frame, title},
        faces::device::devices::device_id,
        kit_channel::tip,
        kit_shell::{Entry, button, entry, gated_button, scrim},
        shell::Shell,
        widgets::check,
    },
};

const SERIAL_HINT: &str = "1 to 16 letters or digits. Empty picks a random one.";

pub fn layer(store: Store, shell: Shell) -> impl IntoView {
    let stored = shell.recall();
    let asked = RwSignal::new(stored.serial_asked);
    let off = RwSignal::new(stored.serial_prompt_off);
    let device = Memo::new(move |_| {
        if off.get() {
            return None;
        }
        let devices = store.devices.get();
        asked.with(|asked| next_to_ask(&devices, asked).cloned())
    });
    let dismiss = move |id: String, never_again: bool| {
        if never_again {
            off.set(true);
        }
        asked.update(|held| *held = mark_asked(held, &id));
        shell.remember(move |prefs| {
            prefs.serial_prompt_off |= never_again;
            prefs.serial_asked = mark_asked(&prefs.serial_asked, &id);
        });
    };
    move || {
        device
            .get()
            .map(|device| AnyView::new(prompt(store, device, dismiss)))
    }
}

fn prompt(
    store: Store,
    device: DeviceInfo,
    dismiss: impl Fn(String, bool) + Copy + 'static,
) -> impl IntoView {
    let id = device_id(&device);
    let never_again = RwSignal::new(false);
    let written = RwSignal::new(None::<String>);
    let close = {
        let id = id.clone();
        move || dismiss(id.clone(), never_again.get_untracked())
    };
    let shown = {
        let close = close.clone();
        move || match written.get() {
            None => AnyView::new(ask(
                store,
                &device,
                Asking {
                    never_again,
                    written,
                },
                close.clone(),
            )),
            Some(serial) => AnyView::new(done(serial, close.clone())),
        }
    };
    scrim(close, shown)
}

#[derive(Clone, Copy)]
struct Asking {
    never_again: RwSignal<bool>,
    written: RwSignal<Option<String>>,
}

fn ask(
    store: Store,
    device: &DeviceInfo,
    asking: Asking,
    close: impl Fn() + 'static,
) -> impl IntoView {
    let Asking {
        never_again,
        written,
    } = asking;
    let typed = RwSignal::new_local(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let id = device_id(device);
    let write = move || {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        let body = request(id.clone(), &typed.get_untracked());
        zgui::task::spawn_local(async move {
            let reply = store
                .api()
                .post::<_, WrittenSerial>("/api/devices/serial", &body)
                .await;
            busy.set(false);
            match reply {
                Ok(done) => written.set(Some(done.serial)),
                Err(caught) => error.set(Some(failure(&caught))),
            }
        });
    };
    let idle = Signal::derive_local(move || !busy.get());
    let port = format!(
        "{} has no serial of its own, so it is told apart by its USB port.",
        device.label
    );
    frame(
        "narrow",
        title("Give this RTL-SDR a serial?"),
        (),
        view! {
            text(class = "sk-text") {{port}}
            text(class = "sk-text") {"With one, its settings and calibration stay with it on any port and next to other dongles."}
            row(class = "sk-toolbar") {
                {tip(SERIAL_HINT, AnyView::new(view! { text(class = "sk-text") {"Serial"} }))}
                {entry(Entry::new(typed, "Auto"), write.clone(), || {})}
            }
            {move || error.get().map(|message| view! { text(class = "sk-text bad", a11y:role = Role::Alert) {{message}} })}
            row(class = "sk-toolbar") {
                {check(never_again.into(), move |on| never_again.set(on))}
                text(class = "sk-text") {"Don't show again"}
            }
        },
        view! {
            spacer() {}
            {button("No", "quiet", close)}
            {gated_button("Yes", "primary", idle, write)}
        },
    )
}

fn done(serial: String, close: impl Fn() + 'static) -> impl IntoView {
    frame(
        "narrow",
        title(format!("Serial {serial} written")),
        (),
        view! { text(class = "sk-text") {"Replug the dongle to use it."} },
        view! {
            spacer() {}
            {button("OK", "primary", close)}
        },
    )
}
