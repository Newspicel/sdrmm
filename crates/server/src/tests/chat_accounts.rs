use sdrmm_wire::{ChatAccount, ChatAccountsResponse, ChatService, event_output::Secret};

use super::*;

const TOKEN: &str = "discord-bot-token";

fn parse(body: &[u8]) -> ChatAccount {
    serde_json::from_slice(body).expect("account")
}

fn stored(secret: &Secret) -> &str {
    match secret {
        Secret::Stored { secret } => secret,
        Secret::Inline(_) => panic!("answered a value, not an id"),
    }
}

#[tokio::test]
async fn created_account_keeps_token_out_of_the_body() {
    let (app, store) = test_router_with_store();

    let (status, body) = request(
        app.clone(),
        "POST",
        "/api/chat-accounts",
        Some(&format!(
            r#"{{"id":"","name":"Net","service":"discord","token":"{TOKEN}"}}"#
        )),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert!(!String::from_utf8_lossy(&body).contains(TOKEN));
    let account = parse(&body);
    let ChatService::Discord { token } = &account.service else {
        panic!("a discord account stays discord");
    };
    assert_eq!(
        store.secret(stored(token)).expect("read").as_deref(),
        Some(TOKEN)
    );

    let (_, listed) = request(app, "GET", "/api/chat-accounts", None).await;
    assert!(!String::from_utf8_lossy(&listed).contains(TOKEN));
    let listed: ChatAccountsResponse = serde_json::from_slice(&listed).expect("list");
    assert_eq!(listed.accounts, vec![account]);
}

#[tokio::test]
async fn teamspeak_account_gets_an_identity() {
    let (app, store) = test_router_with_store();

    let (status, body) = request(
        app,
        "POST",
        "/api/chat-accounts",
        Some(r#"{"id":"","name":"Club","service":"teamspeak","address":"ts.example"}"#),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let ChatService::Teamspeak {
        identity, password, ..
    } = parse(&body).service
    else {
        panic!("a teamspeak account stays teamspeak");
    };
    assert!(!password.is_set());
    let identity = store.secret(stored(&identity)).expect("read");
    assert!(identity.is_some_and(|text| !text.is_empty()));
}

#[tokio::test]
async fn deleting_an_unknown_account_is_404() {
    let (app, _) = test_router_with_store();

    let (status, _) = request(app.clone(), "DELETE", "/api/chat-accounts/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = request(
        app,
        "PUT",
        "/api/chat-accounts/nope",
        Some(r#"{"id":"nope","name":"Net","service":"discord"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
