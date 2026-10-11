use sdrmm_wire::patch::{ChannelNode, NodeBody, NodeCategory, PatchGraph, PatchNode, Position};
use zgui::prelude::*;
use zgui_ui::prelude::*;

use crate::ui::patch::Canvas;
use crate::{
    store::Store,
    ui::{node, widgets::segments},
};


pub fn sheet(store: Store, canvas: Canvas) -> impl IntoView {
    let search = RwSignal::new_local(String::new());
    let category = RwSignal::new(None::<NodeCategory>);
    let rows = move || {
        let catalog = store.catalog.get();
        let mut entries: Vec<(String, String, NodeCategory)> = catalog
            .nodes
            .iter()
            .filter(|info| !info.needs_channel_type)
            .map(|info| (info.kind.clone(), info.name.clone(), info.category))
            .collect();
        for descriptor in store.channel_types.get().iter() {
            entries.push((
                format!("channel:{}", descriptor.type_id),
                descriptor.name.clone(),
                NodeCategory::Channel,
            ));
        }
        entries
            .into_iter()
            .filter(|(kind, name, group)| {
                matches_search(kind, name, *group, &search.get(), category.get())
            })
            .map(|(kind, name, category)| {
                let category_label = node::category_class(category).to_uppercase();
                let chosen = kind.clone();
                view! {
                    control(
                        class = "pal__row",
                        a11y:role = Role::Button,
                        tabindex = Focus::Sequential,
                        on:click:stop = move |_| {
                            add(store, canvas, &chosen);
                            store.palette.set(false);
                        }
                    ) {
                        text {{name}}
                        text(class = "pal__cat") {{category_label}}
                    }
                }
            })
            .collect::<Vec<_>>()
    };

    view! {
        column(class = "pal") {
            row(class = "pal__head") {
                text(class = "legend") {"Add a node"}
                spacer()
                control(class = "node__shut", on:click:stop = move |_| store.palette.set(false)) {"x"}
            }
            box(class = "pal__search", on:key_down = crate::ui::kit_shell::typing) {
                Input(class = "native-input", value = search, label = "Search nodes", placeholder = "Search nodes…")
            }
            {segments(
                vec![(None, "All"), (Some(NodeCategory::Source), "Sources"), (Some(NodeCategory::Channel), "Channels"), (Some(NodeCategory::Tool), "Tools"), (Some(NodeCategory::Output), "Outputs")],
                category.into(), move |chosen| category.set(chosen),
            )}
            column(class = "pal__list") {{rows}}
        }
    }
}

fn matches_search(
    kind: &str,
    name: &str,
    category: NodeCategory,
    search: &str,
    selected: Option<NodeCategory>,
) -> bool {
    if selected.is_some_and(|selected| selected != category) {
        return false;
    }
    let haystack = format!("{kind} {name}").to_lowercase();
    search
        .split_whitespace()
        .all(|term| haystack.contains(&term.to_lowercase()))
}

pub fn body_for(kind: &str) -> Option<NodeBody> {
    if let Some(type_id) = kind.strip_prefix("channel:") {
        return Some(NodeBody::Channel(ChannelNode {
            channel_type: type_id.to_owned(),
            tuning_locked: false,
        }));
    }
    NodeBody::default_for(kind)
}

fn add(store: Store, canvas: Canvas, kind: &str) {
    let Some(body) = body_for(kind) else {
        store.say(format!("Unknown node kind: {kind}"));
        return;
    };
    let at = store
        .palette_at
        .get_untracked()
        .unwrap_or_else(|| crate::ui::patch::place_new(canvas, body.kind()));
    store.palette_at.set(None);
    let taken = crate::ui::patch::graph::node_ids(&store.graph.get_untracked());
    let id = crate::ui::patch::graph::new_node_id(body.kind(), &taken);
    let added = id.clone();
    let runs_alone = matches!(body, NodeBody::SignalGen(_));
    let add = move |graph: &mut PatchGraph| {
        graph.nodes.push(PatchNode {
            id: added,
            body,
            position: Position { x: at.0, y: at.1 },
            size: None,
            label: None,
        });
    };
    if runs_alone {
        store.edit_graph(add);
    } else {
        store.edit_graph_quiet(add);
    }
    store.selected.set(Some(id));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_channel_entry_carries_its_decoder_into_the_node() {
        let NodeBody::Channel(channel) = body_for("channel:wfm").expect("a channel") else {
            panic!("a channel node");
        };
        assert_eq!(channel.channel_type, "wfm");
    }

    #[test]
    fn every_catalog_entry_has_a_typed_body() {
        for entry in sdrmm_wire::patch::PatchCatalog::build().nodes {
            if entry.needs_channel_type {
                continue;
            }
            let body = body_for(&entry.kind).unwrap_or_else(|| panic!("missing {}", entry.kind));
            assert_eq!(body.kind(), entry.kind);
            assert_eq!(body.category(), entry.category);
            let encoded = serde_json::to_value(&body).expect("encode");
            assert_eq!(
                serde_json::from_value::<NodeBody>(encoded).expect("decode"),
                body
            );
        }
        assert!(body_for("unknown_node").is_none());
    }

    #[test]
    fn search_combines_words_and_category_without_case_sensitivity() {
        assert!(matches_search(
            "channel:nfm",
            "Narrow FM",
            NodeCategory::Channel,
            "FM narrow",
            None
        ));
        assert!(!matches_search(
            "channel:nfm",
            "Narrow FM",
            NodeCategory::Channel,
            "FM",
            Some(NodeCategory::Tool)
        ));
        assert!(!matches_search(
            "channel:nfm",
            "Narrow FM",
            NodeCategory::Channel,
            "FM wide",
            None
        ));
    }
}
