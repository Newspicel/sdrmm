use super::*;

#[tokio::test]
async fn channel_audio_record_list_download_and_delete_roundtrip_over_http() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let (app, engine) = recording_router_without_reconcilers(dir.path());
    let ds = open_virtual_radio(&app).await;
    let ch = create_nfm_channel(&app, ds).await;

    let live = engine
        .start_audio_recording("rec", &sdrmm_wire::AudioRoute::channel(ds, ch), false)
        .expect("recording starts");
    assert!(live.file.ends_with(".wav"));
    wait_for_recorded_frames(&app, ds, ch, 4_800).await;
    let done = engine
        .stop_audio_recording("rec", ds, ch)
        .expect("recording stops");
    assert_eq!(done.file, live.file);
    assert!(done.frames > 0);
    assert_eq!(done.bytes, done.frames * 2);
    assert_eq!(done.error, None);

    let listed = list_audio_recordings(&app).await;
    assert_eq!(listed.len(), 1);
    let rec = &listed[0];
    assert_eq!(rec.file, done.file);
    assert_eq!((rec.channels, rec.sample_rate), (1, 48_000));
    assert_eq!(rec.frames, done.frames);
    assert!(rec.duration_s > 0.0);
    rec.created_at
        .parse::<jiff::Timestamp>()
        .expect("rfc3339 created_at");

    let (status, wav) = request(
        app.clone(),
        "GET",
        &format!("/api/audiorecordings/{}/download", rec.file),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&wav[..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(wav.len() as u64, 44 + rec.bytes);

    let (status, _) = request(
        app.clone(),
        "DELETE",
        &format!("/api/audiorecordings/{}", rec.file),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(list_audio_recordings(&app).await.is_empty());
    let (status, _) = request(
        app.clone(),
        "DELETE",
        &format!("/api/audiorecordings/{}", rec.file),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_audio_recording_name_cannot_reach_outside_its_directory() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    std::fs::write(dir.path().join("secret.wav"), b"not yours").expect("plant");
    let app = recording_router(dir.path());
    for name in [
        "..%2Fsecret.wav",
        "..",
        "nothing.wav",
        "notes.txt",
        "%2Fetc%2Fpasswd",
    ] {
        let (status, _) = request(
            app.clone(),
            "GET",
            &format!("/api/audiorecordings/{name}/download"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{name} was served");
    }
}

#[tokio::test]
async fn network_export_start_stream_and_stop_roundtrip_over_http() {
    let app = test_router();
    open_virtual_radio(&app).await;
    let receiver = UdpSocket::bind("127.0.0.1:0").expect("bind receiver");
    receiver
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    let address = receiver.local_addr().expect("address");
    draw(
        &app,
        "network:test",
        exporter("cu8", &address.to_string()),
        &[["radio:band", "iq", "network:test", "iq"]],
    )
    .await;
    let live = exported(&app, "network:test", "start").await;
    assert_eq!(live.node, "network:test");
    assert_eq!(live.settings.address, address.to_string());

    let mut datagram = [0u8; 1_500];
    let received = receiver.recv(&mut datagram).expect("IQ datagram");
    assert!(received > 0);
    assert_eq!(received % 2, 0);

    let done = exported(&app, "network:test", "stop").await;
    assert!(done.samples > 0);
    assert_eq!(done.bytes, done.samples * 2);
    assert!(done.packets > 0);
    assert_eq!(done.error, None);
}

fn exporter(format: &str, address: &str) -> sdrmm_wire::NodeBody {
    sdrmm_wire::NodeBody::NetworkExport(
        serde_json::from_value(serde_json::json!({
            "transport": "udp", "format": format, "address": address,
        }))
        .expect("export settings"),
    )
}

async fn exported(app: &Router, node: &str, action: &str) -> NetworkExportStatus {
    let (status, body) = act_on(
        app,
        node,
        serde_json::json!({"action": "network_export", "with": action}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let sdrmm_wire::NodeActed::NetworkExport(status) = serde_json::from_slice(&body).expect("json")
    else {
        panic!("an export reports its status");
    };
    status
}

async fn pre_roll_of(app: &Router, ds: u32) -> Option<PreRollStatus> {
    get_state(app)
        .await
        .device_sets
        .iter()
        .find(|set| set.id == ds)
        .and_then(|set| set.pre_roll.clone())
}

async fn wait_for_capture(app: &Router, ds: u32, capturing: bool) -> PreRollStatus {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(held) = pre_roll_of(app, ds).await
            && held.capture.is_some() == capturing
        {
            return held;
        }
        assert!(Instant::now() < deadline, "the capture never settled");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn channel_baseband_record_roundtrip_lands_in_the_recording_library() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let (app, engine) = recording_router_without_reconcilers(dir.path());
    let ds = open_virtual_radio(&app).await;
    let ch = create_nfm_channel(&app, ds).await;

    let live = engine
        .start_channel_baseband_recording(ds, ch)
        .expect("baseband starts");
    assert!(live.file.starts_with(&format!("bb_{ds}_{ch}_")));
    live.started_at.parse::<jiff::Timestamp>().expect("rfc3339");
    assert!(engine.start_channel_baseband_recording(ds, ch).is_err());

    wait_for_baseband_samples(&app, ds, ch, 4_800).await;
    let done = engine
        .stop_channel_baseband_recording(ds, ch)
        .expect("baseband stops");
    assert_eq!(done.file, live.file);
    assert!(done.samples >= 4_800);
    assert_eq!(done.error, None);

    let listed = list_recordings(&app).await;
    let entry = listed
        .iter()
        .find(|entry| entry.file == done.file)
        .expect("the finished baseband pair is in the library");
    assert_eq!(entry.sample_rate, 48_000.0);
    assert_eq!(entry.center_hz, 100_000_000.0);
    assert_eq!(entry.samples, done.samples);
}

#[tokio::test]
async fn channel_network_export_streams_that_channels_baseband_over_http() {
    let app = test_router();
    let ds = open_virtual_radio(&app).await;
    create_nfm_channel(&app, ds).await;
    let receiver = UdpSocket::bind("127.0.0.1:0").expect("bind receiver");
    receiver
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    let address = receiver.local_addr().expect("address").to_string();
    draw(
        &app,
        "baseband:test",
        exporter("cf32_le", &address),
        &[["tap-nfm", "baseband", "baseband:test", "baseband"]],
    )
    .await;
    let live = exported(&app, "baseband:test", "start").await;
    assert_eq!(live.sample_rate, 48_000);
    assert_eq!(live.node, "baseband:test");

    let mut datagram = [0u8; 1_500];
    let received = receiver.recv(&mut datagram).expect("baseband datagram");
    assert!(received > 0 && received.is_multiple_of(8));

    let state = get_state(&app).await;
    let channel = &state.device_sets[0].channels[0];
    assert_eq!(
        channel
            .network_export
            .as_ref()
            .map(|export| export.node.as_str()),
        Some("baseband:test")
    );

    let done = exported(&app, "baseband:test", "stop").await;
    assert!(done.samples > 0);
    assert_eq!(done.error, None);
}

#[tokio::test]
async fn the_time_machine_arms_captures_and_files_its_window_over_http() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let app = recording_router(dir.path());
    let ds = open_virtual_radio(&app).await;
    let node = "recorder:test";
    draw(
        &app,
        node,
        pre_roll_node(1, false),
        &[["radio:band", "iq", node, "iq"]],
    )
    .await;

    let armed = wait_for_history(&app, ds).await;
    assert_eq!(armed.node, node);
    assert_eq!(armed.pre_roll_s, 1);
    assert_eq!(armed.capacity_samples, 2_048_000);
    assert!(armed.capture.is_none());

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let held = get_state(&app)
            .await
            .device_sets
            .iter()
            .find(|set| set.id == ds)
            .expect("set listed")
            .pre_roll
            .clone()
            .expect("armed")
            .held_samples;
        if held >= 1_024_000 {
            break;
        }
        assert!(Instant::now() < deadline, "the window never filled");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    edit_node(&app, node, pre_roll_node(1, true)).await;
    let capturing = wait_for_capture(&app, ds, true).await;
    let file = capturing.capture.expect("a capture is running").file;

    edit_node(&app, node, pre_roll_node(1, false)).await;
    wait_for_capture(&app, ds, false).await;

    let entry = wait_for_recording(&app, &file).await;
    assert_eq!(entry.sample_rate, 2_048_000.0);
    assert!(
        entry.samples >= 1_024_000,
        "the past never reached the file"
    );

    edit_node(&app, node, pre_roll_node(0, false)).await;
    wait_for_no_history(&app, ds).await;
}

async fn wait_for_recording(app: &Router, file: &str) -> sdrmm_wire::RecordingInfo {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(entry) = list_recordings(app)
            .await
            .into_iter()
            .find(|entry| entry.file == file)
        {
            return entry;
        }
        assert!(
            Instant::now() < deadline,
            "{file} never reached the library"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
