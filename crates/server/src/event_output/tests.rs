use std::sync::Mutex;

use axum::{
    Router,
    body::to_bytes,
    extract::{Request, State},
    http::Method,
    response::{IntoResponse, Response as AxumResponse},
    routing::any,
};
use reqwest::StatusCode;
use sdrmm_channels::RttyText;
use sdrmm_wire::{
    AccountService, ChannelNode, DecoderEvent, EventAudio, EventOutputNode, MatrixLogin, PatchEdge,
    PatchGraph, PatchNode, PortRef, Position, RackLayout, Secret, UpdateWorkspaceRequest,
    VoiceCall, WorkspaceSnapshot,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;

#[derive(Clone)]
struct Captured {
    method: Method,
    uri: String,
    authorization: Option<String>,
    content_type: Option<String>,
    body: Bytes,
}

type Captures = Arc<Mutex<Vec<Captured>>>;

fn node(id: &str, body: NodeBody) -> PatchNode {
    PatchNode {
        id: id.to_owned(),
        body,
        position: Position { x: 0.0, y: 0.0 },
        size: None,
        label: None,
    }
}

fn edge(from: (&str, &str), to: (&str, &str)) -> PatchEdge {
    PatchEdge {
        from: PortRef {
            node: from.0.to_owned(),
            port: from.1.to_owned(),
        },
        to: PortRef {
            node: to.0.to_owned(),
            port: to.1.to_owned(),
        },
    }
}

fn discord_webhook() -> EventOutputTarget {
    EventOutputTarget::Webhook {
        account: "hook".to_owned(),
        format: WebhookFormat::Discord,
    }
}

fn binding(node: &str, target: EventOutputTarget) -> Binding {
    Binding::plain(node, target)
}

fn call() -> VoiceCall {
    VoiceCall {
        talkgroup_name: None,
        id: 7,
        node: "trunk".to_owned(),
        started_at: "2026-08-15T10:00:00Z".to_owned(),
        ended_at: "2026-08-15T10:00:01Z".to_owned(),
        duration_ms: 1_200,
        device_set: 1,
        channel: 2,
        freq_hz: 451_125_000.0,
        mode: "dmr".to_owned(),
        slot: Some(2),
        color_code: Some(3),
        source: Some(1001),
        destination: Some(91),
        group_call: Some(true),
        encrypted: false,
        emergency: false,
        audio: Some(EventAudio {
            url: "/api/calls/7/audio".to_owned(),
            media_type: "audio/wav".to_owned(),
        }),
        audio_error: None,
        talker_alias: None,
        key_id: None,
        transcript: None,
    }
}

fn call_record() -> DecodedRecord {
    DecodedRecord {
        origin: None,
        sinks: Vec::new(),
        callsign: None,
        freq_hz: 451_125_000.0,
        event: DecoderEvent::of(&call()),
        ..decoded()
    }
}

fn message(node: &str, record: &DecodedRecord) -> OutputMessage {
    decoded_message(node, record, 1, view_of(record), None, "")
}

fn clip_message(node: &str, bytes: Bytes) -> OutputMessage {
    let record = call_record();
    let view = view_of(&record);
    let audio = OutputAudio {
        bytes,
        stem: clip_stem(&record, &view),
        duration_ms: view.duration_ms.unwrap_or_default(),
    };
    decoded_message(node, &record, 1, view, Some(audio), "")
}

fn call_text(call: &VoiceCall) -> OutputMessage {
    message(
        "chat",
        &DecodedRecord {
            event: DecoderEvent::of(call),
            ..call_record()
        },
    )
}

fn stem_of(call: &VoiceCall) -> String {
    let record = DecodedRecord {
        event: DecoderEvent::of(call),
        ..call_record()
    };
    clip_stem(&record, &view_of(&record))
}

fn decoded() -> DecodedRecord {
    DecodedRecord {
        origin: None,
        sinks: vec!["matched".to_owned()],
        callsign: None,
        device_set: 1,
        channel: 2,
        at: "2026-08-15T10:00:02Z".to_owned(),
        freq_hz: 14_080_000.0,
        event: DecoderEvent::of(&RttyText {
            text: "CQ TEST".to_owned(),
        }),
        view: None,
    }
}

async fn server() -> (String, Captures) {
    let captures = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/{*path}", any(capture))
        .with_state(captures.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener");
    let address = listener.local_addr().expect("local address");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("test server") });
    (format!("http://{address}"), captures)
}

async fn capture(State(captures): State<Captures>, request: Request) -> AxumResponse {
    let method = request.method().clone();
    let uri = request.uri().to_string();
    let authorization = request
        .headers()
        .get(reqwest::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let content_type = request
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = to_bytes(request.into_body(), usize::MAX)
        .await
        .expect("request body");
    captures.lock().expect("captures").push(Captured {
        method,
        uri: uri.clone(),
        authorization,
        content_type,
        body,
    });
    if uri.starts_with("/oversized-error") {
        return (
            StatusCode::BAD_REQUEST,
            "x".repeat(MAX_ERROR_BODY.saturating_mul(4)),
        )
            .into_response();
    }
    if uri == "/rate-limited-header" {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(reqwest::header::RETRY_AFTER, "1.5")],
            r#"{"errcode":"M_LIMIT_EXCEEDED","retry_after_ms":9000}"#,
        )
            .into_response();
    }
    if uri == "/rate-limited-body" {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"errcode":"M_LIMIT_EXCEEDED","retry_after_ms":2500}"#,
        )
            .into_response();
    }
    if uri == "/rate-limited-bare" {
        return (StatusCode::TOO_MANY_REQUESTS, "slow down").into_response();
    }
    if uri == "/rate-limited-forever" {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(reqwest::header::RETRY_AFTER, "86400")],
            "",
        )
            .into_response();
    }
    if uri == "/rate-limited-past-duration" {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(reqwest::header::RETRY_AFTER, "1e30")],
            r#"{"retry_after_ms":1e300}"#,
        )
            .into_response();
    }
    if uri.contains("/_matrix/media/v3/upload") {
        return (
            StatusCode::OK,
            r#"{"content_uri":"mxc://matrix.test/audio7"}"#,
        )
            .into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

#[test]
fn a_call_message_leads_with_its_mode_and_holds_the_call_facts() {
    let body = call_text(&call()).body;
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines[0], "DMR call");
    for fact in ["talkgroup 91", "from 1001", "TS2", "1.2 s"] {
        assert!(lines[1].contains(fact), "{fact} in {body}");
    }
    assert!(body.contains("Frequency: 451.125000 MHz"), "{body}");
}

#[test]
fn an_emergency_encrypted_call_names_both_flags() {
    let body = call_text(&VoiceCall {
        emergency: true,
        encrypted: true,
        ..call()
    })
    .body;
    assert!(
        body.contains("emergency") && body.contains("encrypted"),
        "{body}"
    );
}

#[test]
fn the_html_caption_escapes_the_audio_error() {
    let message = call_text(&VoiceCall {
        audio_error: Some("lost <2> blocks & counting".to_owned()),
        ..call()
    });
    let html = message.html.expect("html");
    assert!(html.starts_with("<strong>DMR call</strong><br/>"), "{html}");
    assert!(
        html.ends_with("<br/>Audio: lost &lt;2&gt; blocks &amp; counting"),
        "{html}"
    );
    assert_eq!(
        message.audio_error.as_deref(),
        Some("lost <2> blocks & counting")
    );
}

#[test]
fn summary_contains_generic_decoder_metadata() {
    let text = message("chat", &decoded()).body;
    assert!(text.contains("RTTY decode"));
    assert!(text.contains("CQ TEST"));
    assert!(text.contains("Frequency: 14.080000 MHz"));
    assert!(text.contains("Received: 2026-08-15T10:00:02Z"));
}

