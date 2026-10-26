use sdrmm_wire::{ExampleInfo, ExampleNeeds, WorkspaceExport, kinds_in};

use crate::packed::{inflate, packed_data};

struct Example {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    body: &'static [u8],
}

static EXAMPLES: &[Example] = &[Example {
    id: "no-radio",
    name: "No radio",
    description: "A receiver built on a signal generator, for learning the canvas with no hardware.",
    body: packed_data!("examples/no-radio.json"),
}];

#[derive(Debug)]
pub(crate) enum ExampleError {
    Unknown(String),
    Malformed { id: String, detail: String },
}

impl std::fmt::Display for ExampleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(id) => write!(f, "no such example: {id}"),
            Self::Malformed { id, detail } => write!(f, "the {id} example is malformed: {detail}"),
        }
    }
}

impl std::error::Error for ExampleError {}

fn parse(id: &str, body: &[u8]) -> Result<serde_json::Value, ExampleError> {
    let malformed = |detail: String| ExampleError::Malformed {
        id: id.to_owned(),
        detail,
    };
    let raw = inflate(body).map_err(|err| malformed(err.to_string()))?;
    serde_json::from_slice(&raw).map_err(|err| malformed(err.to_string()))
}

pub(crate) fn export(id: &str) -> Result<serde_json::Value, ExampleError> {
    let example = EXAMPLES
        .iter()
        .find(|example| example.id == id)
        .ok_or_else(|| ExampleError::Unknown(id.to_owned()))?;
    parse(example.id, example.body)
}

pub(crate) fn all() -> Result<Vec<ExampleInfo>, ExampleError> {
    EXAMPLES
        .iter()
        .map(|example| {
            let document = parse(example.id, example.body)?;
            let parsed: WorkspaceExport =
                crate::json::from_value(&document).map_err(|err| ExampleError::Malformed {
                    id: example.id.to_owned(),
                    detail: err.to_string(),
                })?;
            Ok(ExampleInfo {
                id: example.id.to_owned(),
                name: example.name.to_owned(),
                description: example.description.to_owned(),
                needs: ExampleNeeds::of(&parsed.snapshot.graph, &parsed.state),
                node_kinds: kinds_in(&parsed.snapshot.graph),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use sdrmm_wire::{NodeKind, WorkspaceExport};

    use super::*;

    #[test]
    fn every_example_parses_and_lists() {
        let listed = all().expect("examples list");
        assert_eq!(listed.len(), EXAMPLES.len());
    }

    #[test]
    fn ids_are_unique_and_match_their_filenames() {
        let mut seen = BTreeSet::new();
        for example in EXAMPLES {
            assert!(seen.insert(example.id), "duplicate id {}", example.id);
            let path = format!("data/examples/{}.json", example.id);
            assert!(std::path::Path::new(&path).exists(), "missing {path}");
        }
    }

    #[test]
    fn every_example_carries_a_note() {
        for info in all().expect("examples list") {
            assert!(
                info.node_kinds.contains(&NodeKind::Note),
                "{} has no note",
                info.id
            );
        }
    }

    #[test]
    fn an_unknown_id_is_an_unknown_error() {
        assert!(matches!(export("nope"), Err(ExampleError::Unknown(_))));
    }

    #[test]
    fn malformed_bytes_name_the_example() {
        let err = parse("broken", b"not gzip").expect_err("malformed");
        assert!(matches!(&err, ExampleError::Malformed { id, .. } if id == "broken"));
    }

    #[test]
    fn the_no_radio_example_needs_no_hardware() {
        let info = all()
            .expect("examples list")
            .into_iter()
            .find(|e| e.id == "no-radio")
            .expect("no-radio");
        assert_eq!(info.needs.radios, 0);
        assert_eq!(info.needs.recordings, 0);
    }

    #[test]
    fn every_example_imports() {
        let store = crate::store::Store::open(None).expect("store");
        for example in EXAMPLES {
            let mut document = export(example.id).expect("export");
            let notices = crate::store::upgrade_import(&mut document);
            let parsed: WorkspaceExport = crate::json::from_value(&document).expect(example.id);
            store.import_workspace(&parsed, &notices).expect(example.id);
        }
    }
}
