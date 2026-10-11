use std::sync::Arc;

use sdrmm_wire::remote::{RemoteState, RemoteStatus};
use zgui::prelude::*;

use crate::{
    shell::remote::{
        REMOTE_POLL, RemoteAction, app_host, approval_url, remote_action, remote_line,
        remote_polled, shows_app_link,
    },
    store::Store,
    ui::{
        files,
        kit_shell::{hint, hover},
        library::{
            load,
            qr::qr_code,
            reload,
            rows::{Pending, act},
        },
    },
};

const PATH: &str = "/api/remote";

const SHEET: &str = css!(
    r#"
.rm { flex-direction: column; align-items: center; gap: 8px; padding: 14px; }
.rm__code { font-family: var(--mono); font-size: 22px; font-weight: 600; color: var(--accent); }
.rm__line { font-size: 12px; color: var(--ink-dim); }
.rm__line.ok { color: var(--ok); }
.rm__line.bad { color: var(--danger); }
.rm__link { font-size: 12px; color: var(--ink); }
.rm__link:hover { color: var(--accent); }
"#
);

type Loaded = RwSignal<Option<Result<Arc<RemoteStatus>, String>>>;

pub fn panel(store: Store) -> impl IntoView {
    install_stylesheet("library-remote", SHEET);
    let status: Loaded = load(store, PATH.to_owned());
    let polling = set_interval(REMOTE_POLL, move || {
        let current = status.get_untracked().and_then(Result::ok);
        if remote_polled(current.as_deref()) {
            reload(store, PATH.to_owned(), status);
        }
    });
    on_cleanup_local(move || drop(polling));
    let pending = Pending::new();
    move || match status.get() {
        None => AnyView::new(()),
        Some(Err(_)) => AnyView::new(view! {
            column(class = "sk-panel") {{hint("Remote status failed")}}
        }),
        Some(Ok(found)) => AnyView::new(remote_view(store, &found, status, pending)),
    }
}

fn link(store: Store, text: String, url: String) -> AnyView {
    let tip = url.clone();
    hover(
        tip,
        view! {
            control(
                class = "rm__link",
                tabindex = Focus::Sequential,
                a11y:role = Role::Link,
                on:click:stop = move |_| files::open_link(store, &url)
            ) {
                {text}
            }
        },
    )
}

fn pairing(store: Store, status: &RemoteStatus, host: &str) -> AnyView {
    let url = approval_url(status).to_owned();
    let code = status.user_code.clone().unwrap_or_default();
    AnyView::new(view! {
        text(class = "rm__code") {{code}}
        {qr_code(&url, "Pairing QR code")}
        {link(store, format!("Approve on {host}"), url.clone())}
    })
}

fn line(status: &RemoteStatus) -> Option<AnyView> {
    let text = remote_line(status);
    let online = status.state == RemoteState::Online;
    (!text.is_empty())
        .then(|| AnyView::new(view! { text(class = "rm__line", class:ok = online) {{text}} }))
}

fn failure(status: &RemoteStatus) -> Option<AnyView> {
    let error = status
        .error
        .clone()
        .filter(|_| status.state == RemoteState::Unpaired)?;
    Some(AnyView::new(view! {
        text(class = "rm__line bad", a11y:role = Role::Alert) {{error}}
    }))
}

async fn pair(store: Store, status: Loaded) {
    match store
        .api()
        .post_empty::<RemoteStatus>("/api/remote/pair")
        .await
    {
        Ok(next) => status.set(Some(Ok(Arc::new(next)))),
        Err(error) => store.fail("Cannot pair", &error),
    }
    reload(store, PATH.to_owned(), status);
}

async fn unpair(store: Store, status: Loaded) {
    if let Err(error) = store.api().delete(PATH).await {
        store.fail("Cannot disconnect", &error);
    }
    reload(store, PATH.to_owned(), status);
}

fn action_button(
    store: Store,
    status: &RemoteStatus,
    loaded: Loaded,
    pending: Pending,
) -> Option<AnyView> {
    let host = app_host(&status.app_origin);
    let (label, pairs) = match remote_action(status)? {
        RemoteAction::Pair => (format!("Connect to {host}"), true),
        RemoteAction::PairAgain => (String::from("Pair again"), true),
        RemoteAction::Cancel => (String::from("Cancel"), false),
        RemoteAction::Disconnect => (String::from("Disconnect"), false),
    };
    let tone = if pairs { "primary" } else { "" };
    Some(act(label, tone, pending.idle(), None, move || {
        if pairs {
            pending.run(pair(store, loaded));
        } else {
            pending.run(unpair(store, loaded));
        }
    }))
}

fn remote_view(
    store: Store,
    status: &RemoteStatus,
    loaded: Loaded,
    pending: Pending,
) -> impl IntoView {
    let host = app_host(&status.app_origin);
    let code = (status.state == RemoteState::Pairing).then(|| pairing(store, status, &host));
    let app = shows_app_link(status)
        .then(|| link(store, format!("Open {host}"), status.app_origin.clone()));
    view! {
        column(class = "rm") {
            {code}
            {line(status)}
            {failure(status)}
            {app}
            {action_button(store, status, loaded, pending)}
        }
    }
}
