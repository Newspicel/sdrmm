use rusqlite::params;
use sdrmm_wire::{WorkspaceExport, WorkspaceSnapshot};
use serde_json::{Value, json};

use super::super::secrets;
use super::*;
use crate::Store;

const TOKEN: &str = "matrix-secret";

fn output(id: &str, target: &Value) -> Value {
    json!({
        "id": id,
        "position": { "x": 0.0, "y": 400.0 },
        "kind": "event_output",
        "data": { "target": target }
    })
}

fn matrix(room: &str, token: &Value) -> Value {
    json!({
        "service": "matrix",
        "homeserver_url": "https://matrix.example",
        "room_id": room,
        "access_token": token
    })
}

fn with_nodes(nodes: Vec<Value>) -> Value {
    let mut snapshot = serde_json::to_value(WorkspaceSnapshot::starter()).expect("snapshot");
    if let Some(list) = snapshot["graph"]["nodes"].as_array_mut() {
        list.extend(nodes);
    }
    snapshot
}

fn target_of(snapshot: &Value, node: &str) -> Value {
    snapshot["graph"]["nodes"]
        .as_array()
        .and_then(|nodes| nodes.iter().find(|found| found["id"] == node))
        .map_or(Value::Null, |found| found["data"]["target"].clone())
}

fn rows(store: &Store, id: i64) -> Vec<Value> {
    let texts: Vec<String> = store
        .hold()
        .prepare(
            "SELECT snapshot FROM workspaces WHERE id = ?1 \
             UNION ALL SELECT snapshot FROM workspace_history WHERE workspace_id = ?1",
        )
        .expect("query")
        .query_map([id], |row| row.get(0))
        .expect("rows")
        .collect::<Result<_, _>>()
        .expect("read");
    assert!(texts.iter().all(|text| !text.contains(TOKEN)), "{texts:?}");
    texts
        .iter()
        .map(|text| serde_json::from_str(text).expect("json"))
        .collect()
}

fn named(store: &Store, name: &str) -> Account {
    store
        .accounts()
        .expect("list")
        .into_iter()
        .find(|account| account.name == name)
        .unwrap_or_else(|| panic!("no account {name}"))
}

fn credential(store: &Store, account: &Account) -> Option<String> {
    match account.service.credentials()[0] {
        Secret::Stored { secret } => store.secret(secret).expect("read"),
        Secret::Inline(_) => None,
    }
}

/// A database as an older server left it: logins inside the workspace and its history.
fn old_database(file: &std::path::Path) -> i64 {
    let store = Store::open(Some(file)).expect("open");
    let id = store
        .create_workspace("Old", &WorkspaceSnapshot::starter())
        .expect("created");
    let conn = store.hold();
    let password: String = conn
        .query_row(
            "INSERT INTO secrets (id, value, updated_at) VALUES ('old-pw', 'broker-pw', 't') \
             RETURNING id",
            [],
            |row| row.get(0),
        )
        .expect("old secret");
    let current = with_nodes(vec![
        output("a", &matrix("!a:matrix.example", &json!(TOKEN))),
        output("b", &matrix("!b:matrix.example", &json!(TOKEN))),
        output(
            "broker",
            &json!({
                "service": "mqtt",
                "broker_url": "mqtts://broker.example",
                "topic": "sdrmm/events",
                "username": "radio",
                "password": { "secret": password }
            }),
        ),
        output(
            "blank",
            &json!({ "service": "webhook", "url": "", "format": "json" }),
        ),
    ]);
    let history = with_nodes(vec![
        output("a", &matrix("!a:matrix.example", &json!(TOKEN))),
        json!({
            "id": "discord",
            "position": { "x": 0.0, "y": 0.0 },
            "kind": "chat_output",
            "data": { "target": {
                "service": "discord",
                "webhook_url": "https://discord.com/api/webhooks/1/token"
            }}
        }),
    ]);
    conn.execute(
        "UPDATE workspaces SET snapshot = ?2 WHERE id = ?1",
        params![id, current.to_string()],
    )
    .expect("old row");
    conn.execute(
        "INSERT INTO workspace_history (workspace_id, seq, created_at, snapshot) \
         VALUES (?1, 99, '2026-01-01T00:00:00Z', ?2)",
        params![id, history.to_string()],
    )
    .expect("old history");
    conn.execute("DELETE FROM server_meta WHERE key = ?1", [LIFTED_KEY])
        .expect("not lifted yet");
    id
}

