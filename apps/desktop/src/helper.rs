#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

/// Chromium's renderer, GPU and utility processes. Kept apart from the app so each of them
/// starts without loading the radio engine.
fn main() -> ExitCode {
    let args = cef::args::Args::new();
    #[cfg(target_os = "macos")]
    let _sandbox = {
        let mut sandbox = cef::sandbox::Sandbox::new();
        sandbox.initialize(args.as_main_args());
        sandbox
    };
    #[cfg(target_os = "macos")]
    let _library = {
        let Ok(exe) = std::env::current_exe() else {
            return ExitCode::FAILURE;
        };
        let loader = cef::library_loader::LibraryLoader::new(&exe, true);
        if !loader.load() {
            return ExitCode::FAILURE;
        }
        loader
    };
    cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    let code = cef::execute_process(
        Some(args.as_main_args()),
        None::<&mut cef::App>,
        std::ptr::null_mut(),
    );
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
