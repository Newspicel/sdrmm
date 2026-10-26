use sdrmm_wire::{ExampleInfo, ExampleNeeds, WorkspaceExport, kinds_in};

use crate::packed::{inflate, packed_data};

struct Example {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    body: &'static [u8],
}

static EXAMPLES: &[Example] = &[
    Example {
        id: "no-radio",
        name: "No radio",
        description: "A receiver on a signal generator, for learning the canvas with no hardware.",
        body: packed_data!("examples/no-radio.json"),
    },
    Example {
        id: "satellite-pass",
        name: "Satellite pass",
        description: "Predicts a pass, points a rotator, keeps the IQ.",
        body: packed_data!("examples/satellite-pass.json"),
    },
    Example {
        id: "record-replay",
        name: "Record and replay",
        description: "Play an IQ file back, keep the channel, hold the last 30 seconds.",
        body: packed_data!("examples/record-replay.json"),
    },
    Example {
        id: "scanner-sweep",
        name: "Scanner sweep",
        description: "A scanner drives the decoder while a monitor names what else is there.",
        body: packed_data!("examples/scanner-sweep.json"),
    },
    Example {
        id: "hf-station",
        name: "HF station",
        description: "FT8 and WSPR on one radio, into a propagation map.",
        body: packed_data!("examples/hf-station.json"),
    },
    Example {
        id: "trunked-talkgroups",
        name: "Trunked talkgroups",
        description: "A trunk system follows its own traffic, cleaned and recorded.",
        body: packed_data!("examples/trunked-talkgroups.json"),
    },
    Example {
        id: "network-feed",
        name: "Network feed",
        description: "Decodes to a file, a webhook, and raw IQ over the network.",
        body: packed_data!("examples/network-feed.json"),
    },
    Example {
        id: "df-drive",
        name: "DF drive",
        description: "Five coherent lanes into bearings, crossed into a fix as you drive.",
        body: packed_data!("examples/df-drive.json"),
    },
    Example {
        id: "passive-radar",
        name: "Passive radar",
        description: "Aircraft echoes of a broadcast FM station, without transmitting.",
        body: packed_data!("examples/passive-radar.json"),
    },
    Example {
        id: "beam-and-stitch",
        name: "Beam and stitch",
        description: "Aim the array at what the direction finder sees, or join its lanes wide.",
        body: packed_data!("examples/beam-and-stitch.json"),
    },
    Example {
        id: "array-lab",
        name: "Array lab",
        description: "Bearing over frequency, baseline visibilities and polarisation.",
        body: packed_data!("examples/array-lab.json"),
    },
];

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
    fn each_example_asks_for_the_hardware_its_graph_implies() {
        let listed = all().expect("examples list");
        let needs = |id: &str| {
            listed
                .iter()
                .find(|e| e.id == id)
                .map(|e| (e.needs.radios, e.needs.recordings, e.needs.lanes))
                .expect(id)
        };
        assert_eq!(needs("no-radio"), (0, 0, 0));
        assert_eq!(needs("satellite-pass"), (1, 0, 0));
        assert_eq!(needs("record-replay"), (0, 1, 0));
        assert_eq!(needs("scanner-sweep"), (1, 0, 0));
        assert_eq!(needs("hf-station"), (1, 0, 0));
        assert_eq!(needs("trunked-talkgroups"), (1, 0, 0));
        assert_eq!(needs("network-feed"), (1, 0, 0));
    }

    #[test]
    fn the_array_examples_ask_for_lanes() {
        let listed = all().expect("examples list");
        for id in ["df-drive", "passive-radar", "beam-and-stitch", "array-lab"] {
            let info = listed.iter().find(|e| e.id == id).expect(id);
            assert!(
                info.needs.lanes >= 2,
                "{id} asks for {} lanes",
                info.needs.lanes
            );
        }
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
