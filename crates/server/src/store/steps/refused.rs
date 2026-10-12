use sdrmm_wire::{
    PatchEdge, PatchGraph, WorkspaceNoticeKind,
    patch::resolve::{self, ResolveCtx},
};
use serde_json::Value;

use super::wires;

pub(super) fn drop_refused_edges(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    let Some(graph) = snapshot
        .get("graph")
        .and_then(|graph| serde_json::from_value::<PatchGraph>(graph.clone()).ok())
    else {
        return Vec::new();
    };
    let ctx = ResolveCtx {
        descriptors: Some(sdrmm_channels::descriptor_table()),
        live: None,
    };
    let (_, mut refused) = resolve::compile_draft(&graph, &ctx);
    if refused.is_empty() {
        return Vec::new();
    }
    wires::drop_where(snapshot, |edge| {
        let found = PatchEdge {
            from: wires::end(edge, "from")?,
            to: wires::end(edge, "to")?,
        };
        let at = refused.iter().position(|wire| wire.edge == found)?;
        Some(refused.remove(at).reason)
    })
}
