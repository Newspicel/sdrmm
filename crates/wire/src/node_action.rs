use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    ControlAck, HuntAction, NetworkExportAction, NetworkExportStatus, NoticeWire, PlaybackRequest,
    PlaybackStatus, ScanAction, TimeMachineAction, TimeMachineStatus,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "action", content = "with", rename_all = "snake_case")]
pub enum NodeAction {
    Tune { freq_hz: f64 },
    Retype { channel_type: String },
    Scan(ScanAction),
    Hunt(HuntAction),
    TimeMachine(TimeMachineAction),
    Playback(PlaybackRequest),
    NetworkExport(NetworkExportAction),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "result", content = "with", rename_all = "snake_case")]
pub enum NodeActed {
    Controlled(Vec<ControlAck>),
    Tuned,
    Retyped { dropped: Vec<NoticeWire> },
    TimeMachine(TimeMachineStatus),
    Playback(PlaybackStatus),
    NetworkExport(NetworkExportStatus),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NetworkRadioRequest {
    pub address: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_name_their_verb_and_carry_its_body() {
        let tune = serde_json::to_value(NodeAction::Tune { freq_hz: 145.5e6 }).expect("json");
        assert_eq!(
            tune,
            serde_json::json!({"action": "tune", "with": {"freq_hz": 145.5e6}})
        );
        let retype: NodeAction = serde_json::from_value(serde_json::json!({
            "action": "retype", "with": {"channel_type": "am"}
        }))
        .expect("parsed");
        assert_eq!(
            retype,
            NodeAction::Retype {
                channel_type: "am".to_owned()
            }
        );
        let tuned = serde_json::to_value(NodeActed::Tuned).expect("json");
        assert_eq!(tuned, serde_json::json!({"result": "tuned"}));
    }
}
