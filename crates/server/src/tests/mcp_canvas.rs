use serde_json::{Value, json};

use super::*;

async fn answer(app: &Router, tool: &str, arguments: Value) -> Value {
    let answer = mcp_call(app, tool, arguments).await;
    let content = &answer["result"]["structuredContent"];
    assert!(content.is_object(), "{tool} refused: {answer}");
    content.clone()
}

async fn add(app: &Router, body: Value) -> String {
    let added = answer(app, "put_node", json!({ "body": body })).await;
    added["node"]
        .as_str()
        .unwrap_or_else(|| panic!("no node in {added}"))
        .to_owned()
}

fn port(node: &str, port: &str) -> Value {
    json!({ "node": node, "port": port })
}

async fn wire(app: &Router, tool: &str, from: (&str, &str), to: (&str, &str)) -> Value {
    let arguments = json!({ "from": port(from.0, from.1), "to": port(to.0, to.1) });
    answer(app, tool, arguments).await
}

async fn decoders(app: &Router) -> Vec<sdrmm_wire::ChannelInfo> {
    get_state(app)
        .await
        .device_sets
        .into_iter()
        .flat_map(|set| set.channels)
        .collect()
}

async fn running(app: &Router) -> Vec<String> {
    let view = answer(app, "get_workspace", json!({})).await;
    serde_json::from_value(view["running"].clone()).expect("running nodes")
}

#[tokio::test]
async fn mcp_builds_a_receiver_on_the_canvas_and_takes_it_down() {
    let app = test_router();
    let radio = add(
        &app,
        json!({ "kind": "device", "data": { "device": { "backend": "virtual", "key": "band" } } }),
    )
    .await;
    assert_eq!(get_state(&app).await.device_sets.len(), 1);

    let decoder = add(
        &app,
        json!({ "kind": "channel", "data": { "channel_type": "nfm" } }),
    )
    .await;
    assert!(
        decoders(&app).await.is_empty(),
        "an unwired decoder runs nowhere"
    );
    wire(&app, "connect", (&radio, "iq"), (&decoder, "iq")).await;
    assert_eq!(decoders(&app).await.len(), 1);
    assert!(running(&app).await.contains(&decoder));

    let set = answer(
        &app,
        "set_channel",
        json!({ "node": decoder, "settings": nfm_at(25_000.0) }),
    )
    .await;
    assert_eq!(set["live"], true);
    assert!((decoders(&app).await[0].settings.frequency_hz - 100_025_000.0).abs() < 1.0);

    let tuned = answer(
        &app,
        "tune_radio",
        json!({ "node": radio, "settings": { "center_hz": 100_100_000.0 } }),
    )
    .await;
    assert_eq!(tuned["center_hz"], 100_100_000.0);

    let viewed = answer(&app, "get_node", json!({ "node": decoder })).await;
    assert_eq!(
        viewed["decoder"]["settings"]["params"]["type"], "nfm",
        "{viewed}"
    );

    wire(&app, "disconnect", (&radio, "iq"), (&decoder, "iq")).await;
    assert!(
        decoders(&app).await.is_empty(),
        "a cut wire closes the decoder"
    );

    answer(&app, "remove_node", json!({ "node": radio })).await;
    assert!(get_state(&app).await.device_sets.is_empty());
    let view = answer(&app, "get_workspace", json!({})).await;
    let nodes = view["snapshot"]["graph"]["nodes"]
        .as_array()
        .expect("nodes");
    assert!(nodes.iter().all(|node| node["id"] != radio.as_str()));
}

#[tokio::test]
async fn mcp_settings_wait_on_a_decoder_that_runs_nowhere() {
    let app = test_router();
    let decoder = add(
        &app,
        json!({ "kind": "channel", "data": { "channel_type": "nfm" } }),
    )
    .await;
    let set = answer(
        &app,
        "set_channel",
        json!({ "node": decoder, "settings": nfm_at(5_000.0) }),
    )
    .await;
    assert_eq!(set["live"], false);
    let viewed = answer(&app, "get_node", json!({ "node": decoder })).await;
    assert_eq!(viewed["saved"]["frequency_hz"], 100_005_000.0, "{viewed}");
}

#[tokio::test]
async fn mcp_scans_through_the_scanner_node() {
    let app = test_router();
    let radio = add(
        &app,
        json!({ "kind": "device", "data": { "device": { "backend": "virtual", "key": "band" } } }),
    )
    .await;
    let decoder = add(
        &app,
        json!({ "kind": "channel", "data": { "channel_type": "nfm" } }),
    )
    .await;
    wire(&app, "connect", (&radio, "iq"), (&decoder, "iq")).await;
    let scanner = add(
        &app,
        json!({ "kind": "scanner", "data": { "settings": {
            "channel": 0,
            "ranges": [{ "start_hz": 99_000_000.0, "stop_hz": 101_000_000.0, "step_hz": 100_000.0 }],
            "margin_db": 200.0,
            "hardware_sweep": false,
        } } }),
    )
    .await;

    let unwired = mcp_call(&app, "scan", json!({ "node": scanner, "action": "start" })).await;
    assert!(
        unwired["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("control")),
        "{unwired}"
    );

    wire(
        &app,
        "connect",
        (&scanner, "control"),
        (&decoder, "control"),
    )
    .await;
    let started = answer(&app, "scan", json!({ "node": scanner, "action": "start" })).await;
    assert_eq!(started["targets"], 21, "{started}");
    let viewed = answer(&app, "get_node", json!({ "node": scanner })).await;
    assert_eq!(viewed["scan"]["targets"], 21, "{viewed}");
    answer(&app, "scan", json!({ "node": scanner, "action": "stop" })).await;
    assert!(get_state(&app).await.device_sets[0].scanners.is_empty());
}

#[tokio::test]
async fn mcp_canvas_refusals_name_what_was_wrong() {
    let app = test_router();
    let refused = |answer: Value| {
        answer["error"]["message"]
            .as_str()
            .unwrap_or_else(|| panic!("not refused: {answer}"))
            .to_owned()
    };

    let missing = mcp_call(&app, "remove_node", json!({ "node": "nope" })).await;
    assert!(refused(missing).contains("nope"));

    let typo = mcp_call(
        &app,
        "put_node",
        json!({ "body": { "kind": "channel", "data": { "channel_type": "nfmx" } } }),
    )
    .await;
    assert!(refused(typo).contains("nfmx"));

    let crossed = mcp_call(
        &app,
        "connect",
        json!({ "from": port("scope", "iq"), "to": port("device", "iq") }),
    )
    .await;
    assert!(!refused(crossed).is_empty());

    let not_a_radio = mcp_call(
        &app,
        "tune_radio",
        json!({ "node": "scope", "settings": { "center_hz": 1.0 } }),
    )
    .await;
    assert!(refused(not_a_radio).contains("not a radio"));
}

#[tokio::test]
async fn mcp_undo_steps_the_canvas_back() {
    let app = test_router();
    let readout = add(&app, json!({ "kind": "readout" })).await;
    let undone = answer(&app, "undo", json!({})).await;
    let nodes = undone["snapshot"]["graph"]["nodes"]
        .as_array()
        .expect("nodes");
    assert!(nodes.iter().all(|node| node["id"] != readout.as_str()));
    let redone = answer(&app, "redo", json!({})).await;
    let nodes = redone["snapshot"]["graph"]["nodes"]
        .as_array()
        .expect("nodes");
    assert!(nodes.iter().any(|node| node["id"] == readout.as_str()));
}
