//! The Matrix side of a call: membership state, delayed leave, media keys and names.

use std::time::Duration;

use async_trait::async_trait;
use matrix_sdk::deserialized_responses::RawAnySyncOrStrippedState;
use matrix_sdk::ruma::api::client::delayed_events::update_delayed_event::UpdateAction;
use matrix_sdk::ruma::api::error::ErrorKind;
use matrix_sdk::ruma::events::StateEventType;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{OwnedDeviceId, OwnedUserId};
use matrix_sdk::{Client, Room};
use reqwest::StatusCode;
use serde_json::json;

use crate::matrix::e2ee::keys::{EncryptionKeys, KEYS_EVENT};
use crate::matrix::membership::{
    CallState, Focus, LEGACY_TYPE, RTC_TYPE, StateEntry, foci_from_transports,
};

const DELAYED_EVENTS: &str = "org.matrix.msc4140";
/// Room versions with MSC3757 owned state keys, as js-sdk `MembershipManager` matches them.
const OWNED_KEY_VERSIONS: [&str; 2] = ["org.matrix.msc3757", "org.matrix.msc3779"];

/// Why a delayed leave did not restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Restart {
    /// The server no longer has it: it fired or was dropped.
    Gone,
    Failed(String),
}

/// Every Matrix side effect of a call.
#[async_trait]
pub(crate) trait Homeserver: Sync {
    fn room_id(&self) -> String;

    /// The `LiveKit` foci the homeserver advertises, the one to publish on first.
    async fn foci(&self) -> Result<Vec<Focus>, String>;

    async fn encrypted(&self) -> Result<bool, String>;

    fn owned_state_keys(&self) -> bool;

    /// Whether `/versions` lists MSC4140 delayed events.
    async fn delayed_events(&self) -> bool;

    async fn call_states(&self) -> Result<Vec<CallState>, String>;

    async fn put_state(&self, entry: &StateEntry) -> Result<(), String>;

    /// Schedules `entry` after `delay`, answering its delay id.
    async fn put_delayed(&self, entry: &StateEntry, delay: Duration) -> Result<String, String>;

    async fn restart_delayed(&self, id: &str) -> Result<(), Restart>;

    async fn cancel_delayed(&self, id: &str) -> Result<(), String>;

    /// Sends our key Olm-encrypted to each `(user, device)`, answering the ones it did not
    /// reach: devices whose keys are not known yet, or that the send failed for.
    async fn send_keys(
        &self,
        to: &[(String, String)],
        content: &EncryptionKeys,
    ) -> Result<Vec<(String, String)>, String>;

    async fn display_name(&self, user: &str) -> Option<String>;
}

/// The homeserver through `matrix-sdk`.
pub(crate) struct Matrix {
    pub(crate) client: Client,
    pub(crate) room: Room,
}

fn no_call_server() -> String {
    "No call server".to_owned()
}

fn refused<E: std::fmt::Display>(what: &'static str) -> impl FnOnce(E) -> String {
    move |error| {
        tracing::warn!(%error, what, "homeserver refused");
        format!("{what} refused")
    }
}

/// A 404 or `M_NOT_FOUND`, as js-sdk `isDelayedEventGoneError` reads it.
fn gone(error: &matrix_sdk::Error) -> bool {
    matches!(error.client_api_error_kind(), Some(ErrorKind::NotFound))
        || error
            .as_client_api_error()
            .is_some_and(|error| error.status_code == StatusCode::NOT_FOUND)
}

fn call_state(raw: &RawAnySyncOrStrippedState) -> Option<CallState> {
    let RawAnySyncOrStrippedState::Sync(raw) = raw else {
        return None;
    };
    super::call_state(raw)
}

impl Matrix {
    async fn transports(&self) -> Vec<Focus> {
        let found = match self.client.discover_rtc_transports().await {
            Ok(found) => found.unwrap_or_default(),
            Err(error) => {
                tracing::debug!(%error, "no RTC transports");
                Vec::new()
            }
        };
        foci_from_transports(&json!({ "transports": found }))
    }

    async fn well_known(&self) -> Vec<Focus> {
        let found = self
            .client
            .well_known_rtc_transports()
            .await
            .unwrap_or_default();
        foci_from_transports(&json!({ "transports": found }))
    }

