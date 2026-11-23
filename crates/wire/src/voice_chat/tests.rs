use super::*;
use crate::{MicrophoneNode, NodeBody, NodeKind, PortDirection, PortType};

#[test]
fn old_vox_microphone_loads_as_voice_activity() {
    let node: MicrophoneNode =
        serde_json::from_str(r#"{"vox":true,"threshold_db":-40.0,"hang_ms":300}"#).expect("old");
    assert_eq!(node.send, SendMode::VoiceActivity);
    assert_eq!(node.gate().threshold_db, -40.0);
    assert_eq!(node.gate().hang_ms, 300);
}

#[test]
fn old_microphone_without_vox_loads_as_push_to_talk() {
    let node: MicrophoneNode = serde_json::from_str(r#"{"vox":false}"#).expect("old");
    assert_eq!(node.send, SendMode::PushToTalk);
    let bare: MicrophoneNode = serde_json::from_str("{}").expect("empty");
    assert_eq!(bare, MicrophoneNode::default());
    assert_eq!(bare.send, SendMode::PushToTalk);
    let json = serde_json::to_value(&node).expect("json");
    assert_eq!(json["send"], "push_to_talk");
    assert!(json.get("vox").is_none());
}

#[test]
fn old_voice_chat_directions_load_under_their_new_names() {
    let node: VoiceChatNode = serde_json::from_str(
        r#"{"to_air":{"mode":"continuous"},"to_chat":{"mode":"voice_activity","volume":0.5}}"#,
    )
    .expect("old");
    assert_eq!(node.from_channel.mode, SendMode::Continuous);
    assert_eq!(node.to_channel.volume, 0.5);
    let json = serde_json::to_value(&node).expect("json");
    assert!(json.get("from_channel").is_some() && json.get("to_air").is_none());
}

#[test]
fn voice_chat_refuses_push_to_talk() {
    let mut node = VoiceChatNode::default();
    assert!(node.valid());
    node.from_channel.mode = SendMode::PushToTalk;
    assert!(!node.valid());
    node.from_channel.mode = SendMode::Continuous;
    node.to_channel.mode = SendMode::PushToTalk;
    assert!(!node.valid());
}

#[test]
fn voice_chat_refuses_what_it_cannot_hold() {
    let long = |len| "x".repeat(len);
    for bad in [
        VoiceChatNode {
            account: long(crate::account::MAX_ACCOUNT_LEN + 1),
            ..VoiceChatNode::default()
        },
        VoiceChatNode {
            channel: long(MAX_CHAT_CHANNEL_LEN + 1),
            ..VoiceChatNode::default()
        },
        VoiceChatNode {
            allowed: vec![String::new(); MAX_CHAT_ALLOWED + 1],
            ..VoiceChatNode::default()
        },
        VoiceChatNode {
            allowed: vec![long(MAX_CHAT_ALLOWED_LEN + 1)],
            ..VoiceChatNode::default()
        },
        VoiceChatNode {
            to_channel: VoiceGateSettings {
                hang_ms: MAX_VOX_HANG_MS + 1,
                ..VoiceGateSettings::default()
            },
            ..VoiceChatNode::default()
        },
        VoiceChatNode {
            from_channel: VoiceGateSettings {
                volume: MAX_VOICE_VOLUME + 0.1,
                ..VoiceGateSettings::default()
            },
            ..VoiceChatNode::default()
        },
        VoiceChatNode {
            to_channel: VoiceGateSettings {
                volume: f64::NAN,
                ..VoiceGateSettings::default()
            },
            ..VoiceChatNode::default()
        },
    ] {
        assert!(!bad.valid(), "{bad:?}");
    }
}

#[test]
fn a_saved_gate_without_volume_loads_at_unity() {
    let gate: VoiceGateSettings =
        serde_json::from_str(r#"{"mode":"continuous","threshold_db":-40.0,"hang_ms":300}"#)
            .expect("old");
    assert_eq!(gate.volume, DEFAULT_VOICE_VOLUME);
    assert!(gate.valid());
}

#[test]
fn voice_chat_with_unknown_account_is_valid() {
    let node = VoiceChatNode {
        account: "gone".to_owned(),
        channel: "room".to_owned(),
        ..VoiceChatNode::default()
    };
    assert!(node.valid());
}

#[test]
fn voice_chat_node_has_audio_in_and_out() {
    let kind = NodeKind::parse("voice_chat").expect("registered");
    assert_eq!(NodeBody::default_for(kind).kind(), kind);
    let ports = kind.ports();
    for direction in [PortDirection::In, PortDirection::Out] {
        assert!(
            ports
                .iter()
                .any(|port| port.direction == direction && port.port_type == PortType::Audio),
            "{direction:?}"
        );
    }
}

#[test]
fn voice_chat_status_round_trips() {
    let status = VoiceChatStatus {
        node: "vc".to_owned(),
        state: VoiceChatState::Joined,
        error: None,
        talking: vec![ChatTalker {
            id: "u1".to_owned(),
            name: "Ann".to_owned(),
            allowed: true,
            verified: true,
        }],
        to_channel_db: Some(-20.0),
        from_channel_db: None,
        to_channel_open: true,
        from_channel_open: false,
    };
    let json = serde_json::to_value(&status).expect("json");
    assert_eq!(json["state"], "joined");
    assert!(json.get("error").is_none());
    let back: VoiceChatStatus = serde_json::from_value(json).expect("back");
    assert_eq!(back, status);
    let event = crate::ServerEvent::VoiceChatStatus(status);
    let json = serde_json::to_value(&event).expect("json");
    assert_eq!(json["type"], "VoiceChatStatus");
    assert_eq!(
        serde_json::from_value::<crate::ServerEvent>(json).expect("back"),
        event
    );
}
