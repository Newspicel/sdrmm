use std::{
    fs::File,
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use sdrmm_wire::{CreatedRowId, PhoneAccess, PhoneAccessStatus, PhoneListenerState};

pub(super) const SELECTION: &str = "SDRmmUITests/EndToEndUITests";
const FIXTURE: &str = "fixtures/ios/e2e-workspace.json";
const TOKEN: &str = "e2e";
const READY_WITHIN: Duration = Duration::from_secs(180);
/// A starved macOS runner drops loopback connections now and then; repeating a call is harmless.
const CURL_RETRY: [&str; 5] = ["--retry", "4", "--retry-all-errors", "--retry-delay", "2"];
const POLL: Duration = Duration::from_millis(500);

pub(super) struct Server {
    child: Child,
    base: String,
    log: PathBuf,
}

enum Body<'a> {
    Empty,
    Json(String),
    File(&'a Path),
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Err(error) = self.child.kill() {
            eprintln!("could not stop sdrmm: {error}");
        }
        if let Err(error) = self.child.wait() {
            eprintln!("sdrmm did not exit: {error}");
        }
        println!("sdrmm stopped, log in {}", self.log.display());
    }
}

impl Server {
    pub(super) fn start(root: &Path, target: &Path) -> Result<Self> {
        let build = target.join("no-soapy");
        crate::run(
            "cargo",
            &[
                "build",
                "-p",
                "sdrmm",
                "--no-default-features",
                "--target-dir",
                &build.to_string_lossy(),
            ],
            root,
        )?;
        let dir = target.join("ios/e2e");
        if dir.exists() {
            std::fs::remove_dir_all(&dir).with_context(|| format!("remove {}", dir.display()))?;
        }
        std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        let port = free_port(Ipv4Addr::LOCALHOST)?;
        let bind = format!("127.0.0.1:{port}");
        let log = dir.join("server.log");
        let output = File::create(&log).with_context(|| format!("create {}", log.display()))?;
        let errors = output.try_clone().context("clone the server log")?;
        let args = server_args(&bind, &dir);
        println!("$ sdrmm {}", args.join(" "));
        let child = Command::new(build.join("debug/sdrmm"))
            .args(&args)
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(output)
            .stderr(errors)
            .spawn()
            .context("failed to start sdrmm")?;
        let mut server = Self {
            child,
            base: format!("https://{bind}"),
            log,
        };
        server.wait_ready()?;
        Ok(server)
    }

    fn wait_ready(&mut self) -> Result<()> {
        let deadline = Instant::now() + READY_WITHIN;
        loop {
            if let Some(status) = self.child.try_wait().context("poll sdrmm")? {
                bail!("sdrmm exited with {status}, see {}", self.log.display());
            }
            if matches!(self.call("GET", "/api/about", &Body::Empty), Ok((200, _))) {
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline,
                "sdrmm did not answer within {READY_WITHIN:?}, see {}",
                self.log.display()
            );
            std::thread::sleep(POLL);
        }
    }

    /// Opens the server to phones and loads the fixture. The test mints its own code, since
    /// xcodebuild can take longer to reach it than a code lives.
    pub(super) fn open_for_phones(&self, root: &Path) -> Result<Vec<(&'static str, String)>> {
        let port = free_port(Ipv4Addr::UNSPECIFIED)?;
        let access = serde_json::to_string(&PhoneAccess {
            enabled: true,
            port,
        })?;
        let status: PhoneAccessStatus =
            self.json("PUT", "/api/phones/access", &Body::Json(access))?;
        ensure!(
            status.listener == PhoneListenerState::On { port },
            "the phone listener did not start: {:?}",
            status.listener
        );
        let fixture = root.join(FIXTURE);
        let imported: CreatedRowId =
            self.json("POST", "/api/workspaces/import", &Body::File(&fixture))?;
        for step in ["activate", "apply"] {
            let path = format!("/api/workspaces/{}/{step}", imported.id);
            self.ok("POST", &path, &Body::Empty)?;
        }
        Ok(test_env(&self.base, port))
    }