#[test]
fn resolve_maps_configured_outputs_and_the_events_port() {
    let store = crate::Store::open(None).expect("open store");
    let active = store
        .active_workspace()
        .expect("read active workspace")
        .expect("seeded workspace");
    let configured = discord_webhook();
    let graph = PatchGraph {
        nodes: vec![
            node(
                "decoder",
                NodeBody::Channel(ChannelNode {
                    channel_type: "rtty".to_owned(),
                    tuning_locked: false,
                    audio: Default::default(),
                }),
            ),
            node(
                "configured",
                NodeBody::EventOutput(EventOutputNode {
                    message: String::new(),
                    target: configured.clone(),
                }),
            ),
            node("empty", NodeBody::EventOutput(EventOutputNode::default())),
        ],
        edges: vec![
            edge(("decoder", "events"), ("configured", "events")),
            edge(("decoder", "events"), ("empty", "events")),
        ],
    };
    store
        .update_workspace(
            active.info.id,
            &UpdateWorkspaceRequest {
                revision: active.info.revision,
                name: None,
                snapshot: Some(WorkspaceSnapshot::new(graph, RackLayout::default())),
            },
            None,
        )
        .expect("update workspace");

    let routing = routing_over(&store);
    let bindings = routing.bindings;

    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].node, "configured");
    assert_eq!(bindings[0].target, configured);
}

fn routing_for() -> Routing {
    Routing {
        bindings: vec![binding("matched", discord_webhook())],
    }
}

#[test]
fn decoded_records_route_only_to_outputs_the_record_reached() {
    let target = discord_webhook();
    let routing = Routing {
        bindings: vec![binding("matched", target.clone()), binding("other", target)],
    };

    let deliveries = decoded_deliveries(&routing, &decoded(), 9, &Calls::default());

    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].node, "matched");
    assert!(deliveries[0].message.body.contains("RTTY decode"));
    assert!(deliveries[0].message.transaction.ends_with("-rtty-9"));
}

#[test]
fn a_completed_call_travels_the_events_wire_like_any_other_decode() {
    let routing = routing_for();
    let record = DecodedRecord {
        origin: None,
        event: DecoderEvent::of(&call()),
        ..decoded()
    };

    let deliveries = decoded_deliveries(&routing, &record, 9, &Calls::default());

    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].event, "call /api/calls/7/audio");
    assert!(deliveries[0].message.body.contains("talkgroup 91"));
}

fn recordings_routing() -> Routing {
    Routing {
        bindings: vec![binding("matched", EventOutputTarget::Recordings)],
    }
}

fn from_voice(event: DecoderEvent) -> DecodedRecord {
    DecodedRecord {
        origin: Some(sdrmm_wire::EventOrigin {
            node: "voice".to_owned(),
            transmission: 0,
        }),
        event,
        ..decoded()
    }
}

fn analog_call() -> VoiceCall {
    VoiceCall {
        mode: "am".to_owned(),
        freq_hz: 144_100_000.0,
        started_at: "2026-10-01T22:31:00.250Z".to_owned(),
        slot: None,
        color_code: None,
        source: None,
        destination: None,
        group_call: None,
        ..call()
    }
}

#[test]
fn recordings_take_the_clip_the_event_brings() {
    let routing = recordings_routing();
    let calls = Calls::default();
    let (audio, _) = calls.store_clip(&[1, 2, 3]);
    assert!(decoded_deliveries(&routing, &decoded(), 1, &calls).is_empty());
    let clipped = VoiceCall {
        audio: Some(audio),
        ..analog_call()
    };
    let wired = from_voice(DecoderEvent::of(&clipped.clone()));
    let delivered = decoded_deliveries(&routing, &wired, 1, &calls);
    assert_eq!(delivered.len(), 1);
    assert!(delivered[0].message.audio.is_some());
    let silent = from_voice(DecoderEvent::of(&VoiceCall {
        audio: None,
        encrypted: true,
        ..call()
    }));
    assert!(decoded_deliveries(&routing, &silent, 1, &calls).is_empty());
}

#[test]
fn a_call_is_named_by_time_mode_frequency_and_talkgroup() {
    assert_eq!(stem_of(&analog_call()), "20261001T223100Z_AM_144.100MHz");
    assert_eq!(stem_of(&call()), "20260815T100000Z_DMR_451.125MHz_TG91");
    assert_eq!(
        stem_of(&VoiceCall {
            group_call: Some(false),
            ..call()
        }),
        "20260815T100000Z_DMR_451.125MHz_ID91"
    );
}

#[tokio::test]
async fn a_saved_call_lands_in_the_audio_library() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let engine = Engine::with_registry(
        sdrmm_device::DeviceRegistry::new(),
        Some(dir.path().to_path_buf()),
    );
    let calls = Calls::default();
    let (audio, _) = calls.store_clip(&[1, 2, 3]);
    let record = from_voice(DecoderEvent::of(&VoiceCall {
        audio: Some(audio),
        ..analog_call()
    }));
    let deliveries = decoded_deliveries(&recordings_routing(), &record, 1, &calls);
    let weak = Arc::downgrade(&engine);

    for _ in 0..2 {
        save_recording(&weak, &deliveries[0].message)
            .await
            .expect("saved");
    }

    let library = engine.audio_recordings_dir().expect("library");
    let saved = sdrmm_recorder::scan_audio(&library).expect("scan");
    let names: Vec<_> = saved
        .iter()
        .filter_map(|path| path.file_name()?.to_str())
        .collect();
    assert_eq!(
        names,
        [
            "20261001T223100Z_AM_144.100MHz-2.wav",
            "20261001T223100Z_AM_144.100MHz.wav"
        ]
    );
}

#[tokio::test]
async fn a_call_whose_audio_was_lost_says_why_it_saved_nothing() {
    let engine = Engine::with_registry(sdrmm_device::DeviceRegistry::new(), None);
    let record = from_voice(DecoderEvent::of(&VoiceCall {
        audio: None,
        audio_error: Some("audio stream lost 3 block(s)".to_owned()),
        ..analog_call()
    }));
    let deliveries = decoded_deliveries(&recordings_routing(), &record, 1, &Calls::default());
    let error = save_recording(&Arc::downgrade(&engine), &deliveries[0].message)
        .await
        .expect_err("nothing to save");
    assert!(error.to_string().contains("lost 3 block"), "{error}");
}

#[test]
fn a_record_that_reached_no_output_posts_nowhere() {
    let routing = routing_for();
    let unrouted = DecodedRecord {
        origin: None,
        sinks: Vec::new(),
        callsign: None,
        ..decoded()
    };

    assert!(decoded_deliveries(&routing, &unrouted, 9, &Calls::default()).is_empty());
}

#[tokio::test]
async fn a_rate_limited_response_reports_the_header_delay() {
    let (base, _captures) = server().await;
    let response = Client::new()
        .get(format!("{base}/rate-limited-header"))
        .send()
        .await
        .expect("rate limited response");

    let error = checked_response("Test", response)
        .await
        .expect_err("rate limited");

    assert!(matches!(
        error,
        DeliveryError::RateLimited(wait) if wait == Duration::from_millis(1_500)
    ));
}

#[tokio::test]
async fn a_rate_limited_response_falls_back_to_the_matrix_body_field() {
    let (base, _captures) = server().await;
    let response = Client::new()
        .get(format!("{base}/rate-limited-body"))
        .send()
        .await
        .expect("rate limited response");

    let error = checked_response("Test", response)
        .await
        .expect_err("rate limited");

    assert!(matches!(
        error,
        DeliveryError::RateLimited(wait) if wait == Duration::from_millis(2_500)
    ));
}

