use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use sdrmm_channels::{
    Decoder,
    array_processor::{ProcessorFactory, install_processors, processor_refusal},
    encoder::{Encoder, encoder_refusal, install_encoders},
    install_plugin_kinds, install_plugins, plugin_refusal,
};
use sdrmm_wire::KindSpec;

use crate::{
    PluginError,
    cache::ModuleCache,
    decoder::WasmDecoder,
    encoder::WasmEncoder,
    kinds::plugin_kinds,
    module::{PluginModule, plugin_name},
    processor::WasmProcessors,
};

/// Quiet time after the last change before the directory is read again, so a plugin still
/// being written is not loaded half done.
const SETTLE: Duration = Duration::from_millis(300);

/// Every `.wasm` in `dir`, compiled without the cache, with the reason for each file that
/// would not load.
#[must_use]
pub fn load_dir(dir: &Path) -> Scan {
    PluginDir::new(dir.to_owned()).with_cache(None).scan()
}

/// What one pass over the plugins directory changed.
#[derive(Debug, Default)]
pub struct Reload {
    /// Decoders and processors now on a new build: their running channels and processors still
    /// hold the old one.
    pub replaced: Vec<String>,
    pub errors: Vec<PluginError>,
}

/// Everything one pass over the directory loaded.
#[derive(Default)]
pub struct Scan {
    pub decoders: Vec<Arc<dyn Decoder>>,
    pub encoders: Vec<Arc<dyn Encoder>>,
    pub processors: Vec<Arc<dyn ProcessorFactory>>,
    pub kinds: Vec<KindSpec>,
    pub errors: Vec<PluginError>,
}

#[derive(Clone)]
struct Loaded {
    name: String,
    fingerprint: u64,
    decoder: Option<Arc<dyn Decoder>>,
    encoder: Option<Arc<dyn Encoder>>,
    processor: Option<Arc<dyn ProcessorFactory>>,
    kinds: Vec<KindSpec>,
}

/// The type ids one scan has given out so far, each with the plugin that took it.
#[derive(Default)]
struct Taken {
    decoders: Vec<(String, String)>,
    encoders: Vec<(String, String)>,
    processors: Vec<(String, String)>,
}

impl Taken {
    /// Gives `loaded` its types, or refuses the whole plugin when one is built in or another
    /// plugin took it first.
    fn take(&mut self, loaded: &Loaded) -> Result<(), PluginError> {
        let refusal = loaded
            .decoder
            .as_ref()
            .and_then(|decoder| {
                let type_id = &decoder.descriptor().type_id;
                plugin_refusal(type_id, &ids(&self.decoders))
                    .map(|refusal| explain(&refusal, type_id, &self.decoders))
            })
            .or_else(|| {
                let type_id = &loaded.encoder.as_ref()?.descriptor().type_id;
                encoder_refusal(type_id, &ids(&self.encoders))
                    .map(|refusal| explain(&refusal, type_id, &self.encoders))
            })
            .or_else(|| {
                let kind = loaded.processor.as_ref()?.kind();
                processor_refusal(kind, &ids(&self.processors))
                    .map(|refusal| explain(&refusal, &kind.type_id, &self.processors))
            });
        if let Some(error) = refusal {
            return Err(PluginError::Load {
                plugin: loaded.name.clone(),
                error,
            });
        }
        if let Some(decoder) = &loaded.decoder {
            let type_id = decoder.descriptor().type_id.clone();
            self.decoders.push((type_id, loaded.name.clone()));
        }
        if let Some(encoder) = &loaded.encoder {
            let type_id = encoder.descriptor().type_id.clone();
            self.encoders.push((type_id, loaded.name.clone()));
        }
        if let Some(processor) = &loaded.processor {
            let kind = processor.kind().type_id.clone();
            self.processors.push((kind, loaded.name.clone()));
        }
        Ok(())
    }
}

fn ids(taken: &[(String, String)]) -> Vec<&str> {
    taken.iter().map(|(id, _)| id.as_str()).collect()
}

/// `refusal`, naming the plugin that took `type_id` first when another one did.
fn explain(refusal: &impl std::fmt::Display, type_id: &str, taken: &[(String, String)]) -> String {
    let owner = taken.iter().find(|(id, _)| id == type_id);
    owner.map_or_else(
        || refusal.to_string(),
        |(_, plugin)| format!("{refusal}, {plugin} has it"),
    )
}

/// A plugins directory and the decoders last loaded from it, so a file that did not change
/// keeps its decoder and its running channels.
pub struct PluginDir {
    dir: PathBuf,
    cache: Option<ModuleCache>,
    loaded: HashMap<PathBuf, Loaded>,
}

