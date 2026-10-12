use std::{sync::Arc, time::Duration};

use sdrmm_wire::{
    phone::{
        CreateOfferRequest, MAX_PHONE_NAME_LEN, PairingOffer, Phone, PhoneAccess,
        PhoneAccessStatus, PhonesResponse, RenamePhoneRequest,
    },
    ws::StateScope,
};
use zgui::prelude::*;

use crate::{
    shell::phones::{
        OFFER_POLL, OfferView, PairWatch, StatusLine, Tone, countdown_label, discovery_line,
        format_pairing_code, listener_line, offer_is_live, offer_view, platform_label, seen_label,
        with_offer,
    },
    store::Store,
    ui::{
        kit_audio::ticker,
        kit_shell::{Entry, entry, hint, hover},
        kit_sources::{self, NumberSpec, number_field},
        library::{
            on_scope,
            qr::qr_code,
            rows::{Pending, act},
        },
        widgets::check,
    },
};

const PATH: &str = "/api/phones";
const TICK: Duration = Duration::from_secs(1);
const MAX_PORT: f64 = 65_535.0;

const SHEET: &str = css!(
    r#"
.ph-access { flex-direction: column; gap: 6px; }
.ph-line { align-items: center; gap: 8px; }
.ph-status { justify-content: space-between; gap: 8px; font-family: var(--mono); font-size: 10.5px; }
.ph-tone { color: var(--ink-dim); }
.ph-tone.ok { color: var(--ok); }
.ph-tone.bad { color: var(--danger); }
.ph-port { width: 88px; flex: 0 0 auto; margin-left: auto; }
.ph-offer { flex-direction: column; align-items: center; gap: 6px; padding: 8px; border: 1px solid var(--line); border-radius: 3px; }
.ph-offer.gone { flex-direction: row; }
.ph-code { font-family: var(--mono); font-size: 18px; color: var(--ink); }
.ph-key { font-family: var(--mono); font-size: 12px; color: var(--ink); }
.ph-pair { align-items: baseline; gap: 8px; }
.ph-foot { align-self: stretch; align-items: center; justify-content: space-between; gap: 8px; }
.ph-dot { width: 8px; height: 8px; border-radius: 4px; background-color: var(--ink-faint); flex: 0 0 auto; }
.ph-dot.on { background-color: var(--ok); }
.ph-name { font-family: var(--mono); font-size: 12px; color: var(--ink); }
.ph-name:hover { color: var(--accent); }
"#
);

#[derive(Clone, Copy)]
struct Phones {
    store: Store,
    data: RwSignal<Option<Arc<PhonesResponse>>>,
    failed: RwSignal<bool>,
    now: RwSignal<i64>,
    editing: RwSignal<Option<String>>,
    confirming: RwSignal<Option<String>>,
    busy: Pending,
    renaming: Pending,
}

impl Phones {
    fn refresh(self) {
        zgui::task::spawn_local(async move {
            match self.store.api().get::<PhonesResponse>(PATH).await {
                Ok(found) => {
                    self.data.try_set(Some(Arc::new(found)));
                    self.failed.try_set(false);
                }
                Err(error) => {
                    tracing::debug!(%error, "phone list failed");
                    self.failed.try_set(true);
                }
            }
        });
    }

    fn edit(self, change: impl FnOnce(&PhonesResponse) -> PhonesResponse) {
        if let Some(current) = self.data.get_untracked() {
            self.data.set(Some(Arc::new(change(&current))));
        }
    }

    fn set_access(self, access: PhoneAccess) {
        self.busy.run(async move {
            let path = format!("{PATH}/access");
            match self
                .store
                .api()
                .put::<_, PhoneAccessStatus>(&path, &access)
                .await
            {
                Ok(status) => self.edit(|data| PhonesResponse {
                    access: status,
                    ..data.clone()
                }),
                Err(error) => self.store.fail("Cannot change phone access", &error),
            }
            self.refresh();
        });
    }

