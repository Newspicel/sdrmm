use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError, RwLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use reqwest::Client;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const RETRY_AFTER: Duration = Duration::from_secs(30 * 60);
const USER_AGENT: &str = concat!("SDR--/", env!("CARGO_PKG_VERSION"));

pub(crate) type Parse<T> = fn(&[u8]) -> Result<T, String>;

pub(crate) struct Source {
    pub(crate) file: &'static str,
    pub(crate) url: &'static str,
    pub(crate) params: &'static [(&'static str, &'static str)],
    pub(crate) refresh: Duration,
    pub(crate) max_bytes: usize,
}

pub(crate) struct Snapshot<T> {
    pub(crate) value: T,
    pub(crate) fetched: SystemTime,
}

impl<T> Snapshot<T> {
    pub(crate) fn fetched_unix(&self) -> i64 {
        self.fetched.duration_since(UNIX_EPOCH).map_or(0, |since| {
            i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
        })
    }
}

pub(crate) struct Feed<T> {
    source: Source,
    parse: Parse<T>,
    current: RwLock<Option<Arc<Snapshot<T>>>>,
    failed: Mutex<Option<(Instant, String)>>,
    refreshing: tokio::sync::Mutex<()>,
}

impl<T: Send + Sync + 'static> Feed<T> {
    pub(crate) fn new(source: Source, parse: Parse<T>) -> Self {
        Self {
            source,
            parse,
            current: RwLock::new(None),
            failed: Mutex::new(None),
            refreshing: tokio::sync::Mutex::new(()),
        }
    }

    pub(crate) fn file(&self) -> &'static str {
        self.source.file
    }

    pub(crate) fn current(&self) -> Option<Arc<Snapshot<T>>> {
        self.current
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) async fn get(&self, dir: Option<&Path>) -> Result<Arc<Snapshot<T>>, String> {
        if let Some(current) = self.current() {
            return Ok(current);
        }
        if let Some(error) = self.backing_off(Instant::now()) {
            return Err(error);
        }
        self.refresh(dir).await
    }

    pub(crate) fn due(&self, now: SystemTime) -> bool {
        self.backing_off(Instant::now()).is_none()
            && self
                .current()
                .is_none_or(|current| self.stale(&current, now))
    }

    pub(crate) async fn load(&self, dir: &Path) -> Result<bool, String> {
        let path = dir.join(self.source.file);
        let parse = self.parse;
        let loaded = tokio::task::spawn_blocking(move || read_cached(&path, parse))
            .await
            .map_err(|error| format!("could not read the cached copy: {error}"))??;
        let Some(snapshot) = loaded else {
            return Ok(false);
        };
        self.store(snapshot);
        Ok(true)
    }

    pub(crate) async fn refresh(&self, dir: Option<&Path>) -> Result<Arc<Snapshot<T>>, String> {
        self.refresh_from(self.source.url, dir).await
    }

    async fn refresh_from(
        &self,
        url: &str,
        dir: Option<&Path>,
    ) -> Result<Arc<Snapshot<T>>, String> {
        let _refreshing = self.refreshing.lock().await;
        if let Some(current) = self
            .current()
            .filter(|current| !self.stale(current, SystemTime::now()))
        {
            return Ok(current);
        }
        match self.download(url, dir).await {
            Ok(snapshot) => {
                *self.failed.lock().unwrap_or_else(PoisonError::into_inner) = None;
                Ok(self.store(snapshot))
            }
            Err(error) => {
                *self.failed.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some((Instant::now(), error.clone()));
                Err(error)
            }
        }
    }

    async fn download(&self, url: &str, dir: Option<&Path>) -> Result<Snapshot<T>, String> {
        let body = fetch(url, self.source.params, self.source.max_bytes).await?;
        let parse = self.parse;
        let target = dir.map(|dir| (dir.to_path_buf(), self.source.file));
        let sender = url.to_owned();
        let value = tokio::task::spawn_blocking(move || {
            let value = parse(&body).map_err(|error| format!("{sender} sent {error}"))?;
            if let Some((dir, file)) = target
                && let Err(error) = persist(&dir, file, &body)
            {
                tracing::warn!(%error, file, "could not cache reference data");
            }
            Ok::<T, String>(value)
        })
        .await
        .map_err(|error| format!("could not read {url}: {error}"))??;
        Ok(Snapshot {
            value,
            fetched: SystemTime::now(),
        })
    }

    fn store(&self, snapshot: Snapshot<T>) -> Arc<Snapshot<T>> {
        let snapshot = Arc::new(snapshot);
        *self.current.write().unwrap_or_else(PoisonError::into_inner) = Some(snapshot.clone());
        snapshot
    }

    fn stale(&self, snapshot: &Snapshot<T>, now: SystemTime) -> bool {
        now.duration_since(snapshot.fetched)
            .map_or(true, |age| age >= self.source.refresh)
    }

    fn backing_off(&self, now: Instant) -> Option<String> {
        self.failed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .filter(|(at, _)| now.duration_since(*at) < RETRY_AFTER)
            .map(|(_, error)| error.clone())
    }
}

fn read_cached<T>(path: &Path, parse: Parse<T>) -> Result<Option<Snapshot<T>>, String> {
    let body = match std::fs::read(path) {
        Ok(body) => body,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not read {}: {error}", path.display())),
    };
    let fetched = std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|error| format!("could not date {}: {error}", path.display()))?;
    let value = parse(&body).map_err(|error| format!("{} holds {error}", path.display()))?;
    Ok(Some(Snapshot { value, fetched }))
}

fn persist(dir: &Path, file: &str, body: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let partial: PathBuf = dir.join(format!("{file}.partial"));
    std::fs::write(&partial, body)?;
    std::fs::rename(&partial, dir.join(file))
}

pub(crate) async fn fetch(
    url: &str,
    params: &[(&str, &str)],
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let client = Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| format!("could not build the HTTP client: {error}"))?;
    let address = reqwest::Url::parse_with_params(url, params)
        .map_err(|error| format!("could not build a request to {url}: {error}"))?;
    let mut response = client
        .get(address)
        .send()
        .await
        .map_err(|error| format!("could not reach {url}: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("{url} answered {status}"));
    }
    let too_big = || format!("{url} sent more than {max_bytes} bytes");
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(too_big());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("could not read {url}: {error}"))?
    {
        if body.len() + chunk.len() > max_bytes {
            return Err(too_big());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests;