#[tokio::test]
async fn a_rate_limit_without_any_hint_uses_the_default_delay() {
    let (base, _captures) = server().await;
    let response = Client::new()
        .get(format!("{base}/rate-limited-bare"))
        .send()
        .await
        .expect("rate limited response");

    let error = checked_response("Test", response)
        .await
        .expect_err("rate limited");

    assert!(matches!(
        error,
        DeliveryError::RateLimited(wait) if wait == DEFAULT_RETRY_DELAY
    ));
}

#[tokio::test]
async fn an_absurd_retry_after_is_capped() {
    let (base, _captures) = server().await;
    let response = Client::new()
        .get(format!("{base}/rate-limited-forever"))
        .send()
        .await
        .expect("rate limited response");

    let error = checked_response("Test", response)
        .await
        .expect_err("rate limited");

    assert!(matches!(
        error,
        DeliveryError::RateLimited(wait) if wait == MAX_RETRY_DELAY
    ));
}

#[tokio::test]
async fn a_retry_after_beyond_any_duration_is_capped_without_a_panic() {
    let (base, _captures) = server().await;
    let response = Client::new()
        .get(format!("{base}/rate-limited-past-duration"))
        .send()
        .await
        .expect("rate limited response");

    let error = checked_response("Test", response)
        .await
        .expect_err("rate limited");

    assert!(matches!(
        error,
        DeliveryError::RateLimited(wait) if wait == MAX_RETRY_DELAY
    ));
    assert_eq!(seconds_to_delay(f64::MAX), Some(MAX_RETRY_DELAY));
    assert_eq!(
        retry_after_body(r#"{"retry_after_ms":1e308}"#),
        Some(MAX_RETRY_DELAY)
    );
}

#[tokio::test]
async fn a_panicking_delivery_fails_its_batch_and_spares_the_worker() {
    let crashed = guarded(async { panic!("boom") }).await;
    assert!(
        matches!(&crashed, Err(DeliveryError::Failed(reason)) if reason == "Delivery crashed: boom"),
        "{crashed:?}"
    );
    assert!(guarded(async { Ok(()) }).await.is_ok());
}

#[test]
fn a_stopped_delivery_worker_is_reported_on_every_output() {
    let statuses = status::Statuses::default();
    statuses.configure(&[binding("hook", json_webhook("a"))]);

    worker_ended(Ok(()), &statuses);

    let error = statuses.get("hook").and_then(|status| status.error);
    assert_eq!(error.as_deref(), Some("Delivery worker stopped, restarted"));
}

#[tokio::test]
async fn discord_sends_metadata_and_wav_in_one_webhook_message() {
    let (base, captures) = server().await;
    let message = clip_message("chat", Bytes::from_static(b"RIFF-wave"));
    send_webhook(
        &Client::new(),
        &format!("{base}/api/webhooks/1/token?thread_id=5&wait=false"),
        WebhookFormat::Discord,
        &message,
    )
    .await
    .expect("Discord delivery");
    let captured = captures.lock().expect("captures");
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].method, Method::POST);
    assert_eq!(
        captured[0].uri,
        "/api/webhooks/1/token?thread_id=5&wait=true"
    );
    assert!(
        captured[0]
            .content_type
            .as_deref()
            .is_some_and(|value| value.starts_with("multipart/form-data; boundary="))
    );
    let body = String::from_utf8_lossy(&captured[0].body);
    assert!(body.contains("payload_json"));
    assert!(body.contains("DMR call"));
    assert!(body.contains("20260815T100000Z_DMR_451.125MHz_TG91.wav"));
    assert!(body.contains("RIFF-wave"));
}

#[tokio::test]
async fn discord_sends_decoder_events_as_json() {
    let (base, captures) = server().await;
    let message = message("chat", &decoded());
    send_webhook(
        &Client::new(),
        &format!("{base}/api/webhooks/1/token"),
        WebhookFormat::Discord,
        &message,
    )
    .await
    .expect("Discord delivery");
    let captured = captures.lock().expect("captures");
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].method, Method::POST);
    assert!(
        captured[0]
            .content_type
            .as_deref()
            .is_some_and(|value| value.starts_with("application/json"))
    );
    let body: serde_json::Value = serde_json::from_slice(&captured[0].body).expect("Discord JSON");
    assert!(
        body["content"]
            .as_str()
            .is_some_and(|content| content.contains("RTTY decode") && content.contains("CQ TEST"))
    );
    assert_eq!(body["allowed_mentions"]["parse"], json!([]));
}

#[tokio::test]
async fn discord_request_errors_do_not_expose_the_webhook_url() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener");
    let address = listener.local_addr().expect("local address");
    drop(listener);
    let secret = "webhook-secret";
    let message = message("chat", &decoded());

    let error = send_discord(
        &Client::new(),
        &format!("http://{address}/api/webhooks/1/{secret}"),
        &message,
    )
    .await
    .expect_err("connection failure")
    .to_string();

    assert!(error.starts_with("Discord request:"));
    assert!(!error.contains(secret));
}

#[tokio::test]
async fn matrix_creates_one_audio_event_after_uploading_the_wav() {
    let (base, captures) = server().await;
    let message = clip_message("chat", Bytes::from_static(b"RIFF-wave"));
    send_matrix(
        &Client::new(),
        MatrixTarget {
            homeserver_url: &format!("{base}/matrix/"),
            room_id: "!radio:matrix.test",
            access_token: "matrix-secret",
        },
        &message,
    )
    .await
    .expect("Matrix delivery");
    let captured = captures.lock().expect("captures");
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].method, Method::POST);
    assert!(
        captured[0]
            .uri
            .starts_with("/matrix/_matrix/media/v3/upload?filename=")
    );
    assert_eq!(
        captured[0].authorization.as_deref(),
        Some("Bearer matrix-secret")
    );
    assert_eq!(captured[0].content_type.as_deref(), Some("audio/wav"));
    assert_eq!(captured[0].body, Bytes::from_static(b"RIFF-wave"));
    assert_eq!(captured[1].method, Method::PUT);
    assert!(captured[1].uri.contains("/matrix/_matrix/client/v3/rooms/"));
    assert!(captured[1].uri.contains("/send/m.room.message/"));
    let event: serde_json::Value = serde_json::from_slice(&captured[1].body).expect("event JSON");
    assert_eq!(event["msgtype"], "m.audio");
    assert_eq!(event["url"], "mxc://matrix.test/audio7");
    assert_eq!(event["info"]["duration"], 1_200);
    assert_eq!(
        event["filename"],
        "20260815T100000Z_DMR_451.125MHz_TG91.wav"
    );
    assert_eq!(event["format"], MATRIX_HTML_FORMAT);
    assert!(
        event["formatted_body"]
            .as_str()
            .is_some_and(|html| html.starts_with("<strong>DMR call</strong>"))
    );
    assert!(
        event["body"]
            .as_str()
            .is_some_and(|body| body.contains("talkgroup 91")
                && body != "20260815T100000Z_DMR_451.125MHz_TG91.wav"),
        "body must differ from filename so clients treat it as a caption"
    );
}

