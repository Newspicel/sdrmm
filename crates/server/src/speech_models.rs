//! Speech models: downloaded once into the data directory, loaded on first use and kept.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock},
    time::Duration,
};

use reqwest::Client;
use sdrmm_dsp::audio::speech::Parakeet;
use sdrmm_wire::{ModelState, SpeechModel, SpeechModelStatus};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

/// How long the download may stall before it counts as broken; the whole file takes minutes.
const STALL: Duration = Duration::from_secs(60);
const CONNECT: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(crate) struct SpeechModels {
    dir: RwLock<Option<PathBuf>>,
    active: Mutex<HashMap<SpeechModel, ModelState>>,
    loaded: Mutex<HashMap<SpeechModel, Arc<Parakeet>>>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl SpeechModels {
    pub(crate) fn set_dir(&self, dir: PathBuf) {
        *self.dir.write().unwrap_or_else(PoisonError::into_inner) = Some(dir);
        locked(&self.loaded).clear();
    }

    pub(crate) fn path(&self, model: SpeechModel) -> Option<PathBuf> {
        self.dir
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|dir| dir.join(model.file_name()))
    }

    pub(crate) fn installed(&self, model: SpeechModel) -> bool {
        self.path(model).is_some_and(|path| path.is_file())
    }

    pub(crate) fn statuses(&self) -> Vec<SpeechModelStatus> {
        let active = locked(&self.active);
        SpeechModel::ALL
            .into_iter()
            .map(|model| SpeechModelStatus {
                model,
                bytes: model.artifact().bytes,
                state: match active.get(&model) {
                    Some(state) => state.clone(),
                    None if self.installed(model) => ModelState::Ready,
                    None => ModelState::Missing,
                },
            })
            .collect()
    }

    /// Starts fetching `model`; false while it is already on its way.
    pub(crate) fn start(self: &Arc<Self>, model: SpeechModel) -> bool {
        {
            let mut active = locked(&self.active);
            if matches!(active.get(&model), Some(ModelState::Downloading { .. })) {
                return false;
            }
            active.insert(model, ModelState::Downloading { received: 0 });
        }
        let models = Arc::clone(self);
        tokio::spawn(async move {
            let outcome = models.fetch(model).await;
            let mut active = locked(&models.active);
            match outcome {
                Ok(()) => {
                    active.remove(&model);
                }
                Err(error) => {
                    tracing::warn!(model = model.name(), %error, "speech model download failed");
                    active.insert(model, ModelState::Failed { error });
                }
            }
        });
        true
    }

    pub(crate) fn remove(&self, model: SpeechModel) -> Result<(), String> {
        {
            let mut active = locked(&self.active);
            if matches!(active.get(&model), Some(ModelState::Failed { .. })) {
                active.remove(&model);
            }
        }
        locked(&self.loaded).remove(&model);
        match self.path(model).filter(|path| path.is_file()) {
            Some(path) => std::fs::remove_file(path).map_err(|error| error.to_string()),
            None => Ok(()),
        }
    }

    /// The model ready to run, read from disk the first time. Blocks while it reads.
    pub(crate) fn load(&self, model: SpeechModel) -> Result<Arc<Parakeet>, String> {
        if let Some(loaded) = locked(&self.loaded).get(&model) {
            return Ok(Arc::clone(loaded));
        }
        let path = self
            .path(model)
            .filter(|path| path.is_file())
            .ok_or_else(|| format!("{} is not downloaded", model.label()))?;
        let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let parakeet = Arc::new(Parakeet::load(bytes).map_err(|error| error.to_string())?);
        locked(&self.loaded).insert(model, Arc::clone(&parakeet));
        Ok(parakeet)
    }

    async fn fetch(&self, model: SpeechModel) -> Result<(), String> {
        let path = self
            .path(model)
            .ok_or_else(|| "no data directory for models".to_owned())?;
        if let Some(dir) = path.parent() {
            tokio::fs::create_dir_all(dir)
                .await
                .map_err(|error| error.to_string())?;
        }
        let partial = path.with_extension("part");
        let fetched = self.stream_to(model, &partial).await;
        if let Err(error) = fetched {
            let _ = tokio::fs::remove_file(&partial).await;
            return Err(error);
        }
        tokio::fs::rename(&partial, &path)
            .await
            .map_err(|error| error.to_string())
    }

    async fn stream_to(&self, model: SpeechModel, partial: &std::path::Path) -> Result<(), String> {
        let expected = model.artifact();
        let url = model.url();
        let client = Client::builder()
            .connect_timeout(CONNECT)
            .read_timeout(STALL)
            .build()
            .map_err(|error| format!("no HTTP client: {error}"))?;
        let mut response = client
            .get(&url)
            .send()
            .await
            .map_err(|error| format!("could not reach {url}: {error}"))?;
        if !response.status().is_success() {
            return Err(format!("{url} answered {}", response.status()));
        }
        let mut file = tokio::fs::File::create(partial)
            .await
            .map_err(|error| error.to_string())?;
        let mut digest = Sha256::new();
        let mut received = 0u64;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| format!("download broke off: {error}"))?
        {
            received += chunk.len() as u64;
            if received > expected.bytes {
                return Err("download is larger than expected".into());
            }
            digest.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(|error| error.to_string())?;
            locked(&self.active).insert(model, ModelState::Downloading { received });
        }
        file.flush().await.map_err(|error| error.to_string())?;
        let sum: String = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if received != expected.bytes || sum != expected.sha256 {
            return Err("download does not match its checksum".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_model_starts_missing_without_a_data_directory() {
        let statuses = SpeechModels::default().statuses();
        assert_eq!(statuses.len(), SpeechModel::ALL.len());
        assert!(
            statuses
                .iter()
                .all(|status| status.state == ModelState::Missing)
        );
    }

    #[test]
    fn a_model_that_is_not_there_says_so() {
        let models = SpeechModels::default();
        let dir = tempfile::tempdir().unwrap();
        models.set_dir(dir.path().to_path_buf());
        let Err(error) = models.load(SpeechModel::ParakeetTdtV3) else {
            panic!("a missing model loaded");
        };
        assert!(error.contains("not downloaded"), "{error}");
        assert_eq!(models.remove(SpeechModel::ParakeetTdtV3), Ok(()));
    }
}
