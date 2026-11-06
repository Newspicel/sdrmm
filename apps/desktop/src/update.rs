use anyhow::{Context, Result};
use cargo_packager_updater::{Config, Update, WindowsConfig, WindowsUpdateInstallMode};
use semver::Version;
use tokio::runtime::Handle;
use url::Url;

use crate::{dialog, exit::Exit, shell};

const RELEASE: Option<&str> = option_env!("SDRMM_RELEASE");
const RELEASES: &str = "https://github.com/Newspicel/sdrmm/releases";
const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEQ2QkUwNzFEODc5MzYzRTgKUldUb1k1T0hIUWUrMXQ5M21PK2VmYUJFekgvZnoyS0U1dm9TQ1RPNW5kTnVRTkRaOU9TZUFMelcK";
const ENDPOINTS: [&str; 2] = [
    "https://downloads.sdrmm.com/releases/latest.json?version={{current_version}}&target={{target}}&arch={{arch}}",
    "https://github.com/newspicel/sdrmm/releases/latest/download/latest.json",
];

pub fn spawn(runtime: &Handle) {
    if let Some(skip) = skipped() {
        tracing::info!("update check skipped: {}", skip.reason());
        return;
    }
    runtime.spawn(async {
        if let Err(e) = check().await {
            tracing::warn!("update check failed: {e:#}");
        }
    });
}

async fn check() -> Result<()> {
    let Some(update) = tokio::task::spawn_blocking(find).await?? else {
        tracing::debug!("no update available");
        return Ok(());
    };
    tracing::info!(
        "update available: {} -> {}",
        update.current_version,
        update.version
    );
    if !prompt(&update.version).await {
        return Ok(());
    }
    tokio::task::spawn_blocking(move || update.download_and_install()).await??;
    shell::quit(Exit::Relaunch);
    Ok(())
}

async fn prompt(version: &str) -> bool {
    dialog::confirm(
        "Update available",
        format!(
            "SDRmm {version} is available.\n\nInstalling restarts the app, which stops any \
             recording or stream that is running."
        ),
        "Install and restart",
        "Later",
    )
    .await
}

/// Called before the window exists, when the database belongs to a newer release.
pub fn update_now() -> Exit {
    match skipped() {
        Some(Skip::Packaged) => {
            tracing::info!("update skipped: {}", Skip::Packaged.reason());
            dialog::inform("Update", "Update SDRmm with your package manager.");
            Exit::Quit
        }
        Some(Skip::Unreleased) => {
            tracing::info!("update skipped: {}", Skip::Unreleased.reason());
            open_releases();
            Exit::Quit
        }
        None => match install() {
            Ok(true) => Exit::Relaunch,
            Ok(false) => {
                open_releases();
                Exit::Quit
            }
            Err(e) => {
                tracing::warn!("update failed: {e:#}");
                open_releases();
                Exit::Quit
            }
        },
    }
}

fn install() -> Result<bool> {
    let Some(update) = find()? else {
        return Ok(false);
    };
    update.download_and_install()?;
    Ok(true)
}

fn find() -> Result<Option<Update>> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
    Ok(cargo_packager_updater::check_update(current, config()?)?)
}

fn config() -> Result<Config> {
    let endpoints = ENDPOINTS
        .iter()
        .map(|endpoint| Url::parse(endpoint).with_context(|| format!("endpoint {endpoint}")))
        .collect::<Result<_>>()?;
    Ok(Config {
        endpoints,
        pubkey: PUBKEY.to_owned(),
        windows: Some(WindowsConfig {
            installer_args: None,
            install_mode: Some(WindowsUpdateInstallMode::Passive),
        }),
    })
}

fn open_releases() {
    if let Err(e) = opener::open_browser(RELEASES) {
        tracing::warn!("could not open {RELEASES}: {e}");
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Skip {
    Unreleased,
    Packaged,
}

impl Skip {
    fn reason(&self) -> &'static str {
        match self {
            Self::Unreleased => {
                "not built from a release tag; dev and nightly builds never replace themselves"
            }
            Self::Packaged => "installed by a package manager, which owns updates",
        }
    }
}

fn skipped() -> Option<Skip> {
    skip(RELEASE, packaged())
}

fn skip(release: Option<&str>, packaged: bool) -> Option<Skip> {
    if release.is_none_or(str::is_empty) {
        return Some(Skip::Unreleased);
    }
    packaged.then_some(Skip::Packaged)
}

#[cfg(target_os = "linux")]
fn packaged() -> bool {
    std::env::var_os("APPIMAGE").is_none()
}

#[cfg(not(target_os = "linux"))]
fn packaged() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreleased_builds_never_update() {
        assert_eq!(skip(None, false), Some(Skip::Unreleased));
        assert_eq!(skip(Some(""), true), Some(Skip::Unreleased));
    }

    #[test]
    fn packaged_releases_defer_to_the_package_manager() {
        assert_eq!(skip(Some("1"), true), Some(Skip::Packaged));
    }

    #[test]
    fn standalone_releases_update_themselves() {
        assert_eq!(skip(Some("1"), false), None);
    }

    #[test]
    fn the_updater_config_parses() {
        let config = config().unwrap();
        assert_eq!(config.endpoints.len(), ENDPOINTS.len());
    }
}
