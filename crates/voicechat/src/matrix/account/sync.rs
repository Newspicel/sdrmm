//! The account's own task: sign in, trust, then one sync fanned out to every call.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use matrix_sdk::config::SyncSettings;
use matrix_sdk::deserialized_responses::ProcessedToDeviceEvent;
use matrix_sdk::ruma::api::error::ErrorKind;
use matrix_sdk::ruma::{DeviceId, RoomId};
use matrix_sdk::sync::{JoinedRoomUpdate, State, SyncResponse};
use matrix_sdk::{Client, SessionChange};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::broadcast::{self, error::RecvError};
use tokio::sync::{mpsc, watch};

use super::connect::{self, Fault, NOT_SIGNED_IN};
use super::{Ready, Status};
use crate::link::Backoff;
use crate::matrix::MatrixAccount;
use crate::matrix::call::SyncEvent;

/// How long one sync request waits on the homeserver for news.
const SYNC_WAIT: Duration = Duration::from_secs(30);
/// Sync events a call may fall behind before it misses some.
const EVENTS: usize = 256;

/// What the account task follows and reports to.
pub(super) struct Line {
    pub(super) account: Arc<MatrixAccount>,
    pub(super) status: watch::Sender<Status>,
    pub(super) saved: watch::Sender<Option<String>>,
    pub(super) names: watch::Receiver<String>,
}

/// Keeps the account's client up until the session drops, retrying what a retry can fix.
pub(super) async fn run_account(mut line: Line, alive: mpsc::Sender<()>) {
    let mut backoff = Backoff::new();
    loop {
        let fault = tokio::select! {
            fault = connected(&mut line, &mut backoff) => fault,
            () = alive.closed() => return,
        };
        tracing::warn!(reason = fault.reason, "Matrix account down");
        line.status.send_replace(Status::Failed(fault.reason));
        if !fault.retry {
            return alive.closed().await;
        }
        tokio::select! {
            () = tokio::time::sleep(backoff.step()) => {}
            () = alive.closed() => return,
        }
        line.status.send_replace(Status::Starting);
    }
}

/// One client, from opening it to the fault that ends it.
async fn connected(line: &mut Line, backoff: &mut Backoff) -> Fault {
    let account = &*line.account;
    let session = line
        .saved
        .borrow()
        .clone()
        .or_else(|| account.session.clone());
    let client = match connect::open(account, session.as_deref(), &line.saved).await {
        Ok(client) => client,
        Err(fault) => return fault,
    };
    let changes = client.subscribe_to_session_changes();
    let (events, _) = broadcast::channel(EVENTS);
    match client.sync_once(settings()).await {
        Ok(first) => fan_out(&first, &events),
        Err(error) => return sync_fault(&error),
    }
    if let Err(fault) = connect::trust(&client, account).await {
        return fault;
    }
    line.status.send_replace(Status::Ready(Arc::new(Ready {
        client: client.clone(),
        http: reqwest::Client::new(),
        events: events.downgrade(),
    })));
    backoff.reset();
    tokio::select! {
        fault = sync(&client, &events) => fault,
        fault = keep_saved(&client, changes, &line.saved) => fault,
        never = keep_named(&client, &mut line.names) => match never {},
    }
}

fn settings() -> SyncSettings {
    SyncSettings::default().timeout(SYNC_WAIT)
}

/// Syncs until the homeserver forgets the session, sitting out other failures.
async fn sync(client: &Client, events: &broadcast::Sender<SyncEvent>) -> Fault {
    let mut backoff = Backoff::new();
    loop {
        match client.sync_once(settings()).await {
            Ok(response) => {
                backoff.reset();
                fan_out(&response, events);
            }
            Err(error) => {
                let fault = sync_fault(&error);
                if !fault.retry {
                    return fault;
                }
                tokio::time::sleep(backoff.step()).await;
            }
        }
    }
}

fn sync_fault(error: &matrix_sdk::Error) -> Fault {
    if connect::store_failed(error) {
        tracing::warn!(%error, "Matrix store failed");
        return Fault::lasting("Store unreadable");
    }
    if matches!(
        error.client_api_error_kind(),
        Some(ErrorKind::UnknownToken { .. })
    ) {
        Fault::lasting(NOT_SIGNED_IN)
    } else {
        tracing::warn!(%error, "Matrix sync failed");
        Fault::passing("Homeserver unreachable")
    }
}

/// Passes media keys and call member states on to every call.
fn fan_out(response: &SyncResponse, events: &broadcast::Sender<SyncEvent>) {
    let keys = response.to_device.iter().filter_map(key_of);
    let members = response
        .rooms
        .joined
        .iter()
        .flat_map(|(room, update)| members_of(room, update));
    for event in keys.chain(members) {
        if events.send(event).is_err() {
            tracing::trace!("no call listens");
        }
    }
}

#[derive(Deserialize)]
struct ToDeviceJson {
    #[serde(rename = "type")]
    kind: String,
    sender: String,
    #[serde(default)]
    content: Value,
}

/// A to-device event as a media key, naming the sender's device when Olm vouches for it.
pub(crate) fn key_of(event: &ProcessedToDeviceEvent) -> Option<SyncEvent> {
    let (raw, device) = match event {
        ProcessedToDeviceEvent::Decrypted {
            raw,
            encryption_info,
        } => (raw, encryption_info.sender_device.as_deref()),
        ProcessedToDeviceEvent::PlainText(raw) => (raw, None),
        _ => return None,
    };
    let event: ToDeviceJson = raw.deserialize_as_unchecked().ok()?;
    SyncEvent::key(
        &event.kind,
        &event.sender,
        device.map(DeviceId::as_str),
        &event.content,
    )
}

/// The call member states a room's update brings, in the order they took effect.
pub(crate) fn members_of(room: &RoomId, update: &JoinedRoomUpdate) -> Vec<SyncEvent> {
    let room = room.as_str();
    let (state, timeline) = match &update.state {
        State::Before(state) => (state, true),
        State::After(state) => (state, false),
    };
    let mut found: Vec<SyncEvent> = state
        .iter()
        .filter_map(|raw| SyncEvent::member(room, raw))
        .collect();
    if timeline {
        found.extend(
            update
                .timeline
                .events
                .iter()
                .filter_map(|event| SyncEvent::member(room, event.raw())),
        );
    }
    found
}

/// Saves every refreshed session; a session the homeserver dropped ends the account.
async fn keep_saved(
    client: &Client,
    mut changes: broadcast::Receiver<SessionChange>,
    saved: &watch::Sender<Option<String>>,
) -> Fault {
    loop {
        match changes.recv().await {
            Ok(SessionChange::TokensRefreshed) | Err(RecvError::Lagged(_)) => {
                connect::save(client, saved);
            }
            Ok(SessionChange::UnknownToken(_)) => return Fault::lasting(NOT_SIGNED_IN),
            Err(RecvError::Closed) => return std::future::pending().await,
        }
    }
}

/// Shows the account's nickname, following renames.
async fn keep_named(client: &Client, names: &mut watch::Receiver<String>) -> Infallible {
    loop {
        let name = names.borrow_and_update().clone();
        connect::show_name(client, &name).await;
        if names.changed().await.is_err() {
            break;
        }
    }
    std::future::pending().await
}
