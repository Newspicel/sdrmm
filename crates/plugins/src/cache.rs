//! Plugins compiled to native code once and kept on disk, so the next start loads them without
//! compiling again.

use std::{
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    io::{self, Write},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use wasmtime::{Engine, Module};

const MAGIC: &[u8; 8] = b"sdrmmcw1";
const DIGEST_BYTES: usize = 32;
const EXTENSION: &str = "cwasm";

/// A directory of compiled plugins. Each file is named for its plugin and a key over the wasm
/// and the engine that compiled it, so a changed plugin, engine config or wasmtime release misses
/// instead of loading stale code.
///
/// Loading native code trusts the directory: whoever can write it can already run code as this
/// user. Every file carries a digest of its code, so a torn or corrupted write is dropped and
/// compiled again, and wasmtime itself refuses code from another build.
#[derive(Clone, Debug)]
pub struct ModuleCache {
    dir: PathBuf,
}

impl ModuleCache {
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// `sdrmm/plugins` in the user's cache directory, when the system names one.
    #[must_use]
    pub fn user() -> Option<Self> {
        dirs::cache_dir().map(|dir| Self::new(dir.join("sdrmm").join("plugins")))
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `wasm` compiled for `engine`, from the cache when it holds it, else compiled and stored.
    /// A cache that cannot be read or written costs a compile and a warning, never the plugin.
    pub(crate) fn module(
        &self,
        engine: &Engine,
        plugin: &str,
        wasm: &[u8],
    ) -> Result<Module, String> {
        let path = self.path(engine, plugin, wasm);
        match load(engine, &path) {
            Ok(Some(module)) => return Ok(module),
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, plugin, "cached plugin build dropped");
                let _ = fs::remove_file(&path);
            }
        }
        let module = Module::new(engine, wasm).map_err(|error| error.to_string())?;
        if let Err(error) = self.store(&path, plugin, &module) {
            tracing::warn!(%error, plugin, "plugin build not cached");
        }
        Ok(module)
    }

    fn path(&self, engine: &Engine, plugin: &str, wasm: &[u8]) -> PathBuf {
        let mut compat = DefaultHasher::new();
        engine.precompile_compatibility_hash().hash(&mut compat);
        let mut key = Sha256::new();
        key.update(compat.finish().to_le_bytes());
        key.update(wasm);
        let name = format!("{}.{}.{EXTENSION}", stem(plugin), hex(&key.finalize()));
        self.dir.join(name)
    }

    fn store(&self, path: &Path, plugin: &str, module: &Module) -> Result<(), String> {
        let code = module.serialize().map_err(|error| error.to_string())?;
        create_private_dir(&self.dir).map_err(|error| error.to_string())?;
        self.prune(plugin, path);
        let mut file =
            tempfile::NamedTempFile::new_in(&self.dir).map_err(|error| error.to_string())?;
        file.write_all(MAGIC)
            .and_then(|()| file.write_all(&Sha256::digest(&code)))
            .and_then(|()| file.write_all(&code))
            .map_err(|error| error.to_string())?;
        file.persist(path).map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Removes the builds `plugin` left before, so the cache keeps one per plugin.
    fn prune(&self, plugin: &str, keep: &Path) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        let stem = stem(plugin);
        for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
            if path != keep && entry_plugin(&path).is_some_and(|owner| owner == stem) {
                let _ = fs::remove_file(path);
            }
        }
    }
}

/// The cached module at `path`, `None` when there is none.
///
/// Deserializing is unsafe because wasmtime runs the bytes as native code. Only files this
/// cache wrote are named like this, the digest proves the code is whole, and the bytes are read
/// once into memory, so a later change to the file cannot reach the loaded module.
fn load(engine: &Engine, path: &Path) -> Result<Option<Module>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let code = verified(&bytes).ok_or_else(|| "digest mismatch".to_owned())?;
    let module = unsafe { Module::deserialize(engine, code) };
    module.map(Some).map_err(|error| error.to_string())
}

fn verified(bytes: &[u8]) -> Option<&[u8]> {
    let rest = bytes.strip_prefix(MAGIC)?;
    let (digest, code) = rest.split_at_checked(DIGEST_BYTES)?;
    (Sha256::digest(code).as_slice() == digest).then_some(code)
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// The plugin a cache file belongs to: its name up to the key.
fn entry_plugin(path: &Path) -> Option<&str> {
    let name = path.file_name()?.to_str()?;
    let (owner, _key) = name
        .strip_suffix(EXTENSION)?
        .strip_suffix('.')?
        .rsplit_once('.')?;
    Some(owner)
}

/// `plugin` reduced to characters every file system takes.
fn stem(plugin: &str) -> String {
    plugin
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_names_its_plugin_and_only_whole_code_passes() {
        let path = Path::new("/c/my_plugin.abc123.cwasm");
        assert_eq!(entry_plugin(path), Some("my_plugin"));
        assert_eq!(entry_plugin(Path::new("/c/my_plugin.wasm")), None);
        assert_eq!(stem("a.b/c"), "a_b_c");

        let code = b"native".as_slice();
        let mut file = MAGIC.to_vec();
        file.extend_from_slice(&Sha256::digest(code));
        file.extend_from_slice(code);
        assert_eq!(verified(&file), Some(code));
        let last = file.len() - 1;
        file[last] ^= 1;
        assert_eq!(verified(&file), None);
        assert_eq!(verified(&file[..MAGIC.len() + 4]), None);
    }
}