#[tokio::test]
async fn matrix_sends_one_text_event_without_audio() {
    let (base, captures) = server().await;
    let homeserver_url = format!("{base}/matrix");
    let message = message("chat", &decoded());
    send_matrix(
        &Client::new(),
        MatrixTarget {
            homeserver_url: &homeserver_url,
            room_id: "!radio:matrix.test",
            access_token: "matrix-secret",
        },
        &message,
    )
    .await
    .expect("Matrix delivery");
    let captured = captures.lock().expect("captures");
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].method, Method::PUT);
    assert!(captured[0].uri.contains("/matrix/_matrix/client/v3/rooms/"));
    assert_eq!(
        captured[0].authorization.as_deref(),
        Some("Bearer matrix-secret")
    );
    let event: serde_json::Value = serde_json::from_slice(&captured[0].body).expect("event JSON");
    assert_eq!(event["msgtype"], "m.text");
    assert!(
        event["body"]
            .as_str()
            .is_some_and(|body| body.contains("RTTY decode"))
    );
    assert!(event.get("url").is_none());
}

#[test]
fn oversized_decoder_summaries_are_bounded_and_surface_truncation() {
    let mut record = decoded();
    record.event = DecoderEvent::of(&RttyText {
        text: "x".repeat(MAX_MESSAGE_CHARS * 2),
    });

    let message = message("chat", &record);

    assert_eq!(message.body.chars().count(), MAX_MESSAGE_CHARS);
    assert!(message.body.ends_with("… [truncated]"));
}

#[tokio::test]
async fn error_response_body_is_bounded_before_formatting() {
    let (base, _) = server().await;
    let response = Client::new()
        .get(format!("{base}/oversized-error"))
        .send()
        .await
        .expect("error response");

    let error = checked_response("Test", response)
        .await
        .expect_err("non-success response")
        .to_string();
    let (_, body) = error.split_once(": ").expect("error body");

    assert_eq!(body.len(), MAX_ERROR_BODY);
    assert!(body.bytes().all(|byte| byte == b'x'));
}

#[tokio::test]
async fn a_json_webhook_posts_the_structured_event_without_the_wav() {
    let (base, captures) = server().await;
    let message = clip_message("chat", Bytes::from_static(b"RIFF-wave"));

    send_webhook(
        &Client::new(),
        &format!("{base}/hooks/radio"),
        WebhookFormat::Json,
        &message,
    )
    .await
    .expect("webhook delivery");

    let captured = captures.lock().expect("captures");
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].method, Method::POST);
    assert_eq!(captured[0].uri, "/hooks/radio");
    assert!(
        captured[0]
            .content_type
            .as_deref()
            .is_some_and(|value| value.starts_with("application/json"))
    );
    let body: serde_json::Value = serde_json::from_slice(&captured[0].body).expect("webhook JSON");
    assert_eq!(body["output"], "chat");
    assert_eq!(body["kind"], "call");
    assert!(
        body["text"]
            .as_str()
            .is_some_and(|text| text.contains("talkgroup 91"))
    );
    assert_eq!(body["record"]["at"], "2026-08-15T10:00:02Z");
    assert_eq!(body["record"]["event"]["kind"], "call");
    assert_eq!(body["record"]["event"]["data"]["id"], 7);
    assert!(
        !String::from_utf8_lossy(&captured[0].body).contains("RIFF-wave"),
        "audio rides the Discord format only"
    );
}

#[tokio::test]
async fn a_json_webhook_carries_the_decoded_payload() {
    let (base, captures) = server().await;
    let message = message("chat", &decoded());

    send_webhook(
        &Client::new(),
        &format!("{base}/hooks/radio"),
        WebhookFormat::Json,
        &message,
    )
    .await
    .expect("webhook delivery");

    let captured = captures.lock().expect("captures");
    let body: serde_json::Value = serde_json::from_slice(&captured[0].body).expect("webhook JSON");
    assert_eq!(body["kind"], "rtty");
    assert_eq!(body["record"]["event"]["data"]["text"], "CQ TEST");
    assert!(
        body.get("content").is_none(),
        "the generic body is not a Discord payload"
    );
}

#[tokio::test]
async fn a_json_webhook_error_does_not_expose_the_endpoint() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener");
    let address = listener.local_addr().expect("local address");
    drop(listener);
    let secret = "webhook-secret";
    let message = message("chat", &decoded());

    let error = send_webhook(
        &Client::new(),
        &format!("http://{address}/hooks/{secret}"),
        WebhookFormat::Json,
        &message,
    )
    .await
    .expect_err("connection failure")
    .to_string();

    assert!(error.starts_with("Webhook request:"));
    assert!(!error.contains(secret));
}

fn json_webhook(account: &str) -> EventOutputTarget {
    EventOutputTarget::Webhook {
        account: account.to_owned(),
        format: WebhookFormat::Json,
    }
}

fn webhook_delivery(node: &str, url: String) -> Delivery {
    Delivery {
        login: Ok(Arc::new(Login::Webhook { url })),
        ..delivery_to(node, json_webhook(node), &decoded())
    }
}

#[tokio::test]
async fn delivery_outcomes_reach_the_output_status() {
    let (base, _) = server().await;
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener");
    let address = closed.local_addr().expect("local address");
    drop(closed);
    let statuses = status::Statuses::default();
    statuses.configure(&[
        binding("good", json_webhook("good")),
        binding("bad", json_webhook("bad")),
    ]);
    let (sender, receiver) = mpsc::channel(DELIVERY_QUEUE);
    sender
        .send(webhook_delivery("good", format!("{base}/hook")))
        .await
        .expect("queued");
    sender
        .send(webhook_delivery("bad", format!("http://{address}/hook")))
        .await
        .expect("queued");
    drop(sender);

    deliver_all(senders(None), receiver, statuses.clone()).await;

    let good = statuses.get("good").expect("good status");
    assert_eq!((good.delivered, good.failed, good.error), (1, 0, None));
    let bad = statuses.get("bad").expect("bad status");
    assert_eq!((bad.delivered, bad.failed), (0, 1));
    assert!(bad.error.expect("failure").starts_with("Webhook request:"));
}

fn senders(shell: Option<Arc<dyn NativeShell>>) -> Senders {
    Senders {
        client: Client::new(),
        engine: std::sync::Weak::new(),
        shell,
    }
}

fn account(store: &crate::Store, name: &str, service: AccountService) -> String {
    store
        .create_account(sdrmm_wire::Account {
            id: String::new(),
            name: name.to_owned(),
            service,
        })
        .expect("account")
        .id
}

fn influx_account(store: &crate::Store, url: String) -> String {
    account(
        store,
        "Lab",
        AccountService::Influx {
            url,
            org: String::new(),
            token: Secret::Inline("kept-token".to_owned()),
        },
    )
}

fn influx_on(account: String) -> EventOutputTarget {
    EventOutputTarget::Influx {
        account,
        bucket: "radio".to_owned(),
    }
}

async fn problem_of(store: &Arc<crate::Store>, target: EventOutputTarget) -> Option<String> {
    let statuses = status::Statuses::default();
    let mut bindings = [binding("out", target)];
    statuses.configure(&bindings);
    resolve_logins(store, &mut bindings, &statuses).await;
    statuses.get("out").and_then(|status| status.error)
}

#[tokio::test]
async fn a_delivery_signs_in_with_its_accounts_credential() {
    let (base, captures) = server().await;
    let store = Arc::new(crate::Store::open(None).expect("store"));
    let target = influx_on(influx_account(&store, format!("{base}/influx")));
    let statuses = status::Statuses::default();
    let mut bindings = [binding("influx", target)];
    statuses.configure(&bindings);
    resolve_logins(&store, &mut bindings, &statuses).await;
    let [bound] = bindings;
    let delivery = Delivery {
        login: bound.login,
        ..delivery_to("influx", bound.target, &decoded())
    };
    let (sender, receiver) = mpsc::channel(DELIVERY_QUEUE);
    sender.send(delivery).await.expect("queued");
    drop(sender);

    deliver_all(senders(None), receiver, statuses.clone()).await;

    assert_eq!(statuses.get("influx").expect("status").delivered, 1);
    let captured = captures.lock().expect("captures");
    assert_eq!(
        captured[0].authorization.as_deref(),
        Some("Token kept-token")
    );
}