    fn pair(self) {
        self.busy.run(async move {
            let path = format!("{PATH}/offers");
            let body = CreateOfferRequest::default();
            match self.store.api().post::<_, PairingOffer>(&path, &body).await {
                Ok(offer) => self.edit(|data| with_offer(data, &offer)),
                Err(error) => self.store.fail("Cannot pair a phone", &error),
            }
            self.refresh();
        });
    }

    fn cancel(self) {
        self.busy.run(async move {
            if let Err(error) = self.store.api().delete(&format!("{PATH}/offers")).await {
                self.store.fail("Cannot cancel pairing", &error);
            }
            self.refresh();
        });
    }

    fn rename(self, id: String, name: String) {
        self.editing.set(None);
        self.renaming.run(async move {
            let body = RenamePhoneRequest { name };
            let path = format!("{PATH}/{id}");
            if let Err(error) = self.store.api().patch::<_, Phone>(&path, &body).await {
                self.store.fail("Cannot rename the phone", &error);
            }
            self.refresh();
        });
    }

    fn revoke(self, id: String) {
        self.busy.run(async move {
            match self.store.api().delete(&format!("{PATH}/{id}")).await {
                Ok(()) => self.confirming.set(None),
                Err(error) => self.store.fail("Cannot revoke the phone", &error),
            }
            self.refresh();
        });
    }
}

pub fn panel(store: Store) -> impl IntoView {
    install_stylesheet("library-phones", SHEET);
    kit_sources::install();
    let phones = Phones {
        store,
        data: RwSignal::new(None),
        failed: RwSignal::new(false),
        now: ticker(TICK),
        editing: RwSignal::new(None),
        confirming: RwSignal::new(None),
        busy: Pending::new(),
        renaming: Pending::new(),
    };
    phones.refresh();
    on_scope(store, StateScope::Phones, move || phones.refresh());
    let polling = set_interval(OFFER_POLL, move || {
        let live = phones.data.with_untracked(|data| {
            data.as_ref()
                .is_some_and(|data| offer_is_live(data.offer.as_ref()))
        });
        if live {
            phones.refresh();
        }
    });
    on_cleanup_local(move || drop(polling));
    watch_pairing(phones);
    let loaded = Memo::new(move |_| phones.data.with(Option::is_some));
    let failed = move || {
        phones
            .failed
            .get()
            .then(|| AnyView::new(view! { text(class = "sk-text bad", a11y:role = Role::Alert) {"Phone list failed"} }))
    };
    let body = move || {
        if loaded.get() {
            AnyView::new(loaded_view(phones))
        } else if phones.failed.get() {
            AnyView::new(())
        } else {
            hint("Loading")
        }
    };
    view! {
        column(class = "sk-panel") {
            {failed}
            {body}
        }
    }
}

fn watch_pairing(phones: Phones) {
    let mut watch = PairWatch::default();
    let watching = zgui::reactive::RenderEffect::new(move |_| {
        let Some(data) = phones.data.get() else {
            return;
        };
        if let Some(phone) = watch.observe(&data) {
            phones.store.note(format!("Paired {}", phone.name));
        }
    });
    on_cleanup_local(move || drop(watching));
}

fn loaded_view(phones: Phones) -> impl IntoView {
    let listed = Memo::new(move |_| {
        phones
            .data
            .with(|data| data.as_ref().map(|data| data.phones.clone()))
            .unwrap_or_default()
    });
    let rows = move || {
        let found = listed.get();
        if found.is_empty() {
            return hint("No phones");
        }
        let rows: Vec<AnyView> = found
            .into_iter()
            .map(|phone| AnyView::new(phone_row(phones, phone)))
            .collect();
        AnyView::new(view! { column(class = "sk-list") {{rows}} })
    };
    view! {
        {access_settings(phones)}
        {offer_section(phones)}
        {rows}
    }
}

fn status(line: StatusLine) -> AnyView {
    let ok = line.tone == Tone::Ok;
    let bad = line.tone == Tone::Danger;
    match line.title {
        Some(title) => hover(
            title,
            view! { text(class = "ph-tone", class:ok = ok, class:bad = bad) {{line.text}} },
        ),
        None => AnyView::new(view! {
            text(class = "ph-tone", class:ok = ok, class:bad = bad) {{line.text}}
        }),
    }
}

