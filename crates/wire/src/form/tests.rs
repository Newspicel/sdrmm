use serde_json::Value;

use super::audit::audit;
use crate::patch::{NodeBody, NodeKind};

fn body_json(kind: NodeKind) -> Value {
    serde_json::to_value(NodeBody::default_for(kind)).expect("body serializes")
}

fn settings_of(body: &Value) -> Value {
    body.pointer("/data/settings")
        .cloned()
        .expect("body carries settings")
}

fn round_trip(kind: NodeKind, settings: Value) -> Value {
    let mut body = body_json(kind);
    body["data"]["settings"] = settings;
    let parsed: NodeBody = serde_json::from_value(body)
        .unwrap_or_else(|error| panic!("{kind} settings refused: {error}"));
    settings_of(&serde_json::to_value(parsed).expect("serializes"))
}

#[test]
fn every_node_form_passes_the_audit() {
    for kind in NodeKind::ALL {
        let form = kind.settings();
        if form.is_empty() {
            continue;
        }
        let defaults = settings_of(&body_json(*kind));
        audit(kind.as_str(), &form, &defaults, &|settings| {
            round_trip(*kind, settings)
        });
        assert_eq!(kind.info().settings, form);
    }
}
