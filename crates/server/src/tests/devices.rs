use super::*;

#[tokio::test]
async fn get_state_returns_empty_snapshot() {
    let app = test_router();
    let snap = get_state(&app).await;
    assert!(snap.device_sets.is_empty());
}

#[tokio::test]
async fn nmea_device_catalog_is_available_over_http() {
    let (status, body) = request(test_router(), "GET", "/api/position/nmea-devices", None).await;
    assert_eq!(status, StatusCode::OK);
    let response: NmeaDevicesResponse = serde_json::from_slice(&body).expect("NMEA devices");
    assert!(
        response
            .devices
            .iter()
            .all(|device| !device.path.is_empty())
    );
}

#[tokio::test]
async fn a_device_node_opens_its_radio_and_removing_it_closes_the_radio() {
    let app = test_router();
    open_virtual_radio(&app).await;
    let workspace = workspaces(&app).await.active.expect("seeded workspace");
    let mut snapshot = workspace_detail(&app, workspace).await.snapshot;
    snapshot.graph.nodes.retain(|node| node.id != "radio:band");
    put_workspace(&app, &snapshot).await;
    assert!(get_state(&app).await.device_sets.is_empty());
}

#[tokio::test]
async fn a_serial_goes_only_to_a_closed_radio_that_can_keep_one() {
    async fn write(app: &Router, device: &str) -> StatusCode {
        let body = format!(r#"{{"device_id":"{device}"}}"#);
        request(app.clone(), "POST", "/api/devices/serial", Some(&body))
            .await
            .0
    }
    let app = test_router();
    assert_eq!(write(&app, "virtual:band").await, StatusCode::BAD_REQUEST);
    assert_eq!(write(&app, "nothing:here").await, StatusCode::NOT_FOUND);
    open_virtual_radio(&app).await;
    assert_eq!(write(&app, "virtual:band").await, StatusCode::CONFLICT);
}

#[tokio::test]
async fn channeltypes_lists_every_demod_exactly_once() {
    let (status, body) = request(test_router(), "GET", "/api/channeltypes", None).await;
    assert_eq!(status, StatusCode::OK);
    let types: ChannelTypesResponse = serde_json::from_slice(&body).expect("json");
    for id in ["nfm", "selcall", "am", "ssb", "wfm", "freedv"] {
        assert!(
            types.types.iter().any(|t| t.type_id == id),
            "missing type {id}"
        );
    }
    let unique: std::collections::HashSet<&str> =
        types.types.iter().map(|t| t.type_id.as_str()).collect();
    assert_eq!(unique.len(), types.types.len());
}

#[tokio::test]
async fn channel_settings_go_to_a_channel_node_and_open_only_where_it_is_wired() {
    let app = test_router();
    let ds = open_virtual_radio(&app).await;
    let workspace = workspaces(&app).await.active.expect("seeded workspace");
    let mut snapshot = workspace_detail(&app, workspace).await.snapshot;
    snapshot.graph.nodes.push(sdrmm_wire::PatchNode {
        id: "loose".to_string(),
        body: sdrmm_wire::NodeBody::Channel(sdrmm_wire::ChannelNode {
            channel_type: "nfm".to_string(),
            tuning_locked: false,
        }),
        position: sdrmm_wire::Position { x: 400.0, y: 600.0 },
        size: None,
        label: None,
    });
    put_workspace(&app, &snapshot).await;
    let nfm = serde_json::json!({"settings": {"params": {"type": "nfm", "settings": {}}}});
    for (node, wanted) in [
        ("nobody", StatusCode::BAD_REQUEST),
        ("scope", StatusCode::BAD_REQUEST),
        ("loose", StatusCode::NO_CONTENT),
    ] {
        let (status, body) = patch_node_channel(&app, node, nfm.clone()).await;
        assert_eq!(status, wanted, "{node}: {}", String::from_utf8_lossy(&body));
    }
    let (status, _) = request(
        app.clone(),
        "PATCH",
        "/api/workspaces/active/nodes/loose/channel",
        Some(r#"{"params":{"type":"nfm"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    apply(&app, workspace).await;
    assert!(
        get_state(&app).await.device_sets[0].channels.is_empty(),
        "an unwired node holds settings but opens nothing"
    );

    create_nfm_channel(&app, ds).await;
    let channels = &get_state(&app).await.device_sets[0].channels;
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].node.as_deref(), Some("tap-nfm"));
}

#[tokio::test]
async fn channel_and_device_patches_by_node_and_their_error_mapping() {
    let app = test_router();
    let ds = open_virtual_radio(&app).await;
    let (status, _) = patch_node_device(&app, "radio:band", r#"{"tuning":"manual"}"#).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = patch_node_device(&app, "scope", r#"{"tuning":"manual"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a scope has no radio");

    open_channel(
        &app,
        ds,
        "tap-nfm",
        serde_json::json!({"frequency_hz": 100_100_000.0, "params": {"type": "nfm", "settings": {}}}),
    )
    .await;
    let retune = serde_json::json!({"settings": {"frequency_hz": 99_800_000.0, "params": {"type": "nfm", "settings": {}}}});
    let (status, _) = patch_node_channel(&app, "tap-nfm", retune).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let snap = get_state(&app).await;
    assert_eq!(
        snap.device_sets[0].channels[0].settings.frequency_hz,
        99_800_000.0
    );

    let other = serde_json::json!({"settings": {"params": {"type": "am", "settings": {}}}});
    let (status, _) = patch_node_channel(&app, "tap-nfm", other).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a type change is a retype");

    let unknown = serde_json::json!({"settings": {"params": {"type": "zzz", "settings": {}}}});
    let (status, body) = patch_node_channel(&app, "tap-nfm", unknown).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    serde_json::from_slice::<ApiError>(&body).expect("ApiError body");

    let far = serde_json::json!({"settings": {"frequency_hz": 900_000_000.0, "params": {"type": "nfm", "settings": {}}}});
    let (status, _) = patch_node_channel(&app, "tap-nfm", far).await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "a decoder may sit where the radio is not listening"
    );
    assert!(
        get_state(&app).await.device_sets[0].channels[0]
            .out_of_band
            .is_some()
    );
}

#[tokio::test]
async fn extractor_rejections_return_api_error_body() {
    let app = test_router();

    let (status, body) = request(
        app.clone(),
        "POST",
        "/api/devices/network",
        Some("{not json"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: ApiError = serde_json::from_slice(&body).expect("ApiError body");
    assert_eq!(err.error, "invalid request body");
    assert!(err.detail.is_some());

    let (status, body) = request(
        app.clone(),
        "POST",
        "/api/devices/network",
        Some(r#"{"nope":1}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let err: ApiError = serde_json::from_slice(&body).expect("ApiError body");
    assert_eq!(err.error, "invalid request body");

    let (status, body) = request(app, "DELETE", "/api/saved-radios/abc", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: ApiError = serde_json::from_slice(&body).expect("ApiError body");
    assert_eq!(err.error, "invalid path parameter");
}

#[tokio::test]
async fn dab_transmission_modes_round_trip_over_http() {
    let app = test_router();
    let ds = open_virtual_radio(&app).await;
    open_channel(
        &app,
        ds,
        "tap-dab",
        serde_json::json!({"frequency_hz": 100_000_000.0, "params": {"type": "dab", "settings": {}}}),
    )
    .await;
    for (name, expected) in [
        ("i", sdrmm_wire::DabTransmissionMode::I),
        ("ii", sdrmm_wire::DabTransmissionMode::Ii),
        ("iii", sdrmm_wire::DabTransmissionMode::Iii),
        ("iv", sdrmm_wire::DabTransmissionMode::Iv),
    ] {
        let body = serde_json::json!({"settings": {"params": {"type": "dab", "settings": {"transmission_mode": name, "service_id": 49569}}}}).to_string();
        let (status, _) = request(
            app.clone(),
            "PATCH",
            "/api/workspaces/active/nodes/tap-dab/channel",
            Some(&body),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let snapshot = get_state(&app).await;
        let sdrmm_wire::ChannelParams::Dab(params) =
            snapshot.device_sets[0].channels[0].settings.params
        else {
            panic!("DAB params");
        };
        assert_eq!(params.transmission_mode, expected);
        assert_eq!(params.service_id, Some(49569));
    }
    let (status, _) = request(
        app,
        "PATCH",
        "/api/workspaces/active/nodes/tap-dab/channel",
        Some(r#"{"settings":{"params":{"type":"dab","settings":{"transmission_mode":"v"}}}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn terrestrial_bandwidth_priority_and_service_round_trip_over_http() {
    let dir = tempfile::TempDir::new().expect("recording directory");
    let stem = dir.path().join("terrestrial");
    let mut writer = sdrmm_recorder::SigmfWriter::create(
        &stem,
        12_000_000.0,
        100_000_000.0,
        "DVB-T settings test",
    )
    .expect("writer");
    writer
        .write_block(&vec![num_complex::Complex::new(0.0, 0.0); 65536])
        .expect("IQ");
    writer.finalize().expect("recording");
    let app = recording_router(dir.path());
    let device_id = format!("recording:{}", stem.file_name().expect("stem").display());
    let ds = open_radio(&app, &device_id).await;
    open_channel(
        &app,
        ds,
        "tap-dvbt",
        serde_json::json!({"frequency_hz": 100_000_000.0, "params": {"type": "dvbt", "settings": {}}}),
    )
    .await;
    for (standard, bandwidth) in [
        ("dvb_t", "mhz6"),
        ("dvb_t", "mhz7"),
        ("dvb_t", "mhz8"),
        ("dvb_t", "khz333"),
        ("dvb_t", "mhz2"),
        ("dvb_t2", "mhz1_7"),
        ("dvb_t2", "mhz5"),
        ("dvb_t2", "mhz10"),
    ] {
        let params = serde_json::json!({"type":"dvbt","settings":{"standard":standard,"bandwidth":bandwidth,"low_priority":true,"program":42}});
        let body = serde_json::json!({"settings":{"params":params}}).to_string();
        let (status, _) = request(
            app.clone(),
            "PATCH",
            "/api/workspaces/active/nodes/tap-dvbt/channel",
            Some(&body),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let snapshot = get_state(&app).await;
        assert_eq!(
            serde_json::to_value(&snapshot.device_sets[0].channels[0].settings.params)
                .expect("params"),
            params
        );
    }
    let (status, _) = request(
        app,
        "PATCH",
        "/api/workspaces/active/nodes/tap-dvbt/channel",
        Some(r#"{"settings":{"params":{"type":"dvbt","settings":{"bandwidth":"mhz9"}}}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn satellite_superframes_and_stream_selection_round_trip_over_http() {
    let app = test_router();
    let ds = open_virtual_radio(&app).await;
    open_channel(
        &app,
        ds,
        "tap-datv",
        serde_json::json!({"frequency_hz": 100_000_000.0, "params": {"type": "datv", "settings": {"standard": "dvb_s2"}}}),
    )
    .await;
    for enabled in [true, false] {
        let body = serde_json::json!({"settings":{"params":{"type":"datv","settings":{"standard":"dvb_s2","superframes":enabled,"program":42,"input_stream":7}}}}).to_string();
        let (status, _) = request(
            app.clone(),
            "PATCH",
            "/api/workspaces/active/nodes/tap-datv/channel",
            Some(&body),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let snapshot = get_state(&app).await;
        let sdrmm_wire::ChannelParams::Datv(params) =
            snapshot.device_sets[0].channels[0].settings.params
        else {
            panic!("DATV params");
        };
        assert_eq!(params.superframes, enabled);
        assert_eq!(params.input_stream, Some(7));
        assert_eq!(params.program, Some(42));
    }
}

#[tokio::test]
async fn two_clients_change_different_fields_of_one_channel() {
    let app = test_router();
    let ds = open_virtual_radio(&app).await;
    let base = r#"{"frequency_hz":100100000.0,"params":{"type":"nfm","settings":{}}}"#;
    open_channel(
        &app,
        ds,
        "tap-nfm",
        serde_json::from_str(base).expect("json"),
    )
    .await;
    let uri = "/api/workspaces/active/nodes/tap-nfm/channel";
    let retuned = r#"{"frequency_hz":100200000.0,"params":{"type":"nfm","settings":{}}}"#;
    let squelched = r#"{"frequency_hz":100100000.0,"squelch":{"mode":"manual","level_db":-60.0},"params":{"type":"nfm","settings":{}}}"#;
    for settings in [retuned, squelched] {
        let (status, body) = request(
            app.clone(),
            "PATCH",
            uri,
            Some(&format!(r#"{{"base":{base},"settings":{settings}}}"#)),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NO_CONTENT,
            "{}",
            String::from_utf8_lossy(&body)
        );
    }
    let channel = &get_state(&app).await.device_sets[0].channels[0].settings;
    assert_eq!(
        channel.frequency_hz, 100_200_000.0,
        "the first edit survived"
    );
    assert!(
        matches!(channel.squelch, sdrmm_wire::Squelch::Manual { .. }),
        "the second edit landed"
    );
}
