//! Loads the example plugin as a real library, the way the server does.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use sdrmm_device::RxSink;
use sdrmm_device_plugin::host::{self, LoadError};
use sdrmm_wire::DeviceBackend;

fn example() -> &'static Path {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| {
        let target = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("device-plugin");
        let status = Command::new(env!("CARGO"))
            .args(["build", "-p", "sdrmm-device-plugin-example", "--target-dir"])
            .arg(&target)
            .status()
            .unwrap_or_else(|error| panic!("cannot run cargo: {error}"));
        assert!(status.success(), "the example plugin does not build");
        target.join("debug").join(format!(
            "{}sdrmm_device_plugin_example.{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_EXTENSION
        ))
    })
}

#[test]
fn the_example_loads_and_streams() {
    let loaded = host::load(example()).unwrap_or_else(|error| panic!("loads: {error}"));
    assert_eq!(loaded.spec.backend, DeviceBackend::new("tone"));
    assert!(loaded.plugin.starts_with("sdrmm-device-plugin-example "));
    let found = loaded.driver.probe();
    assert_eq!(found.len(), 1);
    let mut device = loaded
        .driver
        .open(&found[0])
        .unwrap_or_else(|error| panic!("opens: {error}"));
    let counted = Arc::new(AtomicUsize::new(0));
    let count = counted.clone();
    device
        .rx_start(vec![RxSink::new(move |samples, _| {
            count.fetch_add(samples.len(), Ordering::Relaxed);
        })])
        .unwrap_or_else(|error| panic!("streams: {error}"));
    let start = Instant::now();
    while counted.load(Ordering::Relaxed) < 8_192 && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    device.rx_stop();
    assert!(
        counted.load(Ordering::Relaxed) >= 8_192,
        "the tone never arrived"
    );
}

#[test]
fn a_library_without_the_symbol_is_refused() {
    let system = if cfg!(target_os = "macos") {
        "/usr/lib/libz.1.dylib"
    } else if cfg!(windows) {
        "kernel32.dll"
    } else {
        "libc.so.6"
    };
    assert!(matches!(
        host::load(Path::new(system)),
        Err(LoadError::Symbol)
    ));
}

#[test]
fn a_file_that_is_not_a_library_is_refused() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(&dir).unwrap_or_else(|error| panic!("tmpdir: {error}"));
    let path = dir.join(format!("not-a-plugin.{}", std::env::consts::DLL_EXTENSION));
    std::fs::write(&path, b"just text").unwrap_or_else(|error| panic!("write: {error}"));
    assert!(matches!(host::load(&path), Err(LoadError::Open(_))));
}
