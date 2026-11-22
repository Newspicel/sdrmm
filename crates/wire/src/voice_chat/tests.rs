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
fn voice_chat_refuses_push_to_talk() {
    let mut node = VoiceChatNode::default();
    assert!(node.valid());
    node.to_air.mode = SendMode::PushToTalk;
    assert!(!node.valid());
    node.to_air.mode = SendMode::Continuous;
    node.to_chat.mode = SendMode::PushToTalk;
    assert!(!node.valid());
}

#[test]
fn voice_chat_refuses_what_it_cannot_hold() {
    let long = |len| "x".repeat(len);
    for bad in [
        VoiceChatNode {
            account: long(MAX_CHAT_ACCOUNT_LEN + 1),
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
            to_chat: VoiceGateSettings {
                hang_ms: MAX_VOX_HANG_MS + 1,
                ..VoiceGateSettings::default()
            },
            ..VoiceChatNode::default()
        },
        VoiceChatNode {
            to_air: VoiceGateSettings {
                volume: MAX_VOICE_VOLUME + 0.1,
                ..VoiceGateSettings::default()
            },
            ..VoiceChatNode::default()
        },
        VoiceChatNode {
            to_chat: VoiceGateSettings {
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
fn chat_account_round_trips() {
    for account in [
        ChatAccount {
            id: "a1".to_owned(),
            name: "Club".to_owned(),
            service: ChatService::Discord {
                token: Secret::stored("tok_1"),
            },
        },
        ChatAccount {
            id: "a2".to_owned(),
            name: "Net".to_owned(),
            service: ChatService::Teamspeak {
                address: "ts.example.org".to_owned(),
                password: Secret::default(),
                nickname: "sdr".to_owned(),
                identity: Secret::stored("id_1"),
            },
        },
    ] {
        let json = serde_json::to_value(&account).expect("json");
        assert_eq!(json["id"], account.id);
        let back: ChatAccount = serde_json::from_value(json).expect("back");
        assert_eq!(back, account);
    }
    let json = serde_json::to_value(ChatAccount {
        id: "a".to_owned(),
        name: "n".to_owned(),
        service: ChatService::Discord {
            token: Secret::default(),
        },
    })
    .expect("json");
    assert_eq!(json["service"], "discord");
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
fn a_teamspeak_account_without_secrets_loads() {
    let account: ChatAccount = serde_json::from_value(serde_json::json!({
        "id": "t", "name": "Net", "service": "teamspeak", "address": "ts.example.org"
    }))
    .expect("parsed");
    let ChatService::Teamspeak {
        password,
        identity,
        nickname,
        ..
    } = account.service
    else {
        panic!("not teamspeak");
    };
    assert!(!password.is_set() && !identity.is_set());
    assert!(nickname.is_empty());
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
        to_chat_db: Some(-20.0),
        to_air_db: None,
        to_chat_open: true,
        to_air_open: false,
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