    async fn states_of(&self, kind: &str) -> Result<Vec<CallState>, String> {
        let raw = self
            .room
            .get_state_events(StateEventType::from(kind))
            .await
            .map_err(refused("Room state"))?;
        Ok(raw.iter().filter_map(call_state).collect())
    }
}

#[async_trait]
impl Homeserver for Matrix {
    fn room_id(&self) -> String {
        self.room.room_id().to_string()
    }

    async fn foci(&self) -> Result<Vec<Focus>, String> {
        let mut foci = self.transports().await;
        if foci.is_empty() {
            foci = self.well_known().await;
        }
        if foci.is_empty() {
            return Err(no_call_server());
        }
        Ok(foci)
    }

    async fn encrypted(&self) -> Result<bool, String> {
        self.room
            .latest_encryption_state()
            .await
            .map(|state| state.is_encrypted())
            .map_err(refused("Room state"))
    }

    fn owned_state_keys(&self) -> bool {
        self.room.version().is_some_and(|version| {
            OWNED_KEY_VERSIONS
                .iter()
                .any(|prefix| version.as_str().starts_with(prefix))
        })
    }

    async fn delayed_events(&self) -> bool {
        self.client.unstable_features().await.is_ok_and(|features| {
            features
                .iter()
                .any(|feature| feature.as_str() == DELAYED_EVENTS)
        })
    }

    async fn call_states(&self) -> Result<Vec<CallState>, String> {
        let mut states = self.states_of(LEGACY_TYPE).await?;
        states.extend(self.states_of(RTC_TYPE).await?);
        Ok(states)
    }

    async fn put_state(&self, (kind, key, content): &StateEntry) -> Result<(), String> {
        self.room
            .send_state_event_raw(kind, key, content)
            .await
            .map(drop)
            .map_err(refused("Membership"))
    }

    async fn put_delayed(
        &self,
        (kind, key, content): &StateEntry,
        delay: Duration,
    ) -> Result<String, String> {
        self.room
            .send_state_event_raw(kind, key, content)
            .into_delayed(delay)
            .await
            .map(|sent| sent.delay_id)
            .map_err(refused("Delayed leave"))
    }

    async fn restart_delayed(&self, id: &str) -> Result<(), Restart> {
        self.client
            .update_delayed_event(id.to_owned(), UpdateAction::Restart, None)
            .await
            .map(drop)
            .map_err(|error| {
                if gone(&error) {
                    Restart::Gone
                } else {
                    Restart::Failed(refused("Delayed leave")(error))
                }
            })
    }

    async fn cancel_delayed(&self, id: &str) -> Result<(), String> {
        self.client
            .update_delayed_event(id.to_owned(), UpdateAction::Cancel, None)
            .await
            .map(drop)
            .map_err(refused("Delayed leave"))
    }

    async fn send_keys(
        &self,
        to: &[(String, String)],
        content: &EncryptionKeys,
    ) -> Result<Vec<(String, String)>, String> {
        let encryption = self.client.encryption();
        let mut devices = Vec::with_capacity(to.len());
        let mut missed = Vec::new();
        for (user, device) in to {
            let Ok(user) = OwnedUserId::try_from(user.as_str()) else {
                tracing::warn!(user, "call member with a bad user id skipped");
                continue;
            };
            let id = OwnedDeviceId::from(device.as_str());
            if let Ok(Some(found)) = encryption.get_device(&user, &id).await {
                devices.push(found);
            } else {
                tracing::debug!(%user, device, "no keys for call member yet");
                missed.push((user.to_string(), device.clone()));
            }
        }
        if devices.is_empty() {
            return Ok(missed);
        }
        let raw = Raw::new(content)
            .map_err(refused("Media key"))?
            .cast_unchecked();
        let failed = encryption
            .encrypt_and_send_raw_to_device(
                devices.iter().collect(),
                KEYS_EVENT,
                raw,
                Default::default(),
            )
            .await
            .map_err(refused("Media key"))?;
        missed.extend(
            failed
                .into_iter()
                .map(|(user, device)| (user.to_string(), device.to_string())),
        );
        Ok(missed)
    }

    async fn display_name(&self, user: &str) -> Option<String> {
        let user = OwnedUserId::try_from(user).ok()?;
        let member = self.room.get_member_no_sync(&user).await.ok()??;
        Some(member.name().to_owned())
    }
}
