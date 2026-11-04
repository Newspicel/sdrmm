use std::{
    sync::{LazyLock, Once},
    thread,
    time::Duration,
};

use wasmtime::{Config, Engine, OptLevel};

use crate::PluginError;

/// How often the epoch advances; a plugin call gets [`CALL_BUDGET`] of them.
const TICK: Duration = Duration::from_millis(10);

/// Ticks one plugin call may run before it is stopped: long enough for any sane block, short
/// enough that a plugin stuck in a loop cannot hold a DSP thread.
pub(crate) const CALL_BUDGET: u64 = 50;

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