fn access_settings(phones: Phones) -> impl IntoView {
    let access = Memo::new(move |_| {
        phones
            .data
            .with(|data| data.as_ref().map(|data| data.access.clone()))
    });
    let wanted = move || {
        access
            .get_untracked()
            .map(|status| status.access)
            .unwrap_or_default()
    };
    let enabled = Signal::derive(move || access.get().is_some_and(|status| status.access.enabled));
    let port = Signal::derive(move || {
        access
            .get()
            .map_or(0.0, |status| f64::from(status.access.port))
    });
    let busy = Signal::derive(move || phones.busy.busy());
    let spec = NumberSpec::unit("").within(1.0, MAX_PORT).step(1.0);
    let lines = move || {
        access.get().map(|found| {
            AnyView::new(view! {
                row(class = "ph-status") {
                    {status(listener_line(&found))}
                    {status(discovery_line(&found))}
                }
            })
        })
    };
    view! {
        column(class = "ph-access") {
            row(class = "ph-line") {
                {check(enabled, move |on| phones.set_access(PhoneAccess { enabled: on, ..wanted() }))}
                {hover("Listen for phones over HTTPS", view! { text(class = "sk-text") {"Allow phones"} })}
                {hover("Phone port", view! { text(class = "legend ph-port") {"Port"} })}
                box(class = "ph-port") {
                    {number_field("Port", port, spec, busy, move |next| {
                        let port = u16::try_from(next.round() as i64).unwrap_or(sdrmm_wire::phone::DEFAULT_PHONE_PORT);
                        phones.set_access(PhoneAccess { port, ..wanted() });
                    })}
                }
            }
            {lines}
        }
    }
}

fn offer_section(phones: Phones) -> impl IntoView {
    let shown = Memo::new(move |_| {
        let now = phones.now.get();
        phones.data.with(|data| {
            data.as_ref()
                .and_then(|data| offer_view(data.offer.as_ref(), now))
                .map(|view| match view {
                    OfferView::Live { code, uri, .. } => OfferView::Live {
                        code,
                        uri,
                        remaining_ms: 0,
                    },
                    other => other,
                })
        })
    });
    let reachable = Memo::new(move |_| {
        phones.data.with(|data| {
            data.as_ref()
                .is_some_and(|data| data.access.endpoint.is_some())
        })
    });
    let ready = Signal::derive_local(move || !phones.busy.busy() && reachable.get());
    move || match shown.get() {
        None => {
            let title = (!reachable.get()).then(|| String::from("Allow phones first"));
            act("Pair phone", "primary", ready, title, move || phones.pair())
        }
        Some(OfferView::Live { code, uri, .. }) => AnyView::new(live_offer(phones, &code, uri)),
        Some(gone) => AnyView::new(gone_offer(phones, &gone)),
    }
}

fn gone_offer(phones: Phones, view: &OfferView) -> impl IntoView {
    let said = if *view == OfferView::Burned {
        "Too many tries"
    } else {
        "Expired"
    };
    view! {
        row(class = "ph-offer gone") {
            text(class = "sk-text bad", a11y:role = Role::Alert) {{said}}
            spacer() {}
            {act("New code", "", phones.busy.idle(), None, move || phones.pair())}
        }
    }
}

