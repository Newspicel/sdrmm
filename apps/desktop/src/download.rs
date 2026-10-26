use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

use tauri::{Manager, Runtime, Url, Webview, webview::DownloadEvent};
use tauri_plugin_dialog::DialogExt;

const STAGING: &str = "downloads";
const UNNAMED: &str = "download";

#[derive(Debug)]
enum Destination {
    Undecided,
    Chosen(PathBuf),
    Declined,
}

#[derive(Debug)]
struct Transfer {
    url: Url,
    staged: PathBuf,
    name: String,
    destination: Destination,
    downloaded: bool,
}

#[derive(Debug)]
enum Settled {
    Save {
        staged: PathBuf,
        destination: PathBuf,
    },
    Discard {
        staged: PathBuf,
    },
    Failed {
        staged: PathBuf,
        name: String,
    },
}

#[derive(Debug, Default)]
struct Transfers {
    live: Mutex<Vec<Transfer>>,
    tickets: AtomicU64,
}

impl Transfers {
    fn ticket(&self) -> u64 {
        self.tickets.fetch_add(1, Ordering::Relaxed)
    }

    fn start(&self, url: Url, staged: PathBuf, name: String) {
        self.lock().push(Transfer {
            url,
            staged,
            name,
            destination: Destination::Undecided,
            downloaded: false,
        });
    }

    fn decide(&self, staged: &Path, destination: Destination) -> Option<Settled> {
        let mut live = self.lock();
        let at = live.iter().position(|transfer| transfer.staged == staged)?;
        live[at].destination = destination;
        settle(&mut live, at)
    }

    fn finish(&self, url: &Url, success: bool) -> Option<Settled> {
        let mut live = self.lock();
        let at = live
            .iter()
            .position(|transfer| &transfer.url == url && !transfer.downloaded)?;
        if !success {
            let transfer = live.remove(at);
            return Some(Settled::Failed {
                staged: transfer.staged,
                name: transfer.name,
            });
        }
        live[at].downloaded = true;
        settle(&mut live, at)
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Transfer>> {
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn settle(live: &mut Vec<Transfer>, at: usize) -> Option<Settled> {
    if !live[at].downloaded || matches!(live[at].destination, Destination::Undecided) {
        return None;
    }
    let transfer = live.remove(at);
    Some(match transfer.destination {
        Destination::Chosen(destination) => Settled::Save {
            staged: transfer.staged,
            destination,
        },
        Destination::Undecided | Destination::Declined => Settled::Discard {
            staged: transfer.staged,
        },
    })
}

pub fn handler<R: Runtime>() -> impl Fn(Webview<R>, DownloadEvent<'_>) -> bool + Send + Sync {
    let transfers = Arc::new(Transfers::default());
    move |webview, event| match event {
        DownloadEvent::Requested { url, destination } => {
            let name = file_name(destination);
            match stage(&webview, transfers.ticket(), &name) {
                Ok(staged) => {
                    transfers.start(url, staged.clone(), name);
                    ask(&webview, &transfers, &staged, destination);
                    destination.clone_from(&staged);
                    true
                }
                Err(error) => {
                    tracing::warn!(%error, "no staging directory for downloads");
                    tell(&webview, format!("Could not start the download: {error}"));
                    false
                }
            }
        }
        DownloadEvent::Finished { url, success, .. } => {
            if let Some(settled) = transfers.finish(&url, success) {
                complete(&webview, settled);
            }
            true
        }
        _ => true,
    }
}

fn file_name(suggested: &Path) -> String {
    suggested
        .file_name()
        .unwrap_or(OsStr::new(UNNAMED))
        .to_string_lossy()
        .into_owned()
}

fn stage<R: Runtime>(webview: &Webview<R>, ticket: u64, name: &str) -> io::Result<PathBuf> {
    let dir = webview
        .path()
        .app_cache_dir()
        .map_err(io::Error::other)?
        .join(STAGING);
    fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("{ticket}-{name}")))
}

fn ask<R: Runtime>(
    webview: &Webview<R>,
    transfers: &Arc<Transfers>,
    staged: &Path,
    suggested: &Path,
) {
    let mut dialog = webview.dialog().file().set_parent(&webview.window());
    if let Some(name) = suggested.file_name() {
        dialog = dialog.set_file_name(name.to_string_lossy());
    }
    if let Some(dir) = suggested.parent().filter(|dir| dir.is_dir()) {
        dialog = dialog.set_directory(dir);
    }
    let transfers = Arc::clone(transfers);
    let staged = staged.to_path_buf();
    let webview = webview.clone();
    dialog.save_file(move |chosen| {
        let destination = match chosen.and_then(|path| path.into_path().ok()) {
            Some(path) => Destination::Chosen(path),
            None => Destination::Declined,
        };
        if let Some(settled) = transfers.decide(&staged, destination) {
            complete(&webview, settled);
        }
    });
}

fn complete<R: Runtime>(webview: &Webview<R>, settled: Settled) {
    match settled {
        Settled::Save {
            staged,
            destination,
        } => {
            if let Err(error) = save(&staged, &destination) {
                tracing::warn!(%error, "could not save {}", destination.display());
                tell(webview, format!("Could not save {}", destination.display()));
            }
        }
        Settled::Discard { staged } => discard(&staged),
        Settled::Failed { staged, name } => {
            discard(&staged);
            tell(webview, format!("Could not download {name}"));
        }
    }
}

fn save(staged: &Path, destination: &Path) -> io::Result<()> {
    if fs::rename(staged, destination).is_ok() {
        return Ok(());
    }
    fs::copy(staged, destination)?;
    fs::remove_file(staged)
}

fn discard(staged: &Path) {
    if let Err(error) = fs::remove_file(staged)
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(%error, "could not remove {}", staged.display());
    }
}

