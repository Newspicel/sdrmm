use super::*;

#[tokio::test]
async fn token_auth_gates_the_api_and_advertises_itself() {
    let app = test_router_with_options(&ServerOptions {
        dev_cors: false,
        token: Some("s3cret".to_string()),
        ..ServerOptions::default()
    });

    let (status, body) = request(app.clone(), "GET", "/api/auth", None).await;
    assert_eq!(status, StatusCode::OK);
    let info: sdrmm_wire::AuthInfo = serde_json::from_slice(&body).expect("json");
    assert!(info.token_required);

    let (status, _) = request(app.clone(), "GET", "/api/state", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = request(app.clone(), "GET", "/api/state?token=s3cret", None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = request(app, "GET", "/", None).await;
    assert_ne!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_reports_not_required_by_default() {
    let (status, body) = request(test_router(), "GET", "/api/auth", None).await;
    assert_eq!(status, StatusCode::OK);
    let info: sdrmm_wire::AuthInfo = serde_json::from_slice(&body).expect("json");
    assert!(!info.token_required);
}

async fn relayed_auth(app: Router, user: &str) -> sdrmm_wire::AuthInfo {
    let request = Request::builder()
        .uri("/api/auth")
        .extension(sdrmm_tunnel::Relayed {
            user: user.to_owned(),
        })
        .body(Body::empty())
        .expect("request");
    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1 << 16)
        .await
        .expect("body");
    serde_json::from_slice(&body).expect("json")
}

#[tokio::test]
async fn auth_tells_a_relay_whether_its_visitor_still_needs_a_login() {
    let guarded = test_router_with_options(&ServerOptions {
        dev_cors: false,
        token: Some("s3cret".to_string()),
        ..ServerOptions::default()
    });
    assert!(!relayed_auth(guarded, "user-1").await.token_required);
    assert!(relayed_auth(test_router(), "").await.token_required);
}

#[tokio::test]
async fn mcp_is_mounted_and_shares_the_token_gate() {
    let app = test_router_with_options(&ServerOptions {
        dev_cors: false,
        token: Some("s3cret".to_string()),
        ..ServerOptions::default()
    });
    let call = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    let (status, _) = request(app.clone(), "POST", "/mcp", Some(call)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("host", "sdrmm.local:8080")
                .header("authorization", "Bearer s3cret")
                .body(Body::from(call))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json-rpc body");
    let tools = json["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("no tools in {json}"));
    assert!(
        tools.iter().any(|t| t["name"] == "get_workspace"),
        "get_workspace missing from the tool list"
    );
}

#[tokio::test]
async fn mcp_serves_the_tool_bench_beside_the_receiver() {
    let app = test_router();

    let listed = mcp_call(&app, "list_tools", serde_json::json!({})).await;
    let tools = listed["result"]["structuredContent"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("no tool bench in {listed}"));
    assert!(tools.iter().any(|tool| tool["id"] == "antenna"));
    assert!(tools.iter().any(|tool| tool["id"] == "nanovna"));

    let found = mcp_call(
        &app,
        "run_tool",
        serde_json::json!({ "tool": "nanovna", "request": { "action": "list_devices" } }),
    )
    .await;
    let result = &found["result"]["structuredContent"]["result"];
    assert_eq!(result["kind"], "devices", "{found}");
    assert_eq!(result["devices"][0]["port"], "fixture-port");
    assert_eq!(result["ignored_ports"][0], "fixture-gnss");

    let cut = mcp_call(
        &app,
        "run_tool",
        serde_json::json!({
            "tool": "antenna",
            "request": {
                "frequency_hz": 145_500_000.0,
                "design": { "type": "yagi", "settings": { "directors": 3 } },
            },
        }),
    )
    .await;
    let report = &cut["result"]["structuredContent"]["result"];
    assert_eq!(report["design"]["type"], "yagi", "{cut}");
    assert_eq!(report["design"]["settings"]["directors"], 3);
    let parts = report["parts"]
        .as_array()
        .unwrap_or_else(|| panic!("no parts in {cut}"));
    assert!(parts.iter().any(|part| part["name"] == "Director 3"));
}

fn refusal(answer: &serde_json::Value) -> &str {
    answer["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("not refused: {answer}"))
}

#[tokio::test]
async fn mcp_tool_bench_refusals_name_what_was_wrong() {
    let app = test_router();
    let antenna = |design: serde_json::Value, frequency_hz: f64| {
        serde_json::json!({
            "tool": "antenna",
            "request": { "frequency_hz": frequency_hz, "design": design },
        })
    };

    let unknown = mcp_call(
        &app,
        "run_tool",
        antenna(serde_json::json!({ "type": "helix" }), 145_500_000.0),
    )
    .await;
    assert!(refusal(&unknown).contains("helix"), "{unknown}");

    let refused = mcp_call(
        &app,
        "run_tool",
        antenna(serde_json::json!({ "type": "dipole" }), 0.0),
    )
    .await;
    assert!(refusal(&refused).contains("frequency_hz"), "{refused}");

    let too_many = mcp_call(
        &app,
        "run_tool",
        serde_json::json!({
            "tool": "nanovna",
            "request": {
                "action": "sweep",
                "port": "fixture-port",
                "start_hz": 1_000_000,
                "stop_hz": 30_000_000,
                "points": 10_001,
            },
        }),
    )
    .await;
    assert!(refusal(&too_many).contains("401 points"), "{too_many}");

    let no_slot = mcp_call(
        &app,
        "run_tool",
        serde_json::json!({
            "tool": "nanovna",
            "request": { "action": "calibrate", "port": "fixture-port", "step": "save" },
        }),
    )
    .await;
    assert!(refusal(&no_slot).contains("slot"), "{no_slot}");
}

#[tokio::test]
async fn a_phone_cannot_reach_mcp() {
    let (app, state) = tls_router_with_state();
    let paired = crate::phones::tests::pair_one(&state.phones);
    let bearer = format!("Bearer {}", paired.token);
    let call = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    let (status, _, body) = request_parts(
        app,
        "POST",
        "/mcp",
        Some(call),
        &[
            ("authorization", bearer.as_str()),
            ("accept", "application/json, text/event-stream"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let error: ApiError = serde_json::from_slice(&body).expect("error body");
    assert_eq!(error.error, "Not open to phones");
}
