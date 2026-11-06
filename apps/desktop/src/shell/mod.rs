use std::path::Path;

use anyhow::{Result, ensure};
use cef::{CefString, LogSeverity, Settings};
use tokio::runtime::Handle;
use url::Url;

use crate::exit::{self, Exit};

mod app;
mod client;
#[cfg(target_os = "macos")]
mod mac;
mod window;

/// What the browser process needs once Chromium is up.
#[derive(Clone, Debug)]
pub struct Start {
    pub url: Url,
    pub runtime: Handle,
}

/// Keeps the CEF library loaded; on macOS it is opened from the bundle at runtime.
pub struct Library {
    #[cfg(target_os = "macos")]
    _loader: cef::library_loader::LibraryLoader,
}

/// Loads CEF and claims the process for it. On macOS this also installs the application class
/// Chromium needs, which must happen before anything else touches `NSApp`.
pub fn load() -> Result<Library> {
    let exe = std::env::current_exe()?;
    #[cfg(target_os = "macos")]
    let library = {
        crate::paths::framework(&exe)?;
        let loader = cef::library_loader::LibraryLoader::new(&exe, false);
        ensure!(loader.load(), "could not load the Chromium framework");
        Library { _loader: loader }
    };
    #[cfg(not(target_os = "macos"))]
    let library = {
        crate::paths::runtime(&exe)?;
        Library {}
    };
    cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    #[cfg(target_os = "macos")]
    mac::install()?;
    Ok(library)
}

/// Runs the window until it closes.
pub fn run(start: Start, cache: &Path) -> Result<()> {
    let args = cef::args::Args::new();
    let settings = settings(cache)?;
    let mut app = app::ShellApp::new(start);
    let started = cef::initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    ensure!(
        started == 1,
        "Chromium did not start: is SDRmm already running?"
    );
    cef::run_message_loop();
    cef::shutdown();
    Ok(())
}

/// Closes the window from any thread; the app exits once Chromium lets go of it.
pub fn quit(exit: Exit) {
    if exit == Exit::Relaunch {
        exit::relaunch_on_close();
    }
    window::close();
}

fn settings(cache: &Path) -> Result<Settings> {
    let profile = cache.join("browser");
    std::fs::create_dir_all(&profile)?;
    let settings = Settings {
        no_sandbox: i32::from(!cfg!(target_os = "macos")),
        root_cache_path: path(&profile),
        cache_path: path(&profile),
        persist_session_cookies: 1,
        log_file: path(&cache.join("chromium.log")),
        log_severity: LogSeverity::WARNING,
        ..Default::default()
    };
    #[cfg(not(target_os = "macos"))]
    let settings = beside_runtime(settings)?;
    Ok(settings)
}

#[cfg(not(target_os = "macos"))]
fn beside_runtime(settings: Settings) -> Result<Settings> {
    let runtime = crate::paths::runtime(&std::env::current_exe()?)?;
    Ok(Settings {
        browser_subprocess_path: path(&runtime.helper),
        resources_dir_path: path(&runtime.resources),
        locales_dir_path: path(&runtime.locales),
        ..settings
    })
}

fn path(path: &Path) -> CefString {
    CefString::from(path.to_string_lossy().as_ref())
}