#[test]
fn an_old_database_moves_every_output_login_into_an_account() {
    let file = tempfile::NamedTempFile::new().expect("temp db");
    let id = old_database(file.path());

    let store = Store::open(Some(file.path())).expect("reopen");

    let names: Vec<String> = store
        .accounts()
        .expect("list")
        .into_iter()
        .map(|account| account.name)
        .collect();
    assert_eq!(names.len(), 3, "{names:?}");
    let room = named(&store, "Matrix matrix.example");
    let broker = named(&store, "MQTT broker.example");
    let hook = named(&store, "Webhook discord.com");
    assert_eq!(credential(&store, &room).as_deref(), Some(TOKEN));
    assert_eq!(credential(&store, &broker).as_deref(), Some("broker-pw"));
    assert_eq!(
        credential(&store, &hook).as_deref(),
        Some("https://discord.com/api/webhooks/1/token")
    );
    assert_eq!(store.secret("old-pw").expect("read"), None, "collected");

    let rows = rows(&store, id);
    let (current, history) = (&rows[0], &rows[1]);
    assert_eq!(
        target_of(current, "a"),
        json!({ "service": "matrix", "room_id": "!a:matrix.example", "account": room.id })
    );
    assert_eq!(target_of(current, "b")["account"], json!(room.id));
    assert_eq!(
        target_of(current, "broker"),
        json!({ "service": "mqtt", "topic": "sdrmm/events", "account": broker.id })
    );
    assert_eq!(
        target_of(current, "blank"),
        json!({ "service": "webhook", "format": "json", "account": "" })
    );
    assert_eq!(target_of(history, "a")["account"], json!(room.id));
    assert_eq!(
        target_of(history, "discord"),
        json!({ "service": "webhook", "format": "discord", "account": hook.id })
    );
}

#[test]
fn lifting_again_makes_no_new_account() {
    let file = tempfile::NamedTempFile::new().expect("temp db");
    let id = old_database(file.path());
    let store = Store::open(Some(file.path())).expect("reopen");
    let before = rows(&store, id);

    store
        .hold()
        .execute("DELETE FROM server_meta WHERE key = ?1", [LIFTED_KEY])
        .expect("forget the lift");
    lift_stored(&store.hold()).expect("lifted again");

    assert_eq!(store.accounts().expect("list").len(), 3);
    assert_eq!(rows(&store, id), before);
}

fn legacy_export(store: &Store, token: &Value) -> Value {
    let id = store
        .active_workspace()
        .expect("read")
        .expect("seeded")
        .info
        .id;
    let export = store.export_workspace(id).expect("export");
    let mut document = serde_json::to_value(export).expect("export");
    document["snapshot"] = with_nodes(vec![
        output("a", &matrix("!a:matrix.example", token)),
        output("b", &matrix("!b:matrix.example", token)),
    ]);
    document
}

fn import(store: &Store, mut document: Value) -> WorkspaceSnapshot {
    let logins = LegacyLogins::take(&mut document);
    let export: WorkspaceExport = serde_json::from_value(document).expect("typed");
    let id = store
        .import_workspace_with(&export, &[], &logins)
        .expect("imported");
    store.workspace(id).expect("read").snapshot
}

fn account_of(snapshot: &WorkspaceSnapshot, node: &str) -> String {
    let Some(NodeBody::EventOutput(output)) = snapshot.graph.node(node).map(|found| &found.body)
    else {
        panic!("no output {node}");
    };
    output
        .target
        .account()
        .map_or_default(|(id, _)| id.to_owned())
}

#[test]
fn an_old_export_brings_its_login_as_one_account() {
    let store = Store::open(None).expect("store");

    let first = import(&store, legacy_export(&store, &json!(TOKEN)));
    let second = import(&store, legacy_export(&store, &json!(TOKEN)));

    let accounts = store.accounts().expect("list");
    assert_eq!(accounts.len(), 1, "{accounts:?}");
    let room = &accounts[0];
    assert_eq!(room.name, "Matrix matrix.example");
    assert_eq!(credential(&store, room).as_deref(), Some(TOKEN));
    for snapshot in [&first, &second] {
        assert_eq!(account_of(snapshot, "a"), room.id);
        assert_eq!(account_of(snapshot, "b"), room.id);
    }
}

#[test]
fn an_import_never_reads_a_secret_it_names_by_id() {
    let store = Store::open(None).expect("store");
    let held = secrets::insert(&store.hold(), "someone-elses").expect("held");

    import(&store, legacy_export(&store, &json!({ "secret": held })));

    let accounts = store.accounts().expect("list");
    assert_eq!(accounts.len(), 1);
    assert!(!accounts[0].service.credentials()[0].is_set());
}
