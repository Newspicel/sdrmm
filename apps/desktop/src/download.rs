use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
};

use tauri::{Runtime, Url, Webview, webview::DownloadEvent};
use tauri_plugin_dialog::DialogExt;

#[derive(Debug, Default)]
struct Pending(Mutex<HashMap<Url, PathBuf>>);

impl Pending {
    fn start(&self, url: Url, destination: PathBuf) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(url, destination);
    }

    fn finish(&self, url: &Url) -> Option<PathBuf> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(url)
    }
}

pub fn handler<R: Runtime>() -> impl Fn(Webview<R>, DownloadEvent<'_>) -> bool + Send + Sync {
    let pending = Pending::default();
    move |webview, event| match event {
        DownloadEvent::Requested { url, destination } => match ask(&webview, destination) {
            Some(chosen) => {
                destination.clone_from(&chosen);
                pending.start(url, chosen);
                true
            }
            None => false,
        },
        DownloadEvent::Finished { url, success, .. } => {
            if let Some(path) = pending.finish(&url)
                && !success
            {
                failed(&webview, &path);
            }
            true
        }
        _ => true,
    }
}

fn ask<R: Runtime>(webview: &Webview<R>, suggested: &Path) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_parent(&webview.window());
    if let Some(name) = suggested.file_name() {
        dialog = dialog.set_file_name(name.to_string_lossy());
    }
    if let Some(dir) = suggested.parent().filter(|dir| dir.is_dir()) {
        dialog = dialog.set_directory(dir);
    }
    dialog.save_file()
}

fn failed<R: Runtime>(webview: &Webview<R>, path: &Path) {
    tracing::warn!("download to {} failed", path.display());
    webview
        .dialog()
        .message(format!("Could not save {}", path.display()))
        .show(|_| {});
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url() -> Url {
        "http://127.0.0.1/api/decoderlog/export/csv"
            .parse()
            .unwrap()
    }

    #[test]
    fn finish_returns_the_chosen_path_once() {
        let pending = Pending::default();
        pending.start(url(), PathBuf::from("/tmp/decoderlog.csv"));
        assert_eq!(
            pending.finish(&url()),
            Some(PathBuf::from("/tmp/decoderlog.csv"))
        );
        assert_eq!(pending.finish(&url()), None);
    }
}