#[tokio::test]
async fn a_missing_account_shows_until_one_is_picked() {
    let store = Arc::new(crate::Store::open(None).expect("store"));

    assert_eq!(
        problem_of(&store, influx_on("gone".to_owned()))
            .await
            .as_deref(),
        Some("InfluxDB account is gone")
    );

    let id = influx_account(&store, "http://127.0.0.1:9".to_owned());
    assert_eq!(problem_of(&store, influx_on(id)).await, None);
}

#[tokio::test]
async fn an_account_of_another_service_is_refused() {
    let store = Arc::new(crate::Store::open(None).expect("store"));
    let hook = account(
        &store,
        "Hook",
        AccountService::Webhook {
            url: Secret::Inline("https://hooks.example/a".to_owned()),
        },
    );

    assert_eq!(
        problem_of(&store, influx_on(hook)).await.as_deref(),
        Some("Hook is a Webhook account, not InfluxDB")
    );
}

fn matrix_bot(login: MatrixLogin, access_token: Secret) -> AccountService {
    AccountService::Matrix {
        homeserver_url: "https://matrix.example".to_owned(),
        user: "@bot:matrix.example".to_owned(),
        login,
        access_token,
        password: Secret::default(),
        nickname: String::new(),
        recovery_key: Secret::default(),
        session: Secret::default(),
        store_key: Secret::default(),
    }
}

fn matrix_room(account: String) -> EventOutputTarget {
    EventOutputTarget::Matrix {
        account,
        room_id: "!radio:matrix.example".to_owned(),
    }
}

#[tokio::test]
async fn a_matrix_account_without_its_token_cannot_send() {
    let store = Arc::new(crate::Store::open(None).expect("store"));
    let room = account(
        &store,
        "Room",
        matrix_bot(MatrixLogin::Token, Secret::Inline("tok".to_owned())),
    );
    store
        .hold()
        .execute("UPDATE secrets SET value = ''", [])
        .expect("emptied");
    assert_eq!(
        problem_of(&store, matrix_room(room)).await.as_deref(),
        Some("Matrix token is not set")
    );
}

#[tokio::test]
async fn a_voice_matrix_account_cannot_send_events() {
    let store = Arc::new(crate::Store::open(None).expect("store"));
    for login in [MatrixLogin::Password, MatrixLogin::Oauth] {
        let mut service = matrix_bot(login, Secret::default());
        if let AccountService::Matrix { password, .. } = &mut service {
            *password = Secret::Inline("pw".to_owned());
        }
        let bot = account(&store, "Bot", service);
        store
            .set_account_session(
                &bot,
                r#"{"kind":"oauth","client_id":"c","user_id":"@bot:matrix.example","device_id":"D","access_token":"session-token"}"#,
            )
            .expect("signed in");
        assert_eq!(
            problem_of(&store, matrix_room(bot)).await.as_deref(),
            Some("Event output needs a token login")
        );
    }
}

#[tokio::test]
async fn a_stored_webhook_url_still_has_to_be_https() {
    let store = Arc::new(crate::Store::open(None).expect("store"));
    let hook = account(
        &store,
        "Hook",
        AccountService::Webhook {
            url: Secret::Inline("https://hooks.example/a".to_owned()),
        },
    );
    store
        .hold()
        .execute("UPDATE secrets SET value = 'http://plain.example/hook'", [])
        .expect("downgraded");

    assert_eq!(
        problem_of(&store, json_webhook(&hook)).await.as_deref(),
        Some("Webhook URL is not valid")
    );
}

#[tokio::test]
async fn a_login_that_does_not_fit_its_output_fails_the_delivery() {
    let statuses = status::Statuses::default();
    statuses.configure(&[binding("hook", json_webhook("hook"))]);
    let (sender, receiver) = mpsc::channel(DELIVERY_QUEUE);
    let mismatched = Delivery {
        login: Ok(Arc::new(Login::None)),
        ..delivery_to("hook", json_webhook("hook"), &decoded())
    };
    sender.send(mismatched).await.expect("queued");
    drop(sender);

    deliver_all(senders(None), receiver, statuses.clone()).await;

    let status = statuses.get("hook").expect("status");
    assert_eq!(status.failed, 1);
    assert_eq!(
        status.error.as_deref(),
        Some("The account does not fit this output")
    );
}

#[derive(Debug, Default)]
struct Notifications(Mutex<Vec<(String, String)>>);

impl NativeShell for Notifications {
    fn reveal(&self, _: &std::path::Path) -> std::io::Result<()> {
        Ok(())
    }

    fn notify(&self, title: &str, body: &str) -> std::io::Result<()> {
        self.0
            .lock()
            .expect("notifications")
            .push((title.to_owned(), body.to_owned()));
        Ok(())
    }
}

async fn notify_once(shell: Option<Arc<dyn NativeShell>>) -> status::Statuses {
    let statuses = status::Statuses::default();
    statuses.configure(&[binding("desk", EventOutputTarget::Desktop)]);
    let (sender, receiver) = mpsc::channel(DELIVERY_QUEUE);
    sender
        .send(delivery_to("desk", EventOutputTarget::Desktop, &decoded()))
        .await
        .expect("queued");
    drop(sender);
    deliver_all(senders(shell), receiver, statuses.clone()).await;
    statuses
}

#[tokio::test]
async fn a_desktop_output_shows_one_notification_per_event() {
    let shell = Arc::new(Notifications::default());
    let statuses = notify_once(Some(shell.clone())).await;

    assert_eq!(statuses.get("desk").expect("status").delivered, 1);
    let shown = shell.0.lock().expect("notifications").clone();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].0, "RTTY decode");
    assert!(shown[0].1.contains("Frequency:"), "{}", shown[0].1);
}

#[tokio::test]
async fn a_desktop_output_without_the_desktop_app_reports_why() {
    let status = notify_once(None).await.get("desk").expect("status");

    assert_eq!(status.failed, 1);
    assert_eq!(
        status.error.as_deref(),
        Some("Notifications need the desktop app")
    );
}

#[test]
fn a_call_notification_is_titled_by_its_mode() {
    assert_eq!(
        notification_text("DMR call · talkgroup 91 · 2.0 s"),
        ("DMR call", "talkgroup 91 · 2.0 s")
    );
    assert_eq!(notification_text("plain"), ("plain", ""));
}

#[test]
fn a_full_delivery_queue_counts_the_dropped_event() {
    let target = json_webhook("hook");
    let statuses = status::Statuses::default();
    statuses.configure(&[binding("hook", target.clone())]);
    let (sender, _receiver) = mpsc::channel(1);
    enqueue(
        &sender,
        &statuses,
        delivery_to("hook", target.clone(), &decoded()),
    );
    enqueue(&sender, &statuses, delivery_to("hook", target, &decoded()));

    let status = statuses.get("hook").expect("status");
    assert_eq!(status.failed, 1);
    assert_eq!(status.error.as_deref(), Some("Delivery queue full"));
}

