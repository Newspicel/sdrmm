use sdrmm_wire::{
    patch::NodeTypeInfo,
    workspace::{WorkspaceNotice, WorkspaceNoticeKind},
};

const SHOWN_KINDS: usize = 4;
const RETIRED_NAMES: [(&str, &str); 1] = [("df", "DF")];
const CLEARED_GPS_TITLE: &str = "This device is gone. Pick a phone.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoticeLine {
    pub id: i64,
    pub text: String,
    pub title: String,
}

fn spoken(kind: &str) -> String {
    let words = kind.replace('_', " ");
    let mut letters = words.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

fn kind_name(kind: &str, catalog: &[NodeTypeInfo]) -> String {
    catalog
        .iter()
        .find(|entry| entry.kind == kind)
        .map(|entry| entry.name.clone())
        .or_else(|| {
            RETIRED_NAMES
                .iter()
                .find(|(retired, _)| *retired == kind)
                .map(|(_, name)| (*name).to_owned())
        })
        .unwrap_or_else(|| spoken(kind))
}

#[must_use]
pub fn notice_line(notice: &WorkspaceNotice, catalog: &[NodeTypeInfo]) -> NoticeLine {
    match &notice.notice {
        WorkspaceNoticeKind::ClearedGps { nodes } => NoticeLine {
            id: notice.id,
            text: format!("GPS source cleared: {}", nodes.join(", ")),
            title: CLEARED_GPS_TITLE.to_owned(),
        },
        WorkspaceNoticeKind::DroppedNodes { nodes } => {
            let mut names: Vec<String> = Vec::new();
            for node in nodes {
                let name = kind_name(&node.kind, catalog);
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            let more = names
                .len()
                .checked_sub(SHOWN_KINDS)
                .filter(|extra| *extra > 0)
                .map_or_else(String::new, |extra| format!(" +{extra}"));
            names.truncate(SHOWN_KINDS);
            NoticeLine {
                id: notice.id,
                text: format!("Removed old nodes: {}{more}", names.join(", ")),
                title: nodes
                    .iter()
                    .map(|node| node.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{
        patch::{NodeBody, NodeCategory},
        workspace::DroppedNode,
    };

    use super::*;

    fn entry(kind: &str, name: &str) -> NodeTypeInfo {
        NodeTypeInfo {
            kind: kind.to_owned(),
            name: name.to_owned(),
            category: NodeCategory::Tool,
            summary: String::new(),
            ports: Vec::new(),
            needs_channel_type: false,
            default_body: NodeBody::Scope,
        }
    }

    fn dropped(id: i64, nodes: &[(&str, &str)]) -> WorkspaceNotice {
        WorkspaceNotice {
            id,
            at: "2026-09-28T12:00:00Z".to_owned(),
            notice: WorkspaceNoticeKind::DroppedNodes {
                nodes: nodes
                    .iter()
                    .map(|(node, kind)| DroppedNode {
                        id: (*node).to_owned(),
                        kind: (*kind).to_owned(),
                        label: None,
                    })
                    .collect(),
            },
        }
    }

    fn catalog() -> Vec<NodeTypeInfo> {
        vec![entry("scope", "Scope")]
    }

    #[test]
    fn lists_removed_kinds_with_their_ids_as_the_title() {
        let line = notice_line(
            &dropped(
                7,
                &[("df1", "df"), ("arr", "array"), ("radar", "passive_radar")],
            ),
            &catalog(),
        );
        assert_eq!(line.text, "Removed old nodes: DF, Array, Passive radar");
        assert_eq!(line.title, "df1, arr, radar");
    }

    #[test]
    fn a_cleared_gps_says_which_and_why() {
        let line = notice_line(
            &WorkspaceNotice {
                id: 8,
                at: String::new(),
                notice: WorkspaceNoticeKind::ClearedGps {
                    nodes: vec!["gps1".to_owned(), "gps2".to_owned()],
                },
            },
            &catalog(),
        );
        assert_eq!(line.text, "GPS source cleared: gps1, gps2");
        assert_eq!(line.title, CLEARED_GPS_TITLE);
    }

    #[test]
    fn names_each_kind_once_and_counts_the_rest() {
        let line = notice_line(
            &dropped(
                1,
                &[
                    ("a", "df"),
                    ("b", "df"),
                    ("c", "array"),
                    ("d", "old_mixer"),
                    ("e", "stitch"),
                    ("f", "passive_radar"),
                ],
            ),
            &catalog(),
        );
        assert_eq!(
            line.text,
            "Removed old nodes: DF, Array, Old mixer, Stitch +1"
        );
    }

    #[test]
    fn prefers_the_catalog_name_of_a_kind() {
        let line = notice_line(
            &dropped(1, &[("a", "df")]),
            &[entry("df", "Direction finder")],
        );
        assert_eq!(line.text, "Removed old nodes: Direction finder");
    }
}
