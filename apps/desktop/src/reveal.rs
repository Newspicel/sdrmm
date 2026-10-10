use std::{io, path::Path};

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

#[derive(Debug)]
pub struct Shell(pub AppHandle);

impl sdrmm_server::NativeShell for Shell {
    fn reveal(&self, path: &Path) -> io::Result<()> {
        let shown = if path.is_dir() {
            tauri_plugin_opener::open_path(path, None::<&str>)
        } else {
            tauri_plugin_opener::reveal_item_in_dir(path)
        };
        shown.map_err(io::Error::other)
    }

    fn notify(&self, title: &str, body: &str) -> io::Result<()> {
        self.0
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map_err(io::Error::other)
    }
}
