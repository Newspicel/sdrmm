//! WASM decoder plugins: loaded from a directory into the decoder registry, beside the built-in
//! decoders and indistinguishable from them to the rest of the server.
//!
//! A plugin runs in wasmtime. Each call gets an epoch budget, so a plugin stuck in a loop stops
//! its own channel with an error instead of holding a DSP thread. The heavy `dsp` kernels run
//! natively as host imports working on the plugin's memory.

mod channel;
mod decoder;
mod engine;
mod kernels;
mod module;
mod samples;
mod watch;

pub use decoder::WasmDecoder;
pub use module::PluginModule;
pub use watch::{PluginWatcher, load_dir, reload};

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("plugin engine: {0}")]
    Engine(String),
    #[error("plugin host imports: {0}")]
    Link(String),
    #[error("plugin {plugin}: {error}")]
    Load { plugin: String, error: String },
    #[error("plugins directory: {0}")]
    Dir(String),
    #[error("plugins: {0}")]
    Install(String),
}
