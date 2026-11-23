use std::time::Duration;

use matrix_sdk::authentication::matrix::MatrixSession;
use matrix_sdk::authentication::oauth::{ClientId, OAuthSession, UserSession};
use matrix_sdk::deserialized_responses::{
    AlgorithmInfo, EncryptionInfo, ProcessedToDeviceEvent, TimelineEvent, VerificationLevel,
    VerificationState,
};
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{owned_device_id, owned_room_id, owned_user_id, user_id};
use matrix_sdk::sync::{JoinedRoomUpdate, State, Timeline};
use matrix_sdk::test_utils::mocks::MatrixMockServer;
use matrix_sdk::{SessionMeta, SessionTokens};
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio::time::timeout;

use super::account::connect::{Fault, Saved, build_client, open, save, sign_in};
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
    sign_in(&client, &account, None, &saved, None)
        .await
        .unwrap();
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
    sign_in(&client, &account, Some(&session), &saved, None)
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
    let shown = matrix_sign_in(&server.uri(), &memory(), "sdrmm", None)
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

/// A homeserver that approves a device sign-in at once, as `@bot:x.org` unless `as_other`.
async fn approving(as_other: bool) -> MatrixMockServer {
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
    let who = server.mock_who_am_i();
    if as_other {
        who.ok().mount().await;
    } else {
        who.ok_with_user_id(user_id!("@bot:x.org")).mount().await;
    }
    server
}

#[tokio::test]
async fn sign_in_as_another_user_keeps_the_store() {
    let server = approving(true).await;
    let dir = tempfile::tempdir().unwrap();
    let store = MatrixStore {
        dir: Some(dir.path().join("store")),
        passphrase: "secret".into(),
    };
    let kept = dir.path().join("store").join("keys");
    std::fs::create_dir_all(dir.path().join("store")).unwrap();
    std::fs::write(&kept, b"device A").unwrap();
    let shown = matrix_sign_in(&server.uri(), &store, "sdrmm", Some("@bot:x.org"))
        .await
        .unwrap();
    let refused = timeout(PATIENCE, shown.done).await.unwrap().unwrap();
    assert_eq!(refused.unwrap_err(), "Signed in as another user");
    assert!(kept.exists());
}

#[tokio::test]
async fn sign_in_names_the_user_in_any_case() {
    let server = approving(false).await;
    let shown = matrix_sign_in(&server.uri(), &memory(), "sdrmm", Some("@bot:X.org"))
        .await
        .unwrap();
    let signed_in = timeout(PATIENCE, shown.done).await.unwrap().unwrap();
    assert_eq!(signed_in.unwrap().user, "@bot:x.org");
}