fn fingerprint(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

impl PluginDir {
    /// `dir`, its plugins compiled once into the user's cache.
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            cache: ModuleCache::user(),
            loaded: HashMap::new(),
        }
    }

    /// Keeps compiled plugins in `cache` instead, or nowhere.
    #[must_use]
    pub fn with_cache(mut self, cache: Option<ModuleCache>) -> Self {
        self.cache = cache;
        self
    }

    fn paths(&self) -> Result<Vec<PathBuf>, PluginError> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(PluginError::Dir(error.to_string())),
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "wasm"))
            .collect();
        paths.sort();
        Ok(paths)
    }

    fn load(&mut self, path: PathBuf) -> Result<Loaded, PluginError> {
        let bytes = std::fs::read(&path).map_err(|error| PluginError::Load {
            plugin: plugin_name(&path),
            error: error.to_string(),
        })?;
        let fingerprint = fingerprint(&bytes);
        if let Some(loaded) = self.loaded.get(&path)
            && loaded.fingerprint == fingerprint
        {
            return Ok(loaded.clone());
        }
        let name = plugin_name(&path);
        let module = PluginModule::from_bytes(name.clone(), &bytes, self.cache.as_ref())?;
        let processor = WasmProcessors::of(&module)
            .map(|processors| Arc::new(processors) as Arc<dyn ProcessorFactory>);
        let loaded = Loaded {
            name,
            fingerprint,
            decoder: WasmDecoder::of(&module).map(|decoder| Arc::new(decoder) as Arc<dyn Decoder>),
            encoder: WasmEncoder::of(&module).map(|encoder| Arc::new(encoder) as Arc<dyn Encoder>),
            processor,
            kinds: plugin_kinds(&module),
        };
        self.loaded.insert(path, loaded.clone());
        Ok(loaded)
    }

    /// Every `.wasm` in the directory as whatever it brings, reusing each one whose file is
    /// unchanged. A plugin taking a built-in's type or one an earlier file took is left
    /// out with an error; the others load.
    pub fn scan(&mut self) -> Scan {
        let mut scan = Scan::default();
        let paths = match self.paths() {
            Ok(paths) => paths,
            Err(error) => {
                scan.errors.push(error);
                return scan;
            }
        };
        self.loaded.retain(|path, _| paths.contains(path));
        let mut taken = Taken::default();
        for path in paths {
            let admitted = self
                .load(path)
                .and_then(|loaded| taken.take(&loaded).map(|()| loaded));
            match admitted {
                Ok(loaded) => {
                    scan.decoders.extend(loaded.decoder);
                    scan.encoders.extend(loaded.encoder);
                    scan.processors.extend(loaded.processor);
                    scan.kinds.extend(loaded.kinds);
                }
                Err(error) => scan.errors.push(error),
            }
        }
        scan
    }

    /// Loads the directory into the registries, logging every plugin that failed.
    pub fn reload(&mut self) -> Reload {
        let Scan {
            decoders,
            encoders,
            processors,
            kinds,
            mut errors,
        } = self.scan();
        let count = self.loaded.len();
        install_plugin_kinds(kinds);
        let mut replaced = install(install_plugins(decoders), &mut errors);
        install(install_encoders(encoders).map(|()| Vec::new()), &mut errors);
        replaced.extend(install(install_processors(processors), &mut errors));
        tracing::info!(count, dir = %self.dir.display(), "plugins loaded");
        for error in &errors {
            tracing::error!(%error, "plugin not loaded");
        }
        Reload { replaced, errors }
    }
}

fn install(
    installed: Result<Vec<String>, impl std::fmt::Display>,
    errors: &mut Vec<PluginError>,
) -> Vec<String> {
    installed.unwrap_or_else(|error| {
        errors.push(PluginError::Install(error.to_string()));
        Vec::new()
    })
}

/// Keeps the registry in step with the plugins directory for as long as it is held.
pub struct PluginWatcher {
    _watcher: RecommendedWatcher,
}

impl PluginWatcher {
    /// Loads `dir` now and again whenever a file in it changes, handing `reloaded` the type ids
    /// of every decoder and processor a changed file rebuilt, for what runs them to reopen.
    pub fn start(
        dir: PathBuf,
        reloaded: impl Fn(&[String]) + Send + 'static,
    ) -> Result<Self, PluginError> {
        std::fs::create_dir_all(&dir).map_err(|error| PluginError::Dir(error.to_string()))?;
        let mut plugins = PluginDir::new(dir.clone());
        plugins.reload();
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = tx.send(event);
        })
        .map_err(|error| PluginError::Dir(error.to_string()))?;
        watcher
            .watch(&dir, RecursiveMode::NonRecursive)
            .map_err(|error| PluginError::Dir(error.to_string()))?;
        thread::Builder::new()
            .name("plugin-watch".to_owned())
            .spawn(move || settle_and_reload(&mut plugins, &rx, &reloaded))
            .map_err(|error| PluginError::Dir(error.to_string()))?;
        Ok(Self { _watcher: watcher })
    }
}

fn settle_and_reload(
    plugins: &mut PluginDir,
    rx: &mpsc::Receiver<notify::Result<notify::Event>>,
    reloaded: &dyn Fn(&[String]),
) {
    while rx.recv().is_ok() {
        while rx.recv_timeout(SETTLE).is_ok() {}
        reloaded(&plugins.reload().replaced);
    }
}
