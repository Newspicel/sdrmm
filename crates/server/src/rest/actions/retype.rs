use sdrmm_wire::{
    ChannelSettings, NodeBody, NoticeWire, PatchEdge, PatchGraph, UpdateWorkspaceRequest,
    WorkspaceNoticeKind,
    patch::resolve::{self, ResolveCtx},
};

use super::takes_no_such_action;
use crate::{
    AppState,
    rest::{AppError, active_workspace, channel_settings, lock_gate, set_channel},
};

pub(super) fn retype(
    state: &AppState,
    node: &str,
    channel_type: &str,
    author: Option<&str>,
) -> Result<Vec<NoticeWire>, AppError> {
    let descriptors = state.engine.channel_types();
    let defaults = descriptors
        .iter()
        .find(|descriptor| descriptor.type_id == channel_type)
        .and_then(|descriptor| descriptor.defaults.clone())
        .ok_or_else(|| AppError::bad_request(format!("No decoder {channel_type}")))?;
    let current = channel_settings(state, node)?;
    if current.params.type_id() == channel_type {
        return Ok(Vec::new());
    }
    let dropped = {
        let _serialized = lock_gate(&state.apply_gate);
        rewrite(state, node, channel_type, &descriptors, author)?
    };
    set_channel(state, node, retyped(&current, defaults), None, author)?;
    state.resolve_now();
    Ok(dropped)
}

pub(super) fn retyped(current: &ChannelSettings, defaults: ChannelSettings) -> ChannelSettings {
    ChannelSettings {
        frequency_hz: current.frequency_hz,
        squelch: current.squelch,
        ..defaults
    }
}

fn rewrite(
    state: &AppState,
    node: &str,
    channel_type: &str,
    descriptors: &[sdrmm_wire::ChannelDescriptor],
    author: Option<&str>,
) -> Result<Vec<NoticeWire>, AppError> {
    let mut active = active_workspace(&state.store)?;
    let graph = &mut active.snapshot.graph;
    match graph.nodes.iter_mut().find(|held| held.id == node) {
        Some(held) => match &mut held.body {
            NodeBody::Channel(channel) => channel.channel_type = channel_type.to_owned(),
            _ => return Err(takes_no_such_action(node)),
        },
        None => return Err(AppError::not_found(format!("No node {node}"))),
    }
    let dropped = drop_refused(graph, descriptors);
    state.store.update_workspace(
        active.info.id,
        &UpdateWorkspaceRequest {
            revision: active.info.revision,
            name: None,
            snapshot: Some(active.snapshot),
        },
        author,
    )?;
    if !dropped.is_empty() {
        state.store.add_notices(
            active.info.id,
            &[WorkspaceNoticeKind::DroppedWires {
                wires: dropped.clone(),
            }],
        )?;
    }
    state.engine.emit_scope(sdrmm_wire::StateScope::Workspaces);
    Ok(dropped)
}

pub(super) fn drop_refused(
    graph: &mut PatchGraph,
    descriptors: &[sdrmm_wire::ChannelDescriptor],
) -> Vec<NoticeWire> {
    let ctx = ResolveCtx {
        descriptors: Some(descriptors),
        live: None,
    };
    let (_, refused) = resolve::compile_draft(graph, &ctx);
    graph
        .edges
        .retain(|edge: &PatchEdge| !refused.iter().any(|wire| wire.edge == *edge));
    refused
        .into_iter()
        .map(|wire| NoticeWire {
            from: wire.edge.from,
            to: wire.edge.to,
            reason: wire.reason,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::Squelch;

    use super::*;

    #[test]
    fn a_retyped_decoder_keeps_where_it_listens_and_its_squelch() {
        let mut current = ChannelSettings::default_for("nfm").expect("nfm");
        current.frequency_hz = 145_500_000.0;
        current.squelch = Squelch::Manual { level_db: -42.0 };
        let defaults = ChannelSettings::default_for("am").expect("am");
        let retyped = retyped(&current, defaults.clone());
        assert_eq!(retyped.frequency_hz, 145_500_000.0);
        assert_eq!(retyped.squelch, Squelch::Manual { level_db: -42.0 });
        assert_eq!(retyped.params, defaults.params);
    }
}
