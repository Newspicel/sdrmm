#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use sdrmm_engine::Engine;
use sdrmm_plugins::PluginWatcher;
use url::Url;

use crate::exit::Exit;

mod browser;
mod dialog;
mod exit;
mod newer_db;
mod paths;
mod reveal;
mod shell;
mod update;

fn main() -> ExitCode {
    #[cfg(feature = "soapy")]
    sdrmm_device_soapy::enable_isolated_probes();

    if let Err(error) = sdrmm_server::diagnostics::install_tracing() {
        eprintln!("logging is off: {error}");
    }
    let exit = run().unwrap_or_else(|error| {
        tracing::error!("{error:#}");
        dialog::fail(&format!("{error:#}"));
        Exit::Failed
    });
    exit.finish()
}

fn run() -> Result<Exit> {
    let _library = shell::load()?;
    let data_dir = paths::data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let db = data_dir.join("sdrmm.db");
    if let Err(error @ sdrmm_server::StoreError::NewerSchema { .. }) =
        sdrmm_server::Store::open(Some(&db))
    {
        return Ok(newer_db::ask(db, &error));
    }
    let runtime = tokio::runtime::Runtime::new()?;
    let engine = Engine::new(Some(data_dir.join("recordings")));
    let _plugins = watch_plugins(&data_dir, &engine);
    let url = runtime.block_on(serve(db, Arc::clone(&engine)))?;
    update::spawn(runtime.handle());
    let start = shell::Start {
        url,
        runtime: runtime.handle().clone(),
    };
    let shown = shell::run(start, &paths::cache_dir()?);
    engine.shutdown();
    shown?;
    Ok(Exit::requested())
}

fn watch_plugins(data_dir: &Path, engine: &Arc<Engine>) -> Option<Mutex<PluginWatcher>> {
    let reopen = Arc::downgrade(engine);
    match PluginWatcher::start(data_dir.join("plugins"), move |replaced| {
        if let Some(engine) = reopen.upgrade() {
            engine.plugins_reloaded(replaced);
        }
    }) {
        Ok(watcher) => Some(Mutex::new(watcher)),
        Err(error) => {
            tracing::error!(%error, "plugins are off");
            None
        }
    }
}

async fn serve(db: PathBuf, engine: Arc<Engine>) -> Result<Url> {
    let config = sdrmm_server::Config {
        bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        db_path: Some(db),
        tls: None,
        options: sdrmm_server::ServerOptions {
            shell: Some(Arc::new(reveal::Shell)),
            ..sdrmm_server::ServerOptions::default()
        },
    };
    let handle = sdrmm_server::serve(config, engine).await?;
    let url = Url::parse(&format!("http://{}", handle.local_addr))?;
    tokio::spawn(async move {
        if let Err(error) = handle.join().await {
            tracing::error!(%error, "embedded server exited");
            dialog::tell(format!("Server stopped: {error}")).await;
        }
    });
    Ok(url)
}
