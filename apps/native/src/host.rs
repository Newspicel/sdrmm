use std::{
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
};

use anyhow::Context;
use sdrmm_engine::Engine;
use sdrmm_server::{Config, ServerOptions};

pub struct Host {
    pub base: String,
    engine: Option<Arc<Engine>>,
}

impl Host {
    pub fn remote(base: String) -> Self {
        Self {
            base: base.trim_end_matches('/').to_owned(),
            engine: None,
        }
    }

    pub async fn embedded(data_dir: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("cannot create {}", data_dir.display()))?;
        let engine = Engine::with_registry(
            sdrmm_engine::builtin_registry_accelerated(Some(data_dir.join("recordings")), 1.0),
            Some(data_dir.join("recordings")),
        );
        let config = Config {
            bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            db_path: Some(data_dir.join("native.db")),
            tls: None,
            options: ServerOptions::default(),
        };
        let handle = sdrmm_server::serve(config, engine.clone())
            .await
            .context("cannot start the embedded server")?;
        let base = format!("http://{}", handle.local_addr);
        tokio::spawn(async move {
            if let Err(error) = handle.join().await {
                tracing::error!(%error, "embedded server exited");
            }
        });
        Ok(Self {
            base,
            engine: Some(engine),
        })
    }

    pub fn websocket_url(&self) -> String {
        let scheme = if self.base.starts_with("https://") {
            "wss://"
        } else {
            "ws://"
        };
        let rest = self
            .base
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        format!("{scheme}{rest}/api/ws")
    }

    pub fn shutdown(&self) {
        if let Some(engine) = &self.engine {
            engine.shutdown();
        }
    }
}
