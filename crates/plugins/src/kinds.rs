//! The event kinds a plugin declares, presented by asking the plugin.

use std::sync::Arc;

use sdrmm_wire::{EventKind, KindCodec, KindSpec, present::Presentation};

use crate::module::PluginModule;

/// Every kind `module` declares, each presented through the plugin's own presenter.
#[must_use]
pub fn plugin_kinds(module: &Arc<PluginModule>) -> Vec<KindSpec> {
    let codec: Arc<dyn KindCodec> = Arc::new(PluginCodec {
        module: module.clone(),
    });
    module
        .manifest
        .kinds
        .iter()
        .map(|entry| KindSpec::with_codec(entry.clone(), codec.clone()))
        .collect()
}

struct PluginCodec {
    module: Arc<PluginModule>,
}

impl KindCodec for PluginCodec {
    fn present(&self, kind: &EventKind, data: &serde_json::Value) -> Result<Presentation, String> {
        self.module.present(kind, data)
    }
}
