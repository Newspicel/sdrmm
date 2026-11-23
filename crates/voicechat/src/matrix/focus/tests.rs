use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use matrix_sdk::ruma::OwnedServerName;
use matrix_sdk::ruma::api::client::account::request_openid_token::v3::Response as OpenIdToken;
use serde_json::{Value, json};

use super::*;

type Seen = Arc<Mutex<Vec<(String, Value)>>>;

#[derive(Clone)]
struct Fake {
    seen: Seen,
    new_status: StatusCode,
    old_status: StatusCode,
}

fn handle(fake: &Fake, path: &str, status: StatusCode, body: Value) -> (StatusCode, Json<Value>) {
    if let Ok(mut seen) = fake.seen.lock() {
        seen.push((path.to_owned(), body));
    }
    (
        status,
        Json(json!({"url": format!("wss://sfu{path}"), "jwt": "tok"})),
    )
}

async fn get_token(State(f): State<Fake>, Json(b): Json<Value>) -> (StatusCode, Json<Value>) {
    handle(&f, "/get_token", f.new_status, b)
}

async fn sfu_get(State(f): State<Fake>, Json(b): Json<Value>) -> (StatusCode, Json<Value>) {
    handle(&f, "/sfu/get", f.old_status, b)
}

async fn serve(new_status: StatusCode, old_status: StatusCode) -> (Focus, Seen) {
    let seen = Seen::default();
    let fake = Fake {
        seen: seen.clone(),
        new_status,
        old_status,
    };
    let app = Router::new()
        .route("/get_token", post(get_token))
        .route("/sfu/get", post(sfu_get))
        .with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let focus = Focus {
        service_url: format!("http://{}", listener.local_addr().unwrap()),
    };
    tokio::spawn(async move { axum::serve(listener, app).await });
    (focus, seen)
}

fn openid() -> OpenIdToken {
    let server: OwnedServerName = "example.org".try_into().unwrap();
    OpenIdToken::new(
        "abc".to_owned(),
        "Bearer".into(),
        server,
        std::time::Duration::from_secs(3600),
    )
}

async fn ask(focus: &Focus) -> Result<LiveKitGrant, String> {
    livekit_token(
        &Client::new(),
        focus,
        "!r:example.org",
        "@u:example.org",
        "DEV",
        "@u:example.org:DEV",
        &openid(),
    )
    .await
}

#[tokio::test]
async fn old_endpoint_is_tried_first() {
    let (focus, seen) = serve(StatusCode::OK, StatusCode::OK).await;
    let grant = ask(&focus).await.unwrap();
    assert_eq!(grant.jwt, "tok");
    assert_eq!(grant.url, "wss://sfu/sfu/get");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].1["room"], "!r:example.org");
    assert_eq!(seen[0].1["device_id"], "DEV");
    assert_eq!(seen[0].1["openid_token"]["access_token"], "abc");
}

#[tokio::test]
async fn new_endpoint_answers_when_old_is_missing() {
    let (focus, seen) = serve(StatusCode::OK, StatusCode::NOT_FOUND).await;
    let grant = ask(&focus).await.unwrap();
    assert_eq!(grant.url, "wss://sfu/get_token");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    let body = &seen[1].1;
    assert_eq!(body["room_id"], "!r:example.org");
    assert_eq!(body["slot_id"], "m.call#ROOM");
    assert_eq!(body["member"]["id"], "@u:example.org:DEV");
    assert_eq!(body["member"]["claimed_user_id"], "@u:example.org");
    assert_eq!(body["member"]["claimed_device_id"], "DEV");
    assert_eq!(body["openid_token"]["access_token"], "abc");
    assert_eq!(body["openid_token"]["token_type"], "Bearer");
    assert_eq!(body["openid_token"]["matrix_server_name"], "example.org");
    assert_eq!(body["openid_token"]["expires_in"], 3600);
}

#[tokio::test]
async fn a_refusal_reads_short() {
    let (focus, _) = serve(StatusCode::OK, StatusCode::FORBIDDEN).await;
    assert_eq!(ask(&focus).await.unwrap_err(), "Call server refused");
}

#[tokio::test]
async fn an_unreachable_server_reads_short() {
    let focus = Focus {
        service_url: "http://127.0.0.1:1".to_owned(),
    };
    assert_eq!(ask(&focus).await.unwrap_err(), "No call server");
}

#[test]
fn debug_hides_the_token() {
    let grant = LiveKitGrant {
        url: "wss://sfu".to_owned(),
        jwt: "secret-jwt".to_owned(),
    };
    let shown = format!("{grant:?}");
    assert!(shown.contains("wss://sfu"));
    assert!(!shown.contains("secret-jwt"));
}
