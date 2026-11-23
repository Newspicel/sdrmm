use sdrmm_wire::{Account, AccountService, AccountsResponse, Secret};

use super::*;

const TOKEN: &str = "discord-bot-token";

fn parse(body: &[u8]) -> Account {
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
        "/api/accounts",
        Some(&format!(
            r#"{{"id":"","name":"Net","service":"discord","token":"{TOKEN}"}}"#
        )),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert!(!String::from_utf8_lossy(&body).contains(TOKEN));
    let account = parse(&body);
    let AccountService::Discord { token, .. } = &account.service else {
        panic!("a discord account stays discord");
    };
    assert_eq!(
        store.secret(stored(token)).expect("read").as_deref(),
        Some(TOKEN)
    );

    let (_, listed) = request(app, "GET", "/api/accounts", None).await;
    assert!(!String::from_utf8_lossy(&listed).contains(TOKEN));
    let listed: AccountsResponse = serde_json::from_slice(&listed).expect("list");
    assert_eq!(listed.accounts, vec![account]);
}

#[tokio::test]
async fn teamspeak_account_gets_an_identity() {
    let (app, store) = test_router_with_store();

    let (status, body) = request(
        app,
        "POST",
        "/api/accounts",
        Some(r#"{"id":"","name":"Club","service":"teamspeak","address":"ts.example"}"#),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let AccountService::Teamspeak {
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

    let (status, _) = request(app.clone(), "DELETE", "/api/accounts/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = request(
        app,
        "PUT",
        "/api/accounts/nope",
        Some(r#"{"id":"nope","name":"Net","service":"discord"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_output_account_keeps_its_token_and_names_a_bad_field() {
    let (app, store) = test_router_with_store();

    let (status, body) = request(
        app.clone(),
        "POST",
        "/api/accounts",
        Some(
            r#"{"id":"","name":"Room","service":"matrix","homeserver_url":"https://matrix.example","access_token":"mx-token"}"#,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert!(!String::from_utf8_lossy(&body).contains("mx-token"));
    let AccountService::Matrix { access_token, .. } = parse(&body).service else {
        panic!("a matrix account stays matrix");
    };
    assert_eq!(
        store
            .secret(stored(&access_token))
            .expect("read")
            .as_deref(),
        Some("mx-token")
    );

    let (status, body) = request(
        app,
        "POST",
        "/api/accounts",
        Some(r#"{"id":"","name":"Hook","service":"webhook","url":"http://hooks.example/a"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(String::from_utf8_lossy(&body).contains("url"));
}