fn live_offer(phones: Phones, code: &str, uri: Option<String>) -> impl IntoView {
    let key = phones.data.with_untracked(|data| {
        data.as_ref()
            .and_then(|data| data.access.endpoint.as_ref())
            .map(|endpoint| endpoint.key_check.clone())
    });
    let remaining = move || {
        let now = phones.now.get();
        phones.data.with(|data| {
            match data
                .as_ref()
                .and_then(|data| offer_view(data.offer.as_ref(), now))
            {
                Some(OfferView::Live { remaining_ms, .. }) => countdown_label(remaining_ms),
                _ => countdown_label(0),
            }
        })
    };
    let key = key.map(|key| {
        AnyView::new(view! {
            row(class = "ph-pair") {
                text(class = "legend") {"Key"}
                {hover("Check this on the phone", view! { text(class = "ph-key") {{key}} })}
            }
        })
    });
    let code = format_pairing_code(code);
    view! {
        column(class = "ph-offer", a11y:label = "Pairing") {
            {uri.map(|uri| qr_code(&uri, "Pairing QR code"))}
            row(class = "ph-pair") {
                text(class = "legend") {"Code"}
                {hover("Type this on the phone if the camera fails", view! { text(class = "ph-code") {{code}} })}
            }
            {key}
            row(class = "ph-foot") {
                {hover("Code expires", view! { text(class = "sk-mono") {{remaining}} })}
                {act("Cancel", "sm", phones.busy.idle(), None, move || phones.cancel())}
            }
        }
    }
}

fn phone_name(phones: Phones, phone: &Phone) -> impl IntoView {
    let id = phone.id.clone();
    let name = phone.name.clone();
    move || {
        if phones.editing.get().as_deref() == Some(id.as_str()) {
            return AnyView::new(name_entry(phones, id.clone(), name.clone()));
        }
        let open = id.clone();
        hover(
            "Rename",
            view! {
                control(
                    class = "ph-name",
                    tabindex = Focus::Sequential,
                    a11y:role = Role::Button,
                    on:click:stop = move |_| phones.editing.set(Some(open.clone()))
                ) {
                    {name.clone()}
                }
            },
        )
    }
}

fn name_entry(phones: Phones, id: String, name: String) -> impl IntoView {
    let typed = RwSignal::new_local(name.clone());
    let commit = move || {
        let next: String = typed
            .get_untracked()
            .trim()
            .chars()
            .take(MAX_PHONE_NAME_LEN)
            .collect();
        if next.is_empty() || next == name {
            phones.editing.set(None);
        } else {
            phones.rename(id.clone(), next);
        }
    };
    entry(
        Entry::new(typed, "Phone name").focused(),
        commit,
        move || {
            phones.editing.set(None);
        },
    )
}

fn revoke_actions(phones: Phones, phone: &Phone) -> impl IntoView {
    let id = phone.id.clone();
    let label = format!("Revoke {}", phone.name);
    move || {
        let asking = phones.confirming.get().as_deref() == Some(id.as_str());
        let revoked = id.clone();
        let asked = id.clone();
        if asking {
            AnyView::new(view! {
                {act("Revoke?", "sm", phones.busy.idle(), None, move || phones.revoke(revoked.clone()))}
                {act("Cancel", "sm", Signal::stored_local(true), None, move || phones.confirming.set(None))}
            })
        } else {
            let label = label.clone();
            hover(
                "Revoke",
                view! {
                    control(
                        class = "sk-btn sm",
                        tabindex = Focus::Sequential,
                        a11y:role = Role::Button,
                        a11y:label = label,
                        on:click:stop = move |_| phones.confirming.set(Some(asked.clone()))
                    ) {
                        "Revoke"
                    }
                },
            )
        }
    }
}

fn seen(phones: Phones, phone: Phone) -> impl IntoView {
    let platform = platform_label(phone.platform);
    let label = move || format!("{platform} \u{b7} {}", seen_label(&phone, phones.now.get()));
    view! { text(class = "sk-row__secondary") {{label}} }
}

fn phone_row(phones: Phones, phone: Phone) -> impl IntoView {
    let online = phone.online;
    let last_seen = phone.last_seen.clone();
    let secondary = match last_seen {
        Some(at) => hover(at, seen(phones, phone.clone())),
        None => AnyView::new(seen(phones, phone.clone())),
    };
    view! {
        column(class = "sk-row") {
            row(class = "sk-row__line") {
                box(class = "ph-dot", class:on = online) {}
                column(class = "sk-row__main") {
                    {phone_name(phones, &phone)}
                    {secondary}
                }
                row(class = "sk-row__actions") {{revoke_actions(phones, &phone)}}
            }
        }
    }
}
