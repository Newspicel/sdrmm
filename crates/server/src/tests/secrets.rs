use sdrmm_wire::event_output::Secret;

use super::*;

#[tokio::test]
async fn a_secret_is_kept_by_the_server_and_answered_only_by_id() {
    let (app, store) = test_router_with_store();

    let (status, body) = request(
        app.clone(),
        "POST",
        "/api/secrets",
        Some(r#"{"value":"hook-token"}"#),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert!(!String::from_utf8_lossy(&body).contains("hook-token"));
    let Secret::Stored { secret } = serde_json::from_slice(&body).expect("secret") else {
        panic!("answered a value, not an id");
    };
    assert_eq!(
        store.secret(&secret).expect("read").as_deref(),
        Some("hook-token")
    );

    let (status, _) = request(
        app.clone(),
        "PUT",
        "/api/secrets/from-elsewhere",
        Some(r#"{"value":"filled"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        store.secret("from-elsewhere").expect("read").as_deref(),
        Some("filled")
    );
}

#[tokio::test]
async fn a_secret_needs_a_plain_id_and_a_value() {
    let (app, _) = test_router_with_store();
    for (method, uri, body) in [
        ("PUT", "/api/secrets/a.b", r#"{"value":"x"}"#),
        ("POST", "/api/secrets", r#"{"value":" "}"#),
    ] {
        let (status, _) = request(app.clone(), method, uri, Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {uri} {body}");
    }
}
