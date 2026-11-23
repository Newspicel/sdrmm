//! The watcher reloads once per change and then rests, even where reading the directory is
//! itself a filesystem event.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use sdrmm_plugins::PluginWatcher;

#[test]
fn one_change_is_one_reload() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("watch-settles");
    let _ = std::fs::remove_dir_all(&dir);
    let reloads = Arc::new(AtomicUsize::new(0));
    let counter = reloads.clone();
    let _watcher = PluginWatcher::start(&dir, move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap_or_else(|error| panic!("{error}"));

    std::fs::write(dir.join("notes.txt"), "x").unwrap_or_else(|error| panic!("{error}"));
    let deadline = Instant::now() + Duration::from_secs(5);
    while reloads.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(reloads.load(Ordering::SeqCst), 1, "the change was seen");
    thread::sleep(Duration::from_millis(1500));
    assert_eq!(
        reloads.load(Ordering::SeqCst),
        1,
        "the reload did not trigger itself"
    );
}