#[test]
fn a_changed_target_starts_a_fresh_status() {
    let statuses = status::Statuses::default();
    let bound = |account: &str| binding("hook", json_webhook(account));
    statuses.configure(&[bound("a")]);
    statuses.failed("hook", 2, "down".to_owned());
    statuses.configure(&[bound("a")]);
    assert_eq!(statuses.get("hook").expect("kept").failed, 2);
    statuses.configure(&[bound("b")]);
    assert_eq!(statuses.get("hook").expect("reset").failed, 0);
    statuses.configure(&[]);
    assert!(statuses.get("hook").is_none());
}

struct Published {
    topic: String,
    qos: u8,
    payload: Vec<u8>,
}

#[derive(Default)]
struct BrokerLog {
    client_id: String,
    username: Option<String>,
    password: Option<String>,
    published: Vec<Published>,
}

type Broker = Arc<Mutex<BrokerLog>>;

async fn broker() -> (String, Broker) {
    let log: Broker = Arc::new(Mutex::new(BrokerLog::default()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("broker listener");
    let address = listener.local_addr().expect("broker address");
    let served = log.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(serve_broker(stream, served.clone()));
        }
    });
    (format!("mqtt://{address}"), log)
}

async fn serve_broker(mut stream: tokio::net::TcpStream, log: Broker) {
    while let Some((header, body)) = read_packet(&mut stream).await {
        let reply = match header >> 4 {
            1 => {
                record_connect(&log, &body);
                vec![0x20, 0x02, 0x00, 0x00]
            }
            3 => match record_publish(&log, header, &body) {
                Some(id) => vec![0x40, 0x02, (id >> 8) as u8, id as u8],
                None => return,
            },
            12 => vec![0xD0, 0x00],
            _ => return,
        };
        if stream.write_all(&reply).await.is_err() {
            return;
        }
    }
}

async fn read_packet(stream: &mut tokio::net::TcpStream) -> Option<(u8, Vec<u8>)> {
    let mut header = [0_u8; 1];
    stream.read_exact(&mut header).await.ok()?;
    let mut length = 0_usize;
    let mut shift = 0_u32;
    loop {
        let mut byte = [0_u8; 1];
        stream.read_exact(&mut byte).await.ok()?;
        length |= usize::from(byte[0] & 0x7F) << shift;
        if byte[0] & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 21 {
            return None;
        }
    }
    let mut body = vec![0_u8; length];
    stream.read_exact(&mut body).await.ok()?;
    Some((header[0], body))
}

fn take_string(body: &[u8], at: &mut usize) -> Option<String> {
    let length = usize::from(u16::from_be_bytes([*body.get(*at)?, *body.get(*at + 1)?]));
    let start = *at + 2;
    let end = start.checked_add(length)?;
    let text = String::from_utf8(body.get(start..end)?.to_vec()).ok()?;
    *at = end;
    Some(text)
}

fn record_connect(log: &Broker, body: &[u8]) {
    let mut at = 0;
    let Some(_protocol) = take_string(body, &mut at) else {
        return;
    };
    let Some(&flags) = body.get(at + 1) else {
        return;
    };
    at += 4;
    let Some(client_id) = take_string(body, &mut at) else {
        return;
    };
    if flags & 0x04 != 0 {
        take_string(body, &mut at);
        take_string(body, &mut at);
    }
    let username = (flags & 0x80 != 0)
        .then(|| take_string(body, &mut at))
        .flatten();
    let password = (flags & 0x40 != 0)
        .then(|| take_string(body, &mut at))
        .flatten();
    let mut log = log.lock().expect("broker log");
    log.client_id = client_id;
    log.username = username;
    log.password = password;
}

fn record_publish(log: &Broker, header: u8, body: &[u8]) -> Option<u16> {
    let mut at = 0;
    let topic = take_string(body, &mut at)?;
    let qos = (header >> 1) & 0x03;
    let id = u16::from_be_bytes([*body.get(at)?, *body.get(at + 1)?]);
    at += 2;
    log.lock().expect("broker log").published.push(Published {
        topic,
        qos,
        payload: body.get(at..)?.to_vec(),
    });
    Some(id)
}

#[tokio::test]
async fn mqtt_publishes_one_acknowledged_event_per_delivery() {
    let (broker_url, log) = broker().await;
    let message = message("chat", &decoded());

    send_mqtt(
        MqttTarget {
            broker_url: &broker_url,
            topic: "sdrmm/events",
            username: "radio",
            password: "broker-secret",
            client_id: "sdrmm-chat",
        },
        &message,
    )
    .await
    .expect("MQTT delivery");

    let log = log.lock().expect("broker log");
    assert_eq!(log.client_id, "sdrmm-chat");
    assert_eq!(log.username.as_deref(), Some("radio"));
    assert_eq!(log.password.as_deref(), Some("broker-secret"));
    assert_eq!(log.published.len(), 1);
    assert_eq!(log.published[0].topic, "sdrmm/events");
    assert_eq!(
        log.published[0].qos, 1,
        "an unacknowledged publish could be lost"
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&log.published[0].payload).expect("MQTT JSON");
    assert_eq!(payload["kind"], "rtty");
    assert_eq!(payload["record"]["event"]["data"]["text"], "CQ TEST");
}

#[tokio::test]
async fn mqtt_connects_anonymously_when_no_username_is_set() {
    let (broker_url, log) = broker().await;
    let message = message("chat", &decoded());

    send_mqtt(
        MqttTarget {
            broker_url: &broker_url,
            topic: "sdrmm/events",
            username: "",
            password: "",
            client_id: "sdrmm-chat",
        },
        &message,
    )
    .await
    .expect("MQTT delivery");

    let log = log.lock().expect("broker log");
    assert_eq!(log.username, None);
    assert_eq!(log.password, None);
}

#[tokio::test]
async fn an_unreachable_broker_surfaces_the_failure() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener");
    let address = listener.local_addr().expect("local address");
    drop(listener);
    let message = message("chat", &decoded());

    let error = send_mqtt(
        MqttTarget {
            broker_url: &format!("mqtt://{address}"),
            topic: "sdrmm/events",
            username: "",
            password: "",
            client_id: "sdrmm-chat",
        },
        &message,
    )
    .await
    .expect_err("connection failure")
    .to_string();

    assert!(error.starts_with("MQTT broker:"), "{error}");
}

#[test]
fn the_broker_url_takes_the_default_port_for_its_scheme() {
    let target = |broker_url| MqttTarget {
        broker_url,
        topic: "sdrmm/events",
        username: "",
        password: "",
        client_id: "sdrmm-chat",
    };
    for (broker_url, port, secure) in [
        ("mqtt://broker.example", MQTT_PORT, false),
        ("mqtts://broker.example", MQTTS_PORT, true),
        ("mqtt://broker.example:1884", 1_884, false),
    ] {
        assert_eq!(
            mqtt_broker(broker_url).expect("broker address"),
            ("broker.example".to_owned(), port, secure),
            "{broker_url}"
        );
    }
    assert!(mqtt_broker("https://broker.example").is_err());

    let options = mqtt_options(&target("mqtt://broker.example:1884")).expect("broker options");
    assert_eq!(
        options.broker().tcp_address(),
        Some(("broker.example", 1_884))
    );
}

#[test]
fn the_mqtt_client_id_stays_inside_the_protocol_limit() {
    assert_eq!(mqtt_client_id("chat"), "sdrmm-chat");
    assert_eq!(
        mqtt_client_id("an-output-node-with-a-very-long-identifier").len(),
        MQTT_CLIENT_ID_LEN
    );
}

