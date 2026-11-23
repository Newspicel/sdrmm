//! One `MatrixRTC` call joined to a link: membership, media keys and `LiveKit` audio.

#[cfg_attr(
    test,
    expect(dead_code, reason = "the real homeserver needs a live one")
)]
mod homeserver;
mod keep;
mod live;
mod media;
mod people;
mod run;
#[cfg_attr(test, expect(dead_code, reason = "the real SFU needs a live one"))]
mod sfu;
mod wire;

use serde::Deserialize;
use serde_json::Value;
use tokio::sync::broadcast;

use crate::link::Ends;
use crate::matrix::e2ee::keys::{EncryptionKeys, KEYS_EVENT, KEYS_EVENT_STABLE};
use crate::matrix::membership::CallState;

/// What a session hands a call.
#[cfg_attr(test, expect(dead_code, reason = "built by a live session"))]
pub(crate) struct CallContext {
    pub(crate) client: matrix_sdk::Client,
    pub(crate) room: matrix_sdk::Room,
    pub(crate) http: reqwest::Client,
    pub(crate) user: String,
    pub(crate) device: String,
    pub(crate) events: broadcast::Receiver<SyncEvent>,
}

/// What the session's sync loop passes on to its calls.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SyncEvent {
    /// A media key sent to our device. `device` is the sender's device when the event came
    /// Olm-encrypted from it, None when it came in the clear.
    Key {
        sender: String,
        device: Option<String>,
        content: EncryptionKeys,
    },
    /// A call member state event of `room`.
    Member { room: String, state: CallState },
}

impl SyncEvent {
    /// A to-device event as a media key, when it has either key event type.
    pub(crate) fn key(
        event_type: &str,
        sender: &str,
        device: Option<&str>,
        content: &Value,
    ) -> Option<Self> {
        if event_type != KEYS_EVENT && event_type != KEYS_EVENT_STABLE {
            return None;
        }
        let Ok(content) = EncryptionKeys::deserialize(content) else {
            tracing::warn!(sender, "malformed media key ignored");
            return None;
        };
        Some(Self::Key {
            sender: sender.to_owned(),
            device: device.map(str::to_owned),
            content,
        })
    }
}

/// How a call ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Ended {
    /// The link let go.
    Released,
    /// The session's sync stopped, so no retry can help.
    Failed(String),
}

/// Runs the call until its link is released, retrying with backoff.
#[cfg_attr(test, expect(dead_code, reason = "run by a live session"))]
pub(crate) async fn run_call(ctx: CallContext, ends: Ends) -> Ended {
    let CallContext {
        client,
        room,
        http,
        user,
        device,
        events,
    } = ctx;
    let me = people::Me { user, device };
    let dial = sfu::LiveKit {
        client: client.clone(),
        http,
        room: room.room_id().to_string(),
        me: me.clone(),
    };
    let server = homeserver::Matrix { client, room };
    run::run(&server, &dial, me, events, ends).await
}

#[cfg(test)]
mod tests;
