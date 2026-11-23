use std::time::Duration;

use matrix_sdk::authentication::matrix::MatrixSession;
use matrix_sdk::deserialized_responses::{
    AlgorithmInfo, EncryptionInfo, ProcessedToDeviceEvent, TimelineEvent, VerificationLevel,
    VerificationState,
};
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{owned_device_id, owned_room_id, owned_user_id};
use matrix_sdk::sync::{JoinedRoomUpdate, State, Timeline};
use matrix_sdk::test_utils::mocks::MatrixMockServer;
use matrix_sdk::{SessionMeta, SessionTokens};
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio::time::timeout;

use super::account::connect::{Saved, build_client, sign_in};
use super::account::sync::{key_of, members_of};
use super::call::SyncEvent;
use super::*;
use crate::{LinkState, Service, Session};

const PATIENCE: Duration = Duration::from_secs(10);

fn account(homeserver: &str, login: MatrixLogin) -> MatrixAccount {
    MatrixAccount {
        homeserver: homeserver.to_owned(),
        user: "@bot:x.org".into(),
        login,
        nickname: String::new(),
        recovery_key: None,
        store: memory(),
        session: None,
    }
}

fn memory() -> MatrixStore {
    MatrixStore {
        dir: None,
        passphrase: String::new(),
    }
}

async fn homeserver() -> MatrixMockServer {
    let server = MatrixMockServer::new().await;
    server.mock_versions().ok().mount().await;
    server
}

fn saved_json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[tokio::test]
async fn password_login_saves_a_session() {
    let server = homeserver().await;
    server.mock_login().ok().expect(1).mount().await;
    let account = account(&server.uri(), MatrixLogin::Password("hunter2".into()));
    let client = build_client(&account.homeserver, Some(&account.store))
        .await
        .unwrap();
    let saved = watch::Sender::new(None);
    sign_in(&client, &account, None, &saved).await.unwrap();
    let json = saved_json(saved.borrow().as_deref().unwrap());
    assert_eq!(json["kind"], "matrix");
    assert_eq!(json["user_id"], client.user_id().unwrap().as_str());
    assert!(json["access_token"].is_string());
}

#[tokio::test]
async fn restore_skips_login() {
    let server = homeserver().await;
    server.mock_login().ok().expect(0).mount().await;
    let session = Saved::Matrix(MatrixSession {
        meta: SessionMeta {
            user_id: owned_user_id!("@bot:x.org"),
            device_id: owned_device_id!("BOT"),
        },
        tokens: SessionTokens {
            access_token: "token".into(),
            refresh_token: None,
        },
    })
    .json()
    .unwrap();
    let account = account(&server.uri(), MatrixLogin::Password("hunter2".into()));
    let client = build_client(&account.homeserver, Some(&account.store))
        .await
        .unwrap();
    let saved = watch::Sender::new(None);
    sign_in(&client, &account, Some(&session), &saved)
        .await
        .unwrap();
    assert_eq!(client.user_id().unwrap(), "@bot:x.org");
    assert_eq!(client.device_id().unwrap(), "BOT");
    assert_eq!(*saved.borrow(), None);
}

#[tokio::test]
async fn oauth_without_session_fails_not_signed_in() {
    let server = homeserver().await;
    let session = Session::open(Service::Matrix(account(&server.uri(), MatrixLogin::OAuth)));
    let mut link = session.join("!room:x.org").unwrap();
    let failed = timeout(
        PATIENCE,
        link.state
            .wait_for(|state| matches!(state, LinkState::Failed(_))),
    )
    .await
    .unwrap()
    .unwrap()
    .clone();
    assert_eq!(failed, LinkState::Failed("Not signed in".into()));
}

#[tokio::test]
async fn sign_in_reports_the_code() {
    let server = homeserver().await;
    let oauth = server.oauth();
    oauth.mock_server_metadata().ok().mount().await;
    oauth.mock_registration().ok().expect(1).mount().await;
    oauth
        .mock_device_authorization()
        .ok()
        .expect(1)
        .mount()
        .await;
    let shown = matrix_sign_in(&server.uri(), &memory(), "sdrmm")
        .await
        .unwrap();
    assert_eq!(shown.user_code, "N32YVC");
    assert!(
        shown.verification_uri.contains("/link"),
        "{}",
        shown.verification_uri
    );
    assert_eq!(shown.expires_in, Duration::from_mins(20));
}

