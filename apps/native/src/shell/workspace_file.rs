use sdrmm_wire::workspace::MAX_NAME_LEN;
use serde_json::Value;

pub const DEFAULT_NAME: &str = "Workspace";

pub const NOT_JSON: &str = "That file is not a workspace: it is not JSON.";
pub const NOT_WORKSPACE: &str = "That file is not a workspace.";

#[must_use]
pub fn workspace_name(typed: &str) -> String {
    let cut: String = typed.trim().chars().take(MAX_NAME_LEN).collect();
    let trimmed = cut.trim_end();
    if trimmed.is_empty() {
        DEFAULT_NAME.to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[must_use]
pub fn export_file_name(name: &str) -> String {
    let mut slug = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        String::from("workspace.json")
    } else {
        format!("workspace-{slug}.json")
    }
}

pub fn parse_export(text: &str) -> Result<Value, &'static str> {
    let document: Value = serde_json::from_str(text).map_err(|_| NOT_JSON)?;
    let versioned = document.get("version").is_some_and(Value::is_u64);
    let named = document
        .get("name")
        .and_then(Value::as_str)
        .is_some_and(|name| !name.trim().is_empty());
    let laid_out = document
        .get("snapshot")
        .and_then(|snapshot| snapshot.get("graph"))
        .and_then(|graph| graph.get("nodes"))
        .is_some_and(Value::is_array);
    if versioned && named && laid_out {
        Ok(document)
    } else {
        Err(NOT_WORKSPACE)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn document() -> Value {
        json!({
            "version": 1,
            "name": "Airband Watch",
            "snapshot": {
                "version": 3,
                "graph": {
                    "nodes": [
                        { "id": "dev", "kind": "device", "data": {}, "position": { "x": 0, "y": 0 } },
                        { "id": "scope", "kind": "scope", "position": { "x": 400, "y": 0 } }
                    ],
                    "edges": [
                        { "from": { "node": "dev", "port": "iq" }, "to": { "node": "scope", "port": "iq" } }
                    ]
                }
            },
            "state": { "version": 2, "devices": [{ "node": "dev", "settings": { "center_hz": 145_500_000.0 } }] }
        })
    }

    #[test]
    fn keeps_what_was_typed() {
        assert_eq!(workspace_name("Bench"), "Bench");
    }

    #[test]
    fn trims_the_edges_the_server_would_reject() {
        assert_eq!(workspace_name("  Bench  "), "Bench");
    }

    #[test]
    fn falls_back_to_the_default_when_nothing_was_typed() {
        assert_eq!(workspace_name(""), DEFAULT_NAME);
        assert_eq!(workspace_name("   "), DEFAULT_NAME);
    }

    #[test]
    fn cuts_a_name_the_server_would_refuse_for_its_length() {
        assert_eq!(
            workspace_name(&"x".repeat(MAX_NAME_LEN + 10)),
            "x".repeat(MAX_NAME_LEN)
        );
    }

    #[test]
    fn names_the_export_file_after_the_workspace() {
        assert_eq!(
            export_file_name("Airband Watch"),
            "workspace-airband-watch.json"
        );
        assert_eq!(export_file_name("!!"), "workspace.json");
    }

    #[test]
    fn hands_the_file_over_untouched_for_the_server_to_upgrade() {
        let read = parse_export(&document().to_string()).expect("a workspace");
        assert_eq!(read, document());
        assert_eq!(read["snapshot"]["version"], 3);
    }

    #[test]
    fn refuses_a_file_that_is_not_a_workspace() {
        assert_eq!(parse_export("not json").err(), Some(NOT_JSON));
        assert_eq!(parse_export("[]").err(), Some(NOT_WORKSPACE));
        assert_eq!(
            parse_export(&json!({ "version": 1, "name": "No layout" }).to_string()).err(),
            Some(NOT_WORKSPACE)
        );
        let mut unnamed = document();
        unnamed["name"] = json!("  ");
        assert_eq!(
            parse_export(&unnamed.to_string()).err(),
            Some(NOT_WORKSPACE)
        );
    }
}
