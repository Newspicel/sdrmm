use super::*;

#[test]
fn openapi_registers_paths_and_ws_schemas() {
    let spec = openapi().to_pretty_json().expect("serialize");
    for path in [
        "/api/state",
        "/api/status",
        "/api/devices",
        "/api/devices/serial",
        "/api/channeltypes",
        "/api/devices/network",
        "/api/workspaces/active/nodes/{node}/channel",
        "/api/workspaces/active/nodes/{node}/device",
        "/api/workspaces/active/nodes/{node}/actions",
        "/api/presets",
        "/api/presets/{id}",
        "/api/presets/{id}/apply",
        "/api/bookmarks",
        "/api/bookmarks/{id}",
        "/api/audiorecordings",
        "/api/audiorecordings/{file}",
        "/api/audiorecordings/{file}/download",
        "/api/recordings",
        "/api/recordings/{id}",
        "/api/recordings/{id}/download",
        "/api/recordings/{id}/annotation",
        "/api/decoderlog",
        "/api/decoderlog/export/{format}",
        "/api/calls/{id}/audio",
        "/api/workspaces/import",
        "/api/workspaces/{id}/export",
        "/api/workspaces/{id}/apply",
        "/api/workspaces/{id}/undo",
        "/api/workspaces/{id}/redo",
        "/api/workspaces/{id}/notices/{notice}",
        "/api/patch/catalog",
        "/api/patch/wiring",
        "/api/diagnostics",
        "/api/tools",
        "/api/tools/run",
        "/api/images",
        "/api/images/{id}/png",
        "/api/nanovna/library",
        "/api/nanovna/antennas/{id}",
        "/api/nanovna/antennas/{id}/measurements",
        "/api/nanovna/measurements/{id}",
    ] {
        assert!(spec.contains(path), "missing path {path}");
    }
    assert!(
        !spec.contains("/api/devicesets"),
        "nothing mutates a device set by address"
    );
    assert!(spec.contains("ServerEvent"), "ServerEvent schema missing");
    assert!(
        spec.contains("ClientCommand"),
        "ClientCommand schema missing"
    );
    for schema in [
        "DvbtParams",
        "DvbtBandwidth",
        "BroadcastData",
        "ChannelParams",
        "ChannelSettings",
        "PresetSnapshot",
        "RecordingStatus",
        "AudioRecordingStatus",
        "AudioRecordingInfo",
        "RecordingInfo",
        "RecordingAnnotation",
        "NetworkExportStatus",
        "NodeAction",
        "NodeActed",
        "TimeMachineStatus",
        "RecordingInfo",
        "DecoderLogEntry",
        "DecoderLogResponse",
        "DiagnosticsReport",
        "LogLine",
        "LogLevel",
        "ErrorCode",
        "VoiceCall",
        "CapturedImage",
        "CapturedImagesResponse",
        "SstvParams",
        "SstvPicture",
        "DecoderEvent",
        "FlexMessage",
        "ErmesMessage",
        "EotMessage",
        "EotParams",
        "SarsatMessage",
        "SarsatParams",
        "TetrapolMessage",
        "TetrapolParams",
        "CwSkimmerSpot",
        "SelcallSequence",
        "LoraParams",
        "LoraKey",
        "LoraFrame",
        "RemoteIdFrame",
        "RemoteIdParams",
        "BleAdvert",
        "BleParams",
        "WifiOccupancyReport",
        "WifiOccupancyParams",
        "IsmMessage",
        "IsmParams",
        "IsmModulation",
        "IsmCoding",
        "IsmIntegrity",
        "IsmSurveyReport",
        "IsmSurveyParams",
        "NoiseFigureReport",
        "NoiseFigureParams",
        "PerReport",
        "PerParams",
        "SidReport",
        "SidParams",
        "StanagReport",
        "StanagParams",
        "LorawanFrame",
        "MeshtasticPacket",
        "MeshcorePacket",
        "FreeDvParams",
        "DeletedCount",
        "PatchGraph",
        "EventOutputNode",
        "EventOutputTarget",
        "WebhookFormat",
        "RackLayout",
        "DeviceRef",
        "PatchCatalog",
        "PatchApplyReport",
        "PlacementCoverage",
        "WorkspaceExport",
        "WorkspaceState",
        "ToolDescriptor",
        "ToolRequest",
        "ToolResponse",
        "AntennaDesign",
        "AntennaReport",
        "NanoVnaRequest",
        "NanoVnaSweep",
        "NanoVnaDeviceReport",
        "NanoVnaCalibration",
        "NanoVnaCalStep",
    ] {
        assert!(
            spec.contains(&format!("\"{schema}\"")),
            "{schema} schema missing"
        );
    }
    let spec: serde_json::Value = serde_json::from_str(&spec).expect("spec is JSON");
    for params in ["VorParams", "IlsParams"] {
        let report_ms = &spec["components"]["schemas"][params]["properties"]["report_ms"];
        assert_eq!(
            report_ms["minimum"],
            serde_json::json!(sdrmm_wire::MIN_NAVAID_REPORT_MS),
            "{params} report_ms minimum"
        );
        assert_eq!(
            report_ms["maximum"],
            serde_json::json!(sdrmm_wire::MAX_NAVAID_REPORT_MS),
            "{params} report_ms maximum"
        );
    }
}