    fn json<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: &Body<'_>,
    ) -> Result<T> {
        let text = self.ok(method, path, body)?;
        serde_json::from_str(&text).with_context(|| format!("{method} {path} answered {text}"))
    }

    fn ok(&self, method: &str, path: &str, body: &Body<'_>) -> Result<String> {
        let (status, text) = self.call(method, path, body)?;
        ensure!(
            (200..300).contains(&status),
            "{method} {path} answered {status}: {text}"
        );
        Ok(text)
    }

    fn call(&self, method: &str, path: &str, body: &Body<'_>) -> Result<(u16, String)> {
        let url = format!("{}{path}", self.base);
        let auth = format!("Authorization: Bearer {TOKEN}");
        let data = match body {
            Body::Empty => None,
            Body::Json(json) => Some(json.clone()),
            Body::File(path) => Some(format!("@{}", path.display())),
        };
        let mut args = vec![
            "-sS",
            "-k",
            "-X",
            method,
            "-H",
            &auth,
            "-w",
            "\n%{http_code}",
        ];
        args.extend(CURL_RETRY);
        if let Some(data) = &data {
            args.extend([
                "-H",
                "Content-Type: application/json",
                "--data-binary",
                data,
            ]);
        }
        args.push(&url);
        let output = Command::new("curl")
            .args(&args)
            .output()
            .context("failed to spawn `curl`")?;
        ensure!(
            output.status.success(),
            "curl {method} {path}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let (status, text) = split_status(&stdout)?;
        Ok((status, text.to_owned()))
    }
}

fn server_args(bind: &str, dir: &Path) -> Vec<String> {
    vec![
        "--bind".to_owned(),
        bind.to_owned(),
        "--tls-self-signed".to_owned(),
        "--token".to_owned(),
        TOKEN.to_owned(),
        "--db".to_owned(),
        dir.join("e2e.db").display().to_string(),
        "--recordings-dir".to_owned(),
        dir.join("recordings").display().to_string(),
    ]
}

fn free_port(ip: Ipv4Addr) -> Result<u16> {
    let listener = TcpListener::bind((ip, 0)).context("find a free port")?;
    Ok(listener.local_addr()?.port())
}

fn split_status(output: &str) -> Result<(u16, &str)> {
    let (text, status) = output
        .rsplit_once('\n')
        .context("curl printed no status line")?;
    let status = status
        .trim()
        .parse()
        .with_context(|| format!("curl status {status:?}"))?;
    Ok((status, text))
}

fn test_env(base: &str, phone_port: u16) -> Vec<(&'static str, String)> {
    vec![
        ("TEST_RUNNER_SDRMM_E2E_SERVER", base.to_owned()),
        ("TEST_RUNNER_SDRMM_E2E_TOKEN", TOKEN.to_owned()),
        (
            "TEST_RUNNER_SDRMM_E2E_PHONE",
            format!("127.0.0.1:{phone_port}"),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{NodeBody, WorkspaceExport};

    use super::*;

    #[test]
    fn the_test_gets_the_server_its_token_and_the_phone_listener() {
        assert_eq!(
            test_env("https://127.0.0.1:18443", 18_444),
            [
                (
                    "TEST_RUNNER_SDRMM_E2E_SERVER",
                    "https://127.0.0.1:18443".to_owned()
                ),
                ("TEST_RUNNER_SDRMM_E2E_TOKEN", "e2e".to_owned()),
                ("TEST_RUNNER_SDRMM_E2E_PHONE", "127.0.0.1:18444".to_owned()),
            ]
        );
    }

    #[test]
    fn curl_output_splits_into_status_and_body() {
        assert_eq!(
            split_status("{\"id\":3}\n200").expect("split"),
            (200, "{\"id\":3}")
        );
        assert_eq!(split_status("\n204").expect("split"), (204, ""));
        assert!(split_status("no status").is_err());
        assert!(split_status("body\nxyz").is_err());
    }

    #[test]
    fn the_server_runs_with_tls_the_token_and_its_own_data() {
        let args = server_args("127.0.0.1:18443", Path::new("/tmp/e2e"));
        assert_eq!(
            args,
            [
                "--bind",
                "127.0.0.1:18443",
                "--tls-self-signed",
                "--token",
                "e2e",
                "--db",
                "/tmp/e2e/e2e.db",
                "--recordings-dir",
                "/tmp/e2e/recordings",
            ]
        );
    }

    #[test]
    fn the_fixture_is_a_hunt_on_a_virtual_radio() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let text = std::fs::read_to_string(root.join(FIXTURE)).expect("fixture");
        let export: WorkspaceExport = serde_json::from_str(&text).expect("a workspace export");
        export.validate().expect("a current, valid export");
        let graph = &export.snapshot.graph;
        let body = |id: &str| graph.node(id).map(|node| &node.body);
        let wired = |from: &str, to: &str| {
            graph
                .edges
                .iter()
                .any(|edge| edge.from.node == from && edge.to.node == to)
        };
        assert!(matches!(
            body("radio"),
            Some(NodeBody::Device(device))
                if device.device.as_ref().is_some_and(|radio| radio.backend == sdrmm_wire::DeviceBackend::VIRTUAL)
        ));
        assert!(matches!(body("voice"), Some(NodeBody::Channel(_))));
        assert!(matches!(body("hunt"), Some(NodeBody::Hunt(_))));
        assert!(wired("radio", "voice") && wired("hunt", "voice"));
        assert!(wired("radio", "hunt"), "the hunt watches the radio lane");
    }
}