#[test]
fn spectrum_monitor_audio_is_attached_to_event_deliveries() {
    let calls = Calls::default();
    let (audio, evicted) = calls.store_clip(&[1, -2, 3]);
    assert!(!evicted);
    let record = DecodedRecord {
        origin: Some(sdrmm_wire::EventOrigin {
            node: "monitor".to_owned(),
            transmission: 7,
        }),
        event: DecoderEvent::of(&sdrmm_wire::Transmission {
            id: 7,
            state: sdrmm_wire::TransmissionState::Completed,
            signal: Default::default(),
            start_sample: 0,
            end_sample: 48000,
            sample_rate_hz: 48000.0,
            duration_ms: 1000,
            started_at: None,
            ended_at: None,
            decoder: Some("am".to_owned()),
            decoder_confirmed: false,
            audio: Some(audio.clone()),
            error: None,
            transcript: None,
        }),
        ..decoded()
    };
    let mut plain = routing_for();
    plain.bindings[0].target = json_webhook("hook");
    let unkept = decoded_deliveries(&plain, &record, 1, &calls);
    assert!(
        unkept[0].message.audio.is_none(),
        "a JSON hook keeps no clip"
    );
    let deliveries = decoded_deliveries(&routing_for(), &record, 1, &calls);
    assert_eq!(deliveries.len(), 1);
    let attachment = deliveries[0].message.audio.as_ref().unwrap();
    assert_eq!(&attachment.bytes[..4], b"RIFF");
    assert_eq!(&attachment.bytes[44..], &[1, 0, 254, 255, 3, 0]);
    assert_eq!(calls.event_audio(&audio).unwrap(), attachment.bytes);
    assert_eq!(attachment.stem, "20260815T100002Z_AM_14.080MHz");
    assert_eq!(
        deliveries[0].message.payload["record"]["origin"]["node"],
        "monitor"
    );
}

#[test]
fn beast_listener_requires_an_event_wire_and_explicit_enable() {
    let store = crate::Store::open(None).unwrap();
    let active = store.active_workspace().unwrap().unwrap();
    let beast = |enabled| {
        NodeBody::EventOutput(EventOutputNode {
            message: String::new(),
            target: EventOutputTarget::Feed {
                format: "beast".to_owned(),
                address: "127.0.0.1:30005".to_owned(),
                enabled,
            },
        })
    };
    let graph = PatchGraph {
        nodes: vec![
            node(
                "decoder",
                NodeBody::Channel(ChannelNode {
                    channel_type: "adsb".to_owned(),
                    tuning_locked: false,
                    audio: Default::default(),
                }),
            ),
            node("wired", beast(true)),
            node("unwired", beast(true)),
            node("disabled", beast(false)),
        ],
        edges: vec![
            edge(("decoder", "events"), ("wired", "events")),
            edge(("decoder", "events"), ("disabled", "events")),
        ],
    };
    store
        .update_workspace(
            active.info.id,
            &UpdateWorkspaceRequest {
                revision: active.info.revision,
                name: None,
                snapshot: Some(WorkspaceSnapshot::new(graph, RackLayout::default())),
            },
            None,
        )
        .unwrap();
    let routing = routing_over(&store);
    assert_eq!(routing.bindings.len(), 1);
    assert_eq!(routing.bindings[0].node, "wired");
}

fn influx_target() -> EventOutputTarget {
    influx_on("db".to_owned())
}

fn postgres_target(table: &str) -> EventOutputTarget {
    EventOutputTarget::Postgres {
        account: "db".to_owned(),
        table: table.to_owned(),
    }
}

fn postgres_settings<'a>(url: &'a str, table: &'a str) -> postgres::PostgresTarget<'a> {
    postgres::PostgresTarget {
        url,
        table,
        username: "radio",
        password: "secret",
    }
}

fn delivery_to(node: &str, target: EventOutputTarget, record: &DecodedRecord) -> Delivery {
    Delivery {
        node: node.to_owned(),
        target,
        login: Ok(Arc::new(Login::None)),
        event: "test".to_owned(),
        message: message(node, record),
    }
}

#[test]
fn an_influx_point_carries_tags_event_fields_and_the_receive_time() {
    let message = message("influx", &decoded());

    let line = influx::line("influx", &message.facts);

    assert_eq!(
        line,
        "rtty,output=influx,device_set=1,channel=2 \
         freq_hz=14080000.0,summary=\"CQ TEST\",text=\"CQ TEST\" \
         1786788002000000000"
    );
}

#[test]
fn an_influx_point_escapes_names_and_strings() {
    let record = DecodedRecord {
        event: DecoderEvent::of(&RttyText {
            text: "say \"hi\"\\\nnow".to_owned(),
        }),
        ..decoded()
    };
    let message = message("a b,c=d", &record);

    let line = influx::line("a b,c=d", &message.facts);

    assert!(line.starts_with("rtty,output=a\\ b\\,c\\=d,"), "{line}");
    assert!(line.contains(r#"text="say \"hi\"\\ now""#), "{line}");
}

#[tokio::test]
async fn influx_writes_a_batch_in_one_authenticated_request() {
    let (base, captures) = server().await;
    let target = influx::InfluxTarget {
        url: &format!("{base}/influx"),
        bucket: "radio",
        org: "home",
        token: "influx-token",
    };
    let batch = [
        delivery_to("influx", influx_target(), &decoded()),
        delivery_to("influx", influx_target(), &decoded()),
    ];

    influx::send(&Client::new(), target, &batch)
        .await
        .expect("influx write");

    let captured = captures.lock().expect("captures");
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].method, Method::POST);
    assert_eq!(
        captured[0].uri,
        "/influx/api/v2/write?bucket=radio&org=home&precision=ns"
    );
    assert_eq!(
        captured[0].authorization.as_deref(),
        Some("Token influx-token")
    );
    let body = String::from_utf8_lossy(&captured[0].body);
    assert_eq!(body.lines().count(), 2);
}

#[tokio::test]
async fn an_influx_rejection_surfaces_the_server_reply() {
    let (base, _) = server().await;
    let target = influx::InfluxTarget {
        url: &format!("{base}/oversized-error"),
        bucket: "radio",
        org: "",
        token: "",
    };
    let batch = [delivery_to("influx", influx_target(), &decoded())];

    let error = influx::send(&Client::new(), target, &batch)
        .await
        .expect_err("rejected write")
        .to_string();

    assert!(error.starts_with("InfluxDB returned"), "{error}");
}

#[test]
fn only_consecutive_database_deliveries_to_one_output_share_a_batch() {
    let database = delivery_to("influx", influx_target(), &decoded());
    let same = delivery_to("influx", influx_target(), &decoded());
    let other_node = delivery_to("other", influx_target(), &decoded());
    let webhook = delivery_to("hook", discord_webhook(), &decoded());
    let webhook_again = delivery_to("hook", discord_webhook(), &decoded());

    assert!(same_batch(&database, &same));
    assert!(!same_batch(&database, &other_node));
    assert!(!same_batch(&webhook, &webhook_again));
}

#[test]
fn postgres_rows_hold_the_event_columns_and_full_record() {
    let batch = [delivery_to(
        "archive",
        postgres_target("sdrmm_events"),
        &decoded(),
    )];

    let columns = postgres::columns(&batch);

    assert_eq!(columns.at, ["2026-08-15T10:00:02Z"]);
    assert_eq!(columns.output, ["archive"]);
    assert_eq!(columns.kind, ["rtty"]);
    assert_eq!(columns.device_set, [1]);
    assert_eq!(columns.channel, [2]);
    assert_eq!(columns.freq_hz, [14_080_000.0]);
    assert_eq!(columns.summary, ["CQ TEST"]);
    let record: serde_json::Value = serde_json::from_str(&columns.record[0]).expect("record");
    assert_eq!(record["event"]["data"]["text"], "CQ TEST");
}