#[test]
fn router_builds_outside_a_tokio_runtime() {
    let _router = test_router_with_options(&ServerOptions::default());
}

#[test]
fn openapi_matches_the_committed_snapshot() {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("../../../../openapi.json")).expect("snapshot");
    let actual: serde_json::Value =
        serde_json::from_str(&openapi().to_pretty_json().expect("OpenAPI")).expect("OpenAPI");
    assert_eq!(
        actual, expected,
        "regenerate the wire schema with cargo xtask codegen"
    );
}

const NEW_ROUTES: [(&str, &str); 22] = [
    ("GET", "/api/arrays"),
    ("POST", "/api/arrays/{node}/calibrate"),
    ("PATCH", "/api/arrays/{node}/tune"),
    ("POST", "/api/arrays/{node}/recording"),
    ("DELETE", "/api/arrays/{node}/recording"),
    ("GET", "/api/radar/{node}"),
    ("DELETE", "/api/radar/{node}/tracks"),
    ("GET", "/api/phones"),
    ("PUT", "/api/phones/access"),
    ("POST", "/api/phones/offers"),
    ("DELETE", "/api/phones/offers"),
    ("POST", "/api/phones/pair"),
    ("GET", "/api/phones/self"),
    ("DELETE", "/api/phones/self"),
    ("PATCH", "/api/phones/{id}"),
    ("DELETE", "/api/phones/{id}"),
    ("GET", "/api/missions"),
    ("POST", "/api/missions/{node}/actions"),
    ("POST", "/api/missions/workspace"),
    ("GET", "/api/survey/{node}"),
    ("POST", "/api/survey/{node}"),
    ("GET", "/api/fusion/{node}"),
];

#[test]
fn every_new_route_is_in_the_contract() {
    let spec = serde_json::to_value(openapi()).expect("OpenAPI");
    for (method, path) in NEW_ROUTES {
        assert!(
            spec["paths"][path][method.to_lowercase()].is_object(),
            "{method} {path} missing from the contract"
        );
    }
    assert!(spec["paths"]["/api/fusion/{node}"]["delete"].is_object());
    let schemas = &spec["components"]["schemas"];
    for schema in [
        "ArrayStatus",
        "ArrayTuneRequest",
        "RadarUpdate",
        "PhonesResponse",
        "PairingOffer",
        "MissionsResponse",
        "SurveyGrid",
        "ProcessorReading",
        "SurfaceFit",
        "NodeTypeInfo",
    ] {
        assert!(schemas[schema].is_object(), "{schema} schema missing");
    }
}
