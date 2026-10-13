use std::fmt::Write as _;

use sdrmm_wire::{
    EventKind, KindSet, Multi, NodeCategory, NodeTypeInfo, PatchCatalog, Payload, PortDirection,
    PortRepeat, PortSpec,
};

const LISTED_KINDS: usize = 6;

pub(crate) fn page(catalog: &PatchCatalog) -> String {
    let mut out = String::from(
        "# Wires and what they carry\n\n\
         Every port has a type and a contract. A wire joins an output to an input of the same \
         type, and the server refuses one the contract does not allow. This page is generated \
         by `cargo xtask codegen` from the node catalog.\n",
    );
    port_types(&mut out, catalog);
    event_kinds(&mut out, catalog);
    for category in [
        NodeCategory::Source,
        NodeCategory::Channel,
        NodeCategory::Tool,
        NodeCategory::Output,
    ] {
        nodes(&mut out, catalog, category);
    }
    out
}

fn port_types(out: &mut String, catalog: &PatchCatalog) {
    out.push_str("\n## Port types\n\n| Type | Carries |\n|---|---|\n");
    for info in &catalog.port_types {
        let _ = writeln!(
            out,
            "| {} | {} |",
            info.name,
            info.note.as_deref().unwrap_or("")
        );
    }
}

fn event_kinds(out: &mut String, catalog: &PatchCatalog) {
    out.push_str("\n## Event kinds\n\n| Kind | Name | Shows as | Facts |\n|---|---|---|---|\n");
    for entry in &catalog.event_kinds {
        let caps: Vec<String> = entry.caps.iter().map(snake).collect();
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} |",
            entry.kind.as_str(),
            entry.name,
            entry.noun,
            caps.join(", ")
        );
    }
}

fn nodes(out: &mut String, catalog: &PatchCatalog, category: NodeCategory) {
    let _ = write!(out, "\n## {}\n", heading(category));
    for node in catalog
        .nodes
        .iter()
        .filter(|node| node.category == category)
    {
        node_section(out, node);
    }
}

const fn heading(category: NodeCategory) -> &'static str {
    match category {
        NodeCategory::Source => "Sources",
        NodeCategory::Channel => "Channels",
        NodeCategory::Tool => "Tools",
        NodeCategory::Output => "Outputs",
    }
}

fn node_section(out: &mut String, node: &NodeTypeInfo) {
    let _ = write!(out, "\n### {}\n\n{}\n", node.name, node.summary);
    if node.needs_channel_type {
        out.push_str("\nPorts follow the decoder the node runs.\n");
    }
    if node.ports.is_empty() {
        return;
    }
    out.push_str("\n| Port | Way | Type | Carries | Wires |\n|---|---|---|---|---|\n");
    for port in &node.ports {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} |",
            port.name,
            way(port.direction),
            port.port_type.name(),
            carries(&port.payload),
            wires(port)
        );
    }
}

const fn way(direction: PortDirection) -> &'static str {
    match direction {
        PortDirection::In => "in",
        PortDirection::Out => "out",
    }
}

fn carries(payload: &Payload) -> String {
    match payload {
        Payload::Iq { origins } => listed(origins.iter().map(snake)),
        Payload::Audio { streams } => listed(streams.iter().map(snake)),
        Payload::Events { kinds } => kinds_of(*kinds),
        Payload::Control { verbs } => listed(verbs.iter().map(snake)),
        Payload::Baseband
        | Payload::Video
        | Payload::Position
        | Payload::Array
        | Payload::Bearing => String::new(),
    }
}

fn kinds_of(kinds: KindSet) -> String {
    if kinds == KindSet::all() {
        return "any kind".to_owned();
    }
    if kinds.len() > LISTED_KINDS {
        return format!("{} kinds", kinds.len());
    }
    listed(kinds.iter().map(EventKind::name))
}

fn wires(port: &PortSpec) -> String {
    let count = match port.multi {
        Multi::One => "one",
        Multi::Many => "many",
        Multi::ManyLanes => "many lanes",
    };
    let repeat = match port.repeat {
        PortRepeat::Once => "",
        PortRepeat::PerRxStream => ", one per lane",
        PortRepeat::PerLane => ", one per array lane",
    };
    let note = port
        .note
        .as_deref()
        .map_or_else(String::new, |note| format!(". {note}"));
    format!("{count}{repeat}{note}")
}

fn listed<T: AsRef<str>>(items: impl Iterator<Item = T>) -> String {
    items
        .map(|item| item.as_ref().to_owned())
        .collect::<Vec<_>>()
        .join(", ")
}

fn snake<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|json| json.as_str().map(|text| text.replace('_', " ")))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_names_every_node_port_type_and_kind() {
        let catalog = PatchCatalog::build();
        let page = page(&catalog);
        for node in &catalog.nodes {
            assert!(
                page.contains(&format!("### {}\n", node.name)),
                "{}",
                node.name
            );
        }
        for info in &catalog.port_types {
            assert!(
                page.contains(&format!("| {} |", info.name)),
                "{}",
                info.name
            );
        }
        for entry in &catalog.event_kinds {
            assert!(page.contains(&format!("`{}`", entry.kind.as_str())));
        }
        assert!(!page.contains('\u{2014}'), "no em dash");
    }
}
