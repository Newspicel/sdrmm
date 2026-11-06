use std::{io, path::Path};

#[derive(Debug)]
pub struct Shell;

impl sdrmm_server::NativeShell for Shell {
    fn reveal(&self, path: &Path) -> io::Result<()> {
        let shown = if path.is_dir() {
            opener::open(path)
        } else {
            opener::reveal(path)
        };
        shown.map_err(io::Error::other)
    }

    fn notify(&self, title: &str, body: &str) -> io::Result<()> {
        notify_rust::Notification::new()
            .appname("SDRmm")
            .summary(title)
            .body(body)
            .show()
            .map(drop)
            .map_err(io::Error::other)
    }
}
