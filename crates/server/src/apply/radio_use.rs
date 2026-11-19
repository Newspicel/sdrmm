use sdrmm_wire::{NodeBody, PatchGraph, PatchRefusal, RadioUse};

use crate::{AppState, workspace};

pub(super) fn of(body: &NodeBody) -> RadioUse {
    match body {
        NodeBody::Device(device) => device.radio_use,
        _ => RadioUse::default(),
    }
}

/// Puts every open radio to the use its node names.
pub(crate) fn settle_uses(state: &AppState, graph: &PatchGraph) -> Vec<PatchRefusal> {
    workspace::bind_devices(graph, &state.engine.snapshot())
        .into_iter()
        .filter_map(|(node, device_set)| {
            let wanted = graph
                .node(&node)
                .map_or_else(RadioUse::default, |found| of(&found.body));
            let error = state.engine.set_radio_use(device_set, wanted).err()?;
            Some(PatchRefusal {
                node,
                reason: error.to_string(),
            })
        })
        .collect()
}