#[tokio::test]
async fn sign_in_ends_with_an_oauth_session() {
    let server = approving(false).await;
    let shown = matrix_sign_in(&server.uri(), &memory(), "sdrmm", Some("@bot:x.org"))
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

fn device_session(device: &str) -> String {
    Saved::Matrix(MatrixSession {
        meta: SessionMeta {
            user_id: owned_user_id!("@bot:x.org"),
            device_id: device.into(),
        },
        tokens: SessionTokens {
            access_token: "token".into(),
            refresh_token: None,
        },
    })
    .json()
    .unwrap()
}

/// A store folder holding device `A`'s keys.
async fn store_of_device_a(homeserver: &str, dir: &std::path::Path) -> MatrixStore {
    let store = MatrixStore {
        dir: Some(dir.join("store")),
        passphrase: "secret".into(),
    };
    let client = build_client(homeserver, Some(&store)).await.unwrap();
    let saved = watch::Sender::new(None);
    let account = account(homeserver, MatrixLogin::Password("hunter2".into()));
    sign_in(&client, &account, Some(&device_session("A")), &saved, None)
        .await
        .unwrap();
    client
        .encryption()
        .wait_for_e2ee_initialization_tasks()
        .await;
    store
}

#[tokio::test]
async fn a_fresh_login_starts_on_a_cleared_store() {
    let server = homeserver().await;
    server.mock_login().ok().expect(1).mount().await;
    let dir = tempfile::tempdir().unwrap();
    let mut account = account(&server.uri(), MatrixLogin::Password("hunter2".into()));
    account.store = store_of_device_a(&server.uri(), dir.path()).await;
    let saved = watch::Sender::new(None);
    let client = open(&account, None, &saved, None).await.unwrap();
    assert_ne!(client.device_id().unwrap(), "A");
    assert!(saved.borrow().is_some());
}

#[tokio::test]
async fn a_store_of_another_device_is_a_lasting_fault() {
    let server = homeserver().await;
    let dir = tempfile::tempdir().unwrap();
    let mut account = account(&server.uri(), MatrixLogin::Password("hunter2".into()));
    account.store = store_of_device_a(&server.uri(), dir.path()).await;
    let saved = watch::Sender::new(None);
    let fault = open(&account, Some(&device_session("B")), &saved, None)
        .await
        .err()
        .unwrap();
    assert_eq!(fault, Fault::lasting("Store unreadable"));
}

#[tokio::test]
async fn the_first_login_handed_back_keeps_the_link() {
    let server = homeserver().await;
    server.mock_login().ok().mount().await;
    let mut account = account(&server.uri(), MatrixLogin::Password("hunter2".into()));
    account.nickname = "Relay".into();
    let mut session = Session::open(Service::Matrix(account));
    let _room = session.join("!room:x.org").unwrap();
    let mut saved = session.saved().unwrap();
    let first = timeout(PATIENCE, saved.wait_for(Option::is_some))
        .await
        .unwrap()
        .unwrap()
        .clone();
    let mut next = session.service().clone();
    if let Service::Matrix(account) = &mut next {
        account.session = first;
    }
    assert_eq!(session.update(next), Ok(()));
    assert_eq!(
        session.join("!room:x.org").unwrap_err(),
        crate::VoiceChatError::Clash("!room:x.org".into())
    );
}

#[test]
fn a_saved_session_hands_out_its_access_token() {
    assert_eq!(
        session_access_token(&device_session("A")).as_deref(),
        Some("token")
    );
    let oauth = Saved::OAuth(OAuthSession {
        client_id: ClientId::new("client".into()),
        user: UserSession {
            meta: SessionMeta {
                user_id: owned_user_id!("@bot:x.org"),
                device_id: owned_device_id!("BOT"),
            },
            tokens: SessionTokens {
                access_token: "oauth-token".into(),
                refresh_token: Some("refresh".into()),
            },
        },
    })
    .json()
    .unwrap();
    assert_eq!(session_access_token(&oauth).as_deref(), Some("oauth-token"));
    assert_eq!(session_access_token("{}"), None);
    assert_eq!(session_user(&oauth).as_deref(), Some("@bot:x.org"));
    assert_eq!(session_user("{}"), None);
}

#[test]
fn user_ids_compare_in_lowercase() {
    assert!(same_user("@bot:x.org", "@Bot:X.org"));
    assert!(!same_user("@bot:x.org", "@bot2:x.org"));
}

async fn failed_with(session: &Session) -> LinkState {
    let mut link = session.join("!room:x.org").unwrap();
    timeout(
        PATIENCE,
        link.state
            .wait_for(|state| matches!(state, LinkState::Failed(_))),
    )
    .await
    .unwrap()
    .unwrap()
    .clone()
}

#[tokio::test]
async fn a_revoked_password_session_logs_in_once_more() {
    let server = homeserver().await;
    server.mock_login().ok().expect(2).mount().await;
    server.mock_sync().error_unknown_token(false).mount().await;
    let account = account(&server.uri(), MatrixLogin::Password("hunter2".into()));
    let session = Session::open(Service::Matrix(account));
    assert_eq!(
        failed_with(&session).await,
        LinkState::Failed("Not signed in".into())
    );
}

#[tokio::test]
async fn a_revoked_oauth_session_does_not_log_in() {
    let server = homeserver().await;
    server.mock_login().ok().expect(0).mount().await;
    server.mock_sync().error_unknown_token(false).mount().await;
    let mut account = account(&server.uri(), MatrixLogin::OAuth);
    account.session = Some(device_session("BOT"));
    let session = Session::open(Service::Matrix(account));
    assert_eq!(
        failed_with(&session).await,
        LinkState::Failed("Not signed in".into())
    );
}

#[tokio::test]
async fn a_saved_session_carries_the_recovery_key_it_made() {
    let server = homeserver().await;
    let account = account(&server.uri(), MatrixLogin::Password("hunter2".into()));
    let client = build_client(&account.homeserver, Some(&account.store))
        .await
        .unwrap();
    let saved = watch::Sender::new(None);
    sign_in(
        &client,
        &account,
        Some(&device_session("BOT")),
        &saved,
        None,
    )
    .await
    .unwrap();
    save(&client, &saved, Some("EsTc made"));
    let kept = saved.borrow().clone().unwrap();
    assert_eq!(saved_json(&kept)["recovery_key"], "EsTc made");
    let (session, key) = take_recovery_key(&kept).unwrap();
    assert_eq!(key, "EsTc made");
    assert_eq!(saved_json(&session).get("recovery_key"), None);
    assert_eq!(session_user(&session).as_deref(), Some("@bot:x.org"));
    assert_eq!(take_recovery_key(&session), None);

    save(&client, &saved, None);
    assert_eq!(
        saved_json(saved.borrow().as_deref().unwrap()).get("recovery_key"),
        None
    );
}

#[test]
fn the_recovery_key_a_backend_made_keeps_its_session() {
    let before = account("https://x.org", MatrixLogin::Password("hunter2".into()));
    let mut next = before.clone();
    next.recovery_key = Some("EsTc made".into());
    let latest = r#"{"kind":"matrix","user_id":"@bot:x.org","device_id":"BOT","access_token":"t","recovery_key":"EsTc made"}"#;
    assert!(before.renamed_to(&next, Some(latest)));
    next.recovery_key = Some("EsTc other".into());
    assert!(!before.renamed_to(&next, Some(latest)));
    assert!(!before.renamed_to(&next, None));
}
