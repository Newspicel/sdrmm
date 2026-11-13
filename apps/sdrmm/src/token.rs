use std::{
    fs,
    io::{ErrorKind, Write},
    net::SocketAddr,
    path::Path,
};

use anyhow::Context;

const FILE_NAME: &str = "access-token";
const TOKEN_BYTES: usize = 32;

/// The token the server runs with. A network bind never runs open by accident: without
/// `--token` it uses the one kept in `dir`, minted on first start, so a container comes up
/// protected with no setup. `--no-auth` is the deliberate way to run open.
pub(crate) fn resolve(
    bind: SocketAddr,
    given: Option<String>,
    no_auth: bool,
    https: bool,
    dir: &Path,
) -> anyhow::Result<Option<String>> {
    let given = given.filter(|token| !token.is_empty());
    if no_auth && given.is_some() {
        anyhow::bail!("--no-auth and --token contradict each other; drop one");
    }
    if given.is_some() || bind.ip().is_loopback() {
        return Ok(given);
    }
    if no_auth {
        tracing::warn!(%bind, "--no-auth: anyone who reaches this address controls the server");
        return Ok(None);
    }
    let path = dir.join(FILE_NAME);
    if let Some(token) = stored(&path)? {
        tracing::info!(file = %path.display(), "no --token given, using the stored one");
        return Ok(Some(token));
    }
    let token = mint(&path)?;
    tracing::warn!(
        url = %format!(
            "{}://localhost:{}/?token={token}",
            if https { "https" } else { "http" },
            bind.port()
        ),
        file = %path.display(),
        "no --token given, generated one"
    );
    Ok(Some(token))
}

fn stored(path: &Path) -> anyhow::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents.trim().to_owned()).filter(|token| !token.is_empty())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
    }
}

/// Hides the token from the diagnostics log before returning it, so the line that announces it
/// reaches only the console.
fn mint(path: &Path) -> anyhow::Result<String> {
    let mut bytes = [0; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|error| anyhow::anyhow!("no randomness: {error}"))?;
    let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    sdrmm_server::diagnostics::hide_secret(&token);
    write_private(path, &token).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(token)
}

fn write_private(path: &Path, token: &str) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;
    writeln!(file, "{token}")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn file(dir: &Path) -> PathBuf {
        dir.join(FILE_NAME)
    }

    fn open() -> SocketAddr {
        "0.0.0.0:8080".parse().expect("addr")
    }

    #[test]
    fn a_given_token_is_used_as_is() {
        let dir = tempfile::tempdir().expect("dir");
        let token =
            resolve(open(), Some("s3cret".into()), false, false, dir.path()).expect("resolve");
        assert_eq!(token.as_deref(), Some("s3cret"));
        assert!(!file(dir.path()).exists(), "nothing minted");
    }

    #[test]
    fn loopback_runs_without_a_token() {
        let dir = tempfile::tempdir().expect("dir");
        for bind in ["127.0.0.1:8080", "[::1]:8080"] {
            let token = resolve(bind.parse().expect("addr"), None, false, false, dir.path())
                .expect("resolve");
            assert_eq!(token, None, "{bind}");
        }
        assert!(!file(dir.path()).exists());
    }

    #[test]
    fn a_network_bind_mints_a_token_and_keeps_it() {
        let dir = tempfile::tempdir().expect("dir");
        let first = resolve(open(), None, false, false, dir.path())
            .expect("resolve")
            .expect("minted");
        assert_eq!(first.len(), TOKEN_BYTES * 2);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let again =
            resolve(open(), Some(String::new()), false, false, dir.path()).expect("resolve");
        assert_eq!(
            again.as_deref(),
            Some(first.as_str()),
            "an empty token is none"
        );
        let lan = resolve(
            "192.168.1.20:8080".parse().expect("addr"),
            None,
            false,
            false,
            dir.path(),
        );
        assert_eq!(lan.expect("resolve").as_deref(), Some(first.as_str()));
    }

    #[cfg(unix)]
    #[test]
    fn the_minted_token_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("dir");
        resolve(open(), None, false, false, dir.path()).expect("resolve");
        let mode = fs::metadata(file(dir.path()))
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn the_minted_token_is_hidden_from_diagnostics() {
        let dir = tempfile::tempdir().expect("dir");
        let token = resolve(open(), None, false, false, dir.path())
            .expect("resolve")
            .expect("minted");
        assert_eq!(
            sdrmm_server::diagnostics::redact_text(&format!("/?token={token}")),
            "/?token=<redacted>"
        );
    }

    #[test]
    fn an_edited_token_file_is_honoured() {
        let dir = tempfile::tempdir().expect("dir");
        fs::write(file(dir.path()), "  chosen \n").expect("write");
        let token = resolve(open(), None, false, false, dir.path()).expect("resolve");
        assert_eq!(token.as_deref(), Some("chosen"));
    }

    #[test]
    fn no_auth_runs_open_and_mints_nothing() {
        let dir = tempfile::tempdir().expect("dir");
        assert_eq!(
            resolve(open(), None, true, false, dir.path()).expect("resolve"),
            None
        );
        let empty = resolve(open(), Some(String::new()), true, false, dir.path());
        assert_eq!(empty.expect("an empty token is none"), None);
        assert!(!file(dir.path()).exists());
    }

    #[test]
    fn no_auth_with_a_token_is_refused() {
        let dir = tempfile::tempdir().expect("dir");
        assert!(resolve(open(), Some("s3cret".into()), true, false, dir.path()).is_err());
    }
}
