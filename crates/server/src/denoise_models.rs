use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use reqwest::Client;
use sdrmm_engine::{DenoiseModels, Engine};
use sdrmm_wire::{DenoiseModel, DenoiseModelState, DenoiseModelStatus};
use sha2::{Digest, Sha256};

const TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Default)]
pub(crate) struct Downloads {
    active: Mutex<HashMap<DenoiseModel, DenoiseModelState>>,
}

impl Downloads {
    pub(crate) fn statuses(&self, models: &DenoiseModels) -> Vec<DenoiseModelStatus> {
        let active = self.active();
        DenoiseModel::ALL
            .into_iter()
            .map(|model| DenoiseModelStatus {
                model,
                bytes: model.artifact().bytes,
                state: match active.get(&model) {
                    Some(state) => state.clone(),
                    None if models.installed(model) => DenoiseModelState::Ready,
                    None => DenoiseModelState::Missing,
                },
            })
            .collect()
    }

    pub(crate) fn start(self: &Arc<Self>, engine: &Arc<Engine>, model: DenoiseModel) -> bool {
        {
            let mut active = self.active();
            if matches!(
                active.get(&model),
                Some(DenoiseModelState::Downloading { .. })
            ) {
                return false;
            }
            active.insert(model, DenoiseModelState::Downloading { received: 0 });
        }
        let downloads = Arc::clone(self);
        let models = Arc::clone(engine.denoise_models());
        tokio::spawn(async move {
            let url = model.url();
            let outcome = match downloads.fetch(model, &url).await {
                Ok(bytes) => install(models, model, bytes).await,
                Err(error) => Err(error),
            };
            let mut active = downloads.active();
            match outcome {
                Ok(()) => {
                    active.remove(&model);
                }
                Err(error) => {
                    tracing::warn!(model = model.name(), %error, "denoise model download failed");
                    active.insert(model, DenoiseModelState::Failed { error });
                }
            }
        });
        true
    }

    pub(crate) fn forget(&self, model: DenoiseModel) {
        let mut active = self.active();
        if matches!(active.get(&model), Some(DenoiseModelState::Failed { .. })) {
            active.remove(&model);
        }
    }

    fn active(&self) -> std::sync::MutexGuard<'_, HashMap<DenoiseModel, DenoiseModelState>> {
        self.active.lock().unwrap_or_else(PoisonError::into_inner)
    }

    async fn fetch(&self, model: DenoiseModel, url: &str) -> Result<Vec<u8>, String> {
        let expected = model.artifact();
        let client = Client::builder()
            .timeout(TIMEOUT)
            .build()
            .map_err(|error| format!("no HTTP client: {error}"))?;
        let mut response = client
            .get(url)
            .send()
            .await
            .map_err(|error| format!("could not reach {url}: {error}"))?;
        if !response.status().is_success() {
            return Err(format!("{url} answered {}", response.status()));
        }
        let mut body = Vec::with_capacity(usize::try_from(expected.bytes).unwrap_or(0));
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| format!("download broke off: {error}"))?
        {
            body.extend_from_slice(&chunk);
            if body.len() as u64 > expected.bytes {
                return Err("download is larger than expected".into());
            }
            self.active().insert(
                model,
                DenoiseModelState::Downloading {
                    received: body.len() as u64,
                },
            );
        }
        let digest: String = Sha256::digest(&body)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if digest != expected.sha256 {
            return Err("download does not match its checksum".into());
        }
        Ok(body)
    }
}

async fn install(
    models: Arc<DenoiseModels>,
    model: DenoiseModel,
    bytes: Vec<u8>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || models.install(model, &bytes))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_model_starts_missing_without_a_data_directory() {
        let statuses = Downloads::default().statuses(&DenoiseModels::default());
        assert_eq!(statuses.len(), DenoiseModel::ALL.len());
        assert!(
            statuses
                .iter()
                .all(|status| status.state == DenoiseModelState::Missing && status.bytes > 0)
        );
    }
}