fn tell<R: Runtime>(webview: &Webview<R>, message: String) {
    webview.dialog().message(message).show(|_| {});
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url() -> Url {
        "http://127.0.0.1/api/decoderlog/export/csv"
            .parse()
            .unwrap()
    }

    fn started(transfers: &Transfers, staged: &str) {
        transfers.start(url(), PathBuf::from(staged), "decoderlog.csv".to_owned());
    }

    #[test]
    fn a_finished_download_waits_for_the_dialog() {
        let transfers = Transfers::default();
        started(&transfers, "/staging/0-decoderlog.csv");
        assert!(transfers.finish(&url(), true).is_none());
        let settled = transfers.decide(
            Path::new("/staging/0-decoderlog.csv"),
            Destination::Chosen(PathBuf::from("/home/me/decoderlog.csv")),
        );
        assert!(matches!(
            settled,
            Some(Settled::Save { staged, destination })
                if staged == Path::new("/staging/0-decoderlog.csv")
                    && destination == Path::new("/home/me/decoderlog.csv")
        ));
    }

    #[test]
    fn a_chosen_path_waits_for_the_download() {
        let transfers = Transfers::default();
        started(&transfers, "/staging/0-decoderlog.csv");
        assert!(
            transfers
                .decide(
                    Path::new("/staging/0-decoderlog.csv"),
                    Destination::Chosen(PathBuf::from("/home/me/decoderlog.csv")),
                )
                .is_none()
        );
        assert!(matches!(
            transfers.finish(&url(), true),
            Some(Settled::Save { .. })
        ));
    }

    #[test]
    fn a_declined_dialog_drops_the_staged_file() {
        let transfers = Transfers::default();
        started(&transfers, "/staging/0-decoderlog.csv");
        assert!(transfers.finish(&url(), true).is_none());
        assert!(matches!(
            transfers.decide(
                Path::new("/staging/0-decoderlog.csv"),
                Destination::Declined
            ),
            Some(Settled::Discard { .. })
        ));
    }

    #[test]
    fn a_failed_download_surfaces_without_the_dialog() {
        let transfers = Transfers::default();
        started(&transfers, "/staging/0-decoderlog.csv");
        assert!(matches!(
            transfers.finish(&url(), false),
            Some(Settled::Failed { name, .. }) if name == "decoderlog.csv"
        ));
        assert!(
            transfers
                .decide(
                    Path::new("/staging/0-decoderlog.csv"),
                    Destination::Chosen(PathBuf::from("/home/me/decoderlog.csv")),
                )
                .is_none()
        );
    }

    #[test]
    fn two_downloads_of_one_url_settle_apart() {
        let transfers = Transfers::default();
        started(&transfers, "/staging/0-decoderlog.csv");
        started(&transfers, "/staging/1-decoderlog.csv");
        assert!(transfers.finish(&url(), true).is_none());
        assert!(transfers.finish(&url(), true).is_none());
        let second = transfers.decide(
            Path::new("/staging/1-decoderlog.csv"),
            Destination::Chosen(PathBuf::from("/home/me/second.csv")),
        );
        assert!(matches!(
            second,
            Some(Settled::Save { staged, destination })
                if staged == Path::new("/staging/1-decoderlog.csv")
                    && destination == Path::new("/home/me/second.csv")
        ));
        let first = transfers.decide(
            Path::new("/staging/0-decoderlog.csv"),
            Destination::Chosen(PathBuf::from("/home/me/first.csv")),
        );
        assert!(matches!(
            first,
            Some(Settled::Save { staged, destination })
                if staged == Path::new("/staging/0-decoderlog.csv")
                    && destination == Path::new("/home/me/first.csv")
        ));
    }

    #[test]
    fn tickets_keep_staged_names_apart() {
        let transfers = Transfers::default();
        assert_ne!(transfers.ticket(), transfers.ticket());
    }

    #[test]
    fn save_moves_the_staged_file_over_what_is_there() {
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join("0-decoderlog.csv");
        let destination = dir.path().join("decoderlog.csv");
        fs::write(&staged, b"fresh").unwrap();
        fs::write(&destination, b"stale").unwrap();
        save(&staged, &destination).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"fresh");
        assert!(!staged.exists());
    }

    #[test]
    fn discard_tolerates_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        discard(&dir.path().join("gone.csv"));
    }

    #[test]
    fn an_unnamed_download_still_gets_a_name() {
        assert_eq!(file_name(Path::new("/")), UNNAMED);
        assert_eq!(
            file_name(Path::new("/tmp/decoderlog.csv")),
            "decoderlog.csv"
        );
    }
}
