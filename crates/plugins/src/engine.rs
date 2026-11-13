use std::{
    sync::{LazyLock, Once},
    thread,
    time::Duration,
};

use wasmtime::{Config, Engine, OptLevel, StoreLimits, StoreLimitsBuilder};

use crate::PluginError;

/// How often the epoch advances; a plugin call gets [`CALL_BUDGET`] of them.
const TICK: Duration = Duration::from_millis(10);

/// Ticks one plugin call may run before it is stopped: long enough for any sane block, short
/// enough that a plugin stuck in a loop cannot hold a DSP thread.
pub(crate) const CALL_BUDGET: u64 = 50;

/// Linear memory one plugin instance may grow to: room for large FFTs and sample buffers, while
/// a leaking or hostile plugin fails its own allocation instead of taking the host down.
pub(crate) const MEMORY_LIMIT: usize = 256 << 20;

/// Table entries one instance may hold, far above what a compiled plugin's function table needs.
const TABLE_LIMIT: usize = 1 << 20;

/// What one plugin instance may claim: one memory of at most [`MEMORY_LIMIT`].
pub(crate) fn limits() -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(MEMORY_LIMIT)
        .table_elements(TABLE_LIMIT)
        .instances(1)
        .memories(1)
        .tables(4)
        .build()
}

static ENGINE: LazyLock<Result<Engine, String>> = LazyLock::new(|| {
    let mut config = Config::new();
    config.epoch_interruption(true);
    config.cranelift_opt_level(OptLevel::Speed);
    Engine::new(&config).map_err(|error| error.to_string())
});

static TICKER: Once = Once::new();

/// The engine every plugin compiles and runs on, with its epoch clock started.
pub(crate) fn engine() -> Result<&'static Engine, PluginError> {
    let engine = ENGINE
        .as_ref()
        .map_err(|error| PluginError::Engine(error.clone()))?;
    TICKER.call_once(|| start_ticker(engine.clone()));
    Ok(engine)
}

fn start_ticker(engine: Engine) {
    let spawned = thread::Builder::new()
        .name("plugin-epoch".to_owned())
        .spawn(move || {
            loop {
                thread::sleep(TICK);
                engine.increment_epoch();
            }
        });
    if let Err(error) = spawned {
        tracing::error!(%error, "plugin epoch clock did not start; plugin calls cannot be stopped");
    }
}
