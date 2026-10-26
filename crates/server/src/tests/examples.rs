use super::*;

async fn listed(app: &Router) -> sdrmm_wire::ExamplesResponse {
    let (status, body) = request(app.clone(), "GET", "/api/examples", None).await;
    assert_eq!(status, StatusCode::OK);
    serde_json::from_slice(&body).expect("json")
}

async fn import(app: &Router, id: &str) -> (StatusCode, Bytes) {
    request(
        app.clone(),
        "POST",
        &format!("/api/examples/{id}/import"),
        None,
    )
    .await
}

async fn workspaces(app: &Router) -> sdrmm_wire::WorkspacesResponse {
    let (status, body) = request(app.clone(), "GET", "/api/workspaces", None).await;
    assert_eq!(status, StatusCode::OK);
    serde_json::from_slice(&body).expect("json")
}

#[tokio::test]
async fn examples_list_every_shipped_workspace() {
    let app = test_router();
    assert!(
        listed(&app)
            .await
            .examples
            .iter()
            .any(|e| e.id == "no-radio")
    );
}

#[tokio::test]
async fn importing_an_example_creates_a_workspace() {
    let app = test_router();
    let (status, body) = import(&app, "no-radio").await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let created: sdrmm_wire::CreatedRowId = serde_json::from_slice(&body).expect("json");
    assert!(
        workspaces(&app)
            .await
            .workspaces
            .iter()
            .any(|w| w.id == created.id)
    );
}

#[tokio::test]
async fn importing_twice_gives_two_workspaces_with_distinct_names() {
    let app = test_router();
    let (_, first) = import(&app, "no-radio").await;
    let (_, second) = import(&app, "no-radio").await;
    let first: sdrmm_wire::CreatedRowId = serde_json::from_slice(&first).expect("json");
    let second: sdrmm_wire::CreatedRowId = serde_json::from_slice(&second).expect("json");
    assert_ne!(first.id, second.id);

    let names: Vec<String> = workspaces(&app)
        .await
        .workspaces
        .iter()
        .map(|w| w.name.clone())
        .collect();
    let unique: std::collections::BTreeSet<&String> = names.iter().collect();
    assert_eq!(unique.len(), names.len(), "{names:?}");
}

#[tokio::test]
async fn an_unknown_example_is_not_found() {
    let app = test_router();
    let (status, _) = import(&app, "nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
