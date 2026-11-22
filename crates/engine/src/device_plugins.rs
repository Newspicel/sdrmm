//! Device plugins: native libraries in the plugins folder that each bring one radio driver.

use std::path::{Path, PathBuf};

use sdrmm_device::{DeviceDriver, DeviceRegistry};
use sdrmm_device_plugin::{abi::DEVICE_ABI_VERSION, host};
use sdrmm_wire::{CheckStatus, DeviceBackend, DoctorCheck};

use crate::PLUGIN_PRIORITY;

const LIBRARY_EXTENSION: &str = std::env::consts::DLL_EXTENSION;

enum Found {
    Loaded {
        file: String,
        driver: Box<dyn DeviceDriver>,
        about: String,
    },
    Failed {
        file: String,
        error: String,
    },
}

/// The device plugins found in a folder, loaded but not yet registered.
#[derive(Default)]
pub struct DevicePlugins {
    found: Vec<Found>,
}

impl DevicePlugins {
    /// Every library in `dir` with this platform's extension, each loaded or noted with why not.
    /// A missing folder holds no plugins.
    #[must_use]
    pub fn load(dir: &Path) -> Self {
        let found = libraries(dir)
            .into_iter()
            .map(|path| load_one(&path))
            .collect();
        Self { found }
    }

    /// A driver already in this process, registered as if a plugin brought it.
    #[must_use]
    pub fn with_driver(mut self, file: &str, driver: Box<dyn DeviceDriver>) -> Self {
        self.found.push(Found::Loaded {
            file: file.to_owned(),
            driver,
            about: "in process".to_owned(),
        });
        self
    }

    /// Registers each loaded driver whose backend nobody in `registry` or `reserved` has yet, and
    /// reports every plugin in the doctor.
    pub fn register(self, registry: &mut DeviceRegistry, reserved: &[DeviceBackend]) {
        for found in self.found {
            let check = match found {
                Found::Failed { file, error } => {
                    check(&file, CheckStatus::Fail, error, Some(REBUILD))
                }
                Found::Loaded {
                    file,
                    driver,
                    about,
                } => {
                    let backend = driver.id();
                    let taken = reserved.contains(&backend)
                        || registry.driver_ids().iter().any(|(_, id)| *id == backend);
                    if taken {
                        check(
                            &file,
                            CheckStatus::Fail,
                            format!("backend {backend} is already taken, so this plugin is off"),
                            Some("remove one of the two, or give the plugin its own backend id"),
                        )
                    } else {
                        registry.register(PLUGIN_PRIORITY, driver);
                        check(
                            &file,
                            CheckStatus::Ok,
                            format!("backend {backend} · {about}"),
                            None,
                        )
                    }
                }
            };
            registry.add_check(check);
        }
    }
}

const REBUILD: &str = "rebuild the plugin against this sdr-- version, or remove the file";

fn libraries(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension == LIBRARY_EXTENSION)
        })
        .collect();
    paths.sort();
    paths
}

fn load_one(path: &Path) -> Found {
    let file = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    match host::load(path) {
        Ok(loaded) => {
            let build = if loaded.sdk_build.is_empty() {
                String::new()
            } else {
                format!(" · built on sdr-- {}", loaded.sdk_build)
            };
            tracing::info!(%file, backend = %loaded.spec.backend, "device plugin loaded");
            Found::Loaded {
                file,
                driver: loaded.driver,
                about: format!("{} · ABI {DEVICE_ABI_VERSION}{build}", loaded.plugin),
            }
        }
        Err(error) => {
            tracing::error!(%file, %error, "device plugin not loaded");
            Found::Failed {
                file,
                error: error.to_string(),
            }
        }
    }
}

fn check(file: &str, status: CheckStatus, detail: String, hint: Option<&str>) -> DoctorCheck {
    DoctorCheck {
        id: format!("plugins.device.{file}"),
        name: format!("Device plugin {file}"),
        status,
        detail,
        hint: hint.map(str::to_owned),
    }
}
