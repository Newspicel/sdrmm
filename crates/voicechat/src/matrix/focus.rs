//! Trades a Matrix `OpenID` token for a `LiveKit` join grant.

use matrix_sdk::ruma::api::client::account::request_openid_token::v3::Response as OpenIdToken;
use reqwest::{Client, Response, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use super::membership::Focus;

const SLOT_ID: &str = "m.call#ROOM";

/// Where to connect and the token to connect with.
#[derive(Clone, Deserialize)]
pub(crate) struct LiveKitGrant {
    pub url: String,
    pub jwt: String,
}

impl std::fmt::Debug for LiveKitGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveKitGrant")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

fn openid_json(openid: &OpenIdToken) -> Value {
    json!({
        "access_token": openid.access_token,
        "token_type": openid.token_type.as_str(),
        "matrix_server_name": openid.matrix_server_name,
        "expires_in": openid.expires_in.as_secs(),
    })
}

fn endpoint(focus: &Focus, path: &str) -> String {
    format!("{}/{path}", focus.service_url.trim_end_matches('/'))
}

async fn post(http: &Client, url: &str, body: &Value) -> Result<Response, String> {
    http.post(url)
        .json(body)
        .send()
        .await
        .map_err(|_| "No call server".to_owned())
}

async fn grant_of(response: Response) -> Result<LiveKitGrant, String> {
    match response.status() {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err("Call server refused".to_owned()),
        status if status.is_success() => response
            .json()
            .await
            .map_err(|_| "Call server failed".to_owned()),
        _ => Err("Call server failed".to_owned()),
    }
}

/// Asks `lk-jwt-service` for a grant: `/get_token` first, then the older
/// `/sfu/get` when the service has no such endpoint. `member_id` is the
/// membership id we published.
pub(crate) async fn livekit_token(
    http: &Client,
    focus: &Focus,
    room: &str,
    user: &str,
    device: &str,
    member_id: &str,
    openid: &OpenIdToken,
) -> Result<LiveKitGrant, String> {
    let current = json!({
        "room_id": room,
        "slot_id": SLOT_ID,
        "openid_token": openid_json(openid),
        "member": {"id": member_id, "claimed_user_id": user, "claimed_device_id": device},
    });
    let response = post(http, &endpoint(focus, "get_token"), &current).await?;
    if response.status() != StatusCode::NOT_FOUND {
        return grant_of(response).await;
    }
    let legacy = json!({
        "room": room,
        "openid_token": openid_json(openid),
        "device_id": device,
    });
    grant_of(post(http, &endpoint(focus, "sfu/get"), &legacy).await?).await
}

#[cfg(test)]
mod tests;