#[tokio::test]
async fn sign_in_ends_with_an_oauth_session() {
    let server = homeserver().await;
    let oauth = server.oauth();
    oauth.mock_server_metadata().ok().mount().await;
    oauth.mock_registration().ok().mount().await;
    oauth
        .mock_device_authorization()
        .ok_json(json!({
            "device_code": "device-code",
            "user_code": "CODE",
            "verification_uri": format!("{}/link", server.uri()),
            "expires_in": 60,
            "interval": 1,
        }))
        .mount()
        .await;
    oauth.mock_token().ok().mount().await;
    server.mock_who_am_i().ok().mount().await;
    let shown = matrix_sign_in(&server.uri(), &memory(), "sdrmm")
        .await
        .unwrap();
    let signed_in = timeout(PATIENCE, shown.done)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let json = saved_json(&signed_in.session);
    assert_eq!(json["kind"], "oauth");
    assert_eq!(json["user_id"], signed_in.user.as_str());
    assert_eq!(json["client_id"], "test_client_id");
    assert!(json["refresh_token"].is_string());
    assert!(!format!("{signed_in:?}").contains(json["access_token"].as_str().unwrap()));
}

fn key_content() -> Value {
    json!({
        "keys": {"index": 0, "key": "AAECAwQFBgcICQoLDA0ODw"},
        "member": {"id": "m1", "claimed_device_id": "ALICE"},
        "room_id": "!room:x.org",
        "session": {"application": "m.call", "call_id": "", "scope": "m.room"},
        "sent_ts": 1,
    })
}

fn to_device(kind: &str) -> Raw<matrix_sdk::ruma::events::AnyToDeviceEvent> {
    Raw::new(&json!({
        "type": kind,
        "sender": "@alice:x.org",
        "content": key_content(),
    }))
    .unwrap()
    .cast_unchecked()
}

#[test]
fn a_clear_key_names_no_device() {
    let event = ProcessedToDeviceEvent::PlainText(to_device("io.element.call.encryption_keys"));
    let Some(SyncEvent::Key { sender, device, .. }) = key_of(&event) else {
        panic!("no key");
    };
    assert_eq!(sender, "@alice:x.org");
    assert_eq!(device, None);
}

#[test]
fn an_olm_key_names_its_device() {
    let event = ProcessedToDeviceEvent::Decrypted {
        raw: to_device("m.rtc.encryption_key"),
        encryption_info: EncryptionInfo {
            sender: owned_user_id!("@alice:x.org"),
            sender_device: Some(owned_device_id!("ALICE")),
            forwarder: None,
            algorithm_info: AlgorithmInfo::OlmV1Curve25519AesSha2 {
                curve25519_public_key_base64: String::new(),
            },
            verification_state: VerificationState::Unverified(VerificationLevel::UnsignedDevice),
        },
    };
    let Some(SyncEvent::Key { device, .. }) = key_of(&event) else {
        panic!("no key");
    };
    assert_eq!(device.as_deref(), Some("ALICE"));
    let other = ProcessedToDeviceEvent::PlainText(to_device("m.room_key_request"));
    assert_eq!(key_of(&other), None);
}

fn member_event(kind: &str, sent: u64) -> Value {
    json!({
        "type": kind,
        "state_key": "_@alice:x.org_ALICE_m.call",
        "sender": "@alice:x.org",
        "event_id": format!("$e{sent}"),
        "origin_server_ts": sent,
        "content": {"application": "m.call", "device_id": "ALICE"},
    })
}

#[test]
fn state_members_carry_their_send_time() {
    let room = owned_room_id!("!room:x.org");
    let timeline = TimelineEvent::from_plaintext(
        Raw::new(&member_event("m.rtc.member", 20))
            .unwrap()
            .cast_unchecked(),
    );
    let message = TimelineEvent::from_plaintext(
        Raw::new(&json!({
            "type": "m.room.message",
            "sender": "@alice:x.org",
            "event_id": "$m",
            "origin_server_ts": 30,
            "content": {"body": "hi"},
        }))
        .unwrap()
        .cast_unchecked(),
    );
    let update = JoinedRoomUpdate {
        state: State::Before(vec![
            Raw::new(&member_event("org.matrix.msc3401.call.member", 10))
                .unwrap()
                .cast_unchecked(),
        ]),
        timeline: Timeline {
            events: vec![timeline, message],
            ..Timeline::default()
        },
        ..JoinedRoomUpdate::default()
    };
    let sent: Vec<(String, u64)> = members_of(&room, &update)
        .into_iter()
        .map(|event| match event {
            SyncEvent::Member { room, state } => (room, state.sent_ms),
            SyncEvent::Key { .. } => panic!("not a member"),
        })
        .collect();
    assert_eq!(
        sent,
        [
            ("!room:x.org".to_owned(), 10),
            ("!room:x.org".to_owned(), 20)
        ]
    );
}