#[test]
fn postgres_statements_name_the_configured_table() {
    let create = postgres::create_table("radio_log");
    assert!(create.contains("CREATE TABLE IF NOT EXISTS radio_log ("));
    assert!(create.contains("ON radio_log (kind, at)"));
    assert!(postgres::insert_statement("radio_log").starts_with("INSERT INTO radio_log "));
}

#[test]
fn postgres_takes_credentials_beside_the_url() {
    let config = postgres::config(&postgres::PostgresTarget {
        url: "postgres://db.example:5433/radio?sslmode=require",
        table: "sdrmm_events",
        username: "radio",
        password: "secret",
    })
    .expect("config");

    assert_eq!(config.get_user(), Some("radio"));
    assert_eq!(config.get_password(), Some(&b"secret"[..]));
    assert_eq!(config.get_dbname(), Some("radio"));
    assert_eq!(config.get_ports(), [5433]);
}

#[test]
fn a_remote_postgres_must_speak_tls_unless_the_url_says_otherwise() {
    use tokio_postgres::config::SslMode;
    let mode = |url| {
        postgres::config(&postgres::PostgresTarget {
            url,
            table: "sdrmm_events",
            username: "radio",
            password: "secret",
        })
        .expect("config")
        .get_ssl_mode()
    };
    assert_eq!(mode("postgres://db.example/radio"), SslMode::Require);
    assert_eq!(
        mode("postgres://localhost,db.example/radio"),
        SslMode::Require
    );
    assert_eq!(
        mode("postgres://localhost/radio?hostaddr=192.0.2.7"),
        SslMode::Require
    );
    assert_eq!(mode("postgres://127.0.0.1/radio"), SslMode::Prefer);
    assert_eq!(mode("postgres://localhost/radio"), SslMode::Prefer);
    assert_eq!(
        mode("postgres://db.example/radio?sslmode=disable"),
        SslMode::Disable
    );
    assert_eq!(mode("host=db.example sslmode=prefer"), SslMode::Prefer);
}

#[tokio::test]
async fn postgres_refuses_an_unsafe_table_before_connecting() {
    let batch = [delivery_to(
        "archive",
        postgres_target("events; drop"),
        &decoded(),
    )];
    let settings = postgres_settings("postgres://127.0.0.1:1/radio", "events; drop");

    let error = postgres::Connections::default()
        .insert("archive", &settings, &batch)
        .await
        .expect_err("unsafe table")
        .to_string();

    assert!(error.contains("not a plain lowercase name"), "{error}");
}

#[tokio::test]
async fn an_unreachable_postgres_surfaces_the_failure() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener");
    let address = listener.local_addr().expect("local address");
    drop(listener);
    let url = format!("postgres://{address}/radio?sslmode=disable");
    let batch = [delivery_to(
        "archive",
        postgres_target("sdrmm_events"),
        &decoded(),
    )];

    let error = postgres::Connections::default()
        .insert("archive", &postgres_settings(&url, "sdrmm_events"), &batch)
        .await
        .expect_err("connection failure")
        .to_string();

    assert!(error.starts_with("Postgres connect:"), "{error}");
}

#[test]
fn a_csv_row_quotes_text_and_defuses_formulas() {
    let record = DecodedRecord {
        event: DecoderEvent::of(&RttyText {
            text: "=HYPERLINK(\"x\"), hi".to_owned(),
        }),
        ..decoded()
    };
    let message = message("log", &record);

    let row = csv::row("log", &message.facts);

    assert_eq!(
        row,
        "2026-08-15T10:00:02Z,rtty,log,1,2,14080000,,\"'=HYPERLINK(\"\"x\"\"), hi\",\
         \"{\"\"text\"\":\"\"=HYPERLINK(\\\"\"x\\\"\"), hi\"\"}\"\r\n"
    );
}

#[tokio::test]
async fn csv_appends_rows_under_one_header() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let engine = Engine::with_registry(
        sdrmm_device::DeviceRegistry::new(),
        Some(dir.path().to_path_buf()),
    );
    let target = EventOutputTarget::Csv {
        file: "log".to_owned(),
    };
    let batch = [
        delivery_to("log", target.clone(), &decoded()),
        delivery_to("log", target, &decoded()),
    ];
    let weak = Arc::downgrade(&engine);

    file::append(&weak, file::Format::Csv, "log", &batch)
        .await
        .expect("first");
    file::append(&weak, file::Format::Csv, "log", &batch[..1])
        .await
        .expect("second");

    let text = std::fs::read_to_string(file::events_dir(dir.path()).join("log.csv")).expect("read");
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len(), 4);
    assert!(lines[0].starts_with("at,kind,output"));
    assert_eq!(
        lines.iter().filter(|line| line.starts_with("at,")).count(),
        1
    );
}

#[tokio::test]
async fn csv_refuses_a_path_and_a_missing_library() {
    let engine = Engine::with_registry(sdrmm_device::DeviceRegistry::new(), None);
    let weak = Arc::downgrade(&engine);
    let batch = [delivery_to(
        "log",
        EventOutputTarget::Csv {
            file: "log".to_owned(),
        },
        &decoded(),
    )];

    let escape = file::append(&weak, file::Format::Csv, "../log", &batch)
        .await
        .expect_err("path");
    let missing = file::append(&weak, file::Format::Csv, "log", &batch)
        .await
        .expect_err("no library");

    assert!(escape.to_string().contains("plain name"), "{escape}");
    assert!(missing.to_string().contains("recordings"), "{missing}");
}

#[tokio::test]
async fn text_appends_one_line_per_event_without_a_header() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let engine = Engine::with_registry(
        sdrmm_device::DeviceRegistry::new(),
        Some(dir.path().to_path_buf()),
    );
    let target = EventOutputTarget::Text {
        file: "log".to_owned(),
    };
    let record = DecodedRecord {
        event: DecoderEvent::of(&RttyText {
            text: "two\nlines".to_owned(),
        }),
        ..decoded()
    };
    let batch = [
        delivery_to("log", target.clone(), &record),
        delivery_to("log", target, &record),
    ];
    let weak = Arc::downgrade(&engine);

    file::append(&weak, file::Format::Text, "log", &batch)
        .await
        .expect("append");

    let text = std::fs::read_to_string(file::events_dir(dir.path()).join("log.txt")).expect("read");
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert!(
        lines[0].starts_with("2026-08-15T10:00:02Z  rtty  14.080000 MHz  two lines"),
        "{text}"
    );
}

fn routing_over(store: &crate::Store) -> Routing {
    let hub = crate::resolved::ResolvedHub::default();
    hub.reload(
        store,
        &Engine::new(None),
        &crate::problems::ProblemBoard::default(),
    );
    routing_of(&hub.current())
}

#[test]
fn a_layout_writes_the_message_the_operator_asked_for() {
    let record = decoded();
    let layout = "**{headline}** on {freq} MHz ch {record.channel}{nope}";
    let message = decoded_message("chat", &record, 1, view_of(&record), None, layout);
    assert_eq!(message.body, "**RTTY decode** on 14.08 MHz ch 2");
    assert!(message.html.is_none());
    assert!(message.laid_out);
    assert_eq!(message.payload["text"], json!(message.body));
}

#[test]
fn a_blank_layout_keeps_the_usual_message() {
    let record = decoded();
    let blank = decoded_message("chat", &record, 1, view_of(&record), None, "  ");
    assert_eq!(blank.body, message("chat", &record).body);
    assert!(blank.html.is_some());
    assert!(!blank.laid_out);
}

#[test]
fn a_laid_out_text_line_stays_on_one_line() {
    assert_eq!(text::written("a\nb\tc"), "a b c\n");
}
