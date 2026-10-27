mod frame_fixtures;

#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, RecvTimeoutError},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, Subcommand};
use notify::{Config, Event, EventKind, PollWatcher, RecursiveMode, Watcher};
use num_complex::Complex;
pub(crate) use xtask_release::bundled;
pub(crate) use xtask_release::dist::release_features;
pub(crate) use xtask_release::media::{ensure_target, media_dir, media_env, run_against_media};
pub(crate) use xtask_release::proc::{run, run_with_env, utf8};
pub(crate) use xtask_release::web::{PNPM, ensure_web_deps, web_build};

mod architecture;
mod bandplan;
mod ber;
mod broadcast_fixtures;
mod bundle;
mod capture;
mod comments;
mod compare;
mod denoise_model;
mod excerpt;
mod fuzz_seeds;
mod icons;
mod ident_matrix;
mod ios;
mod licenses;
mod mobile;
mod nixhash;
mod replay;
mod resolved_fixtures;
#[cfg(test)]
mod site;
mod units;
mod wires;

#[derive(Parser)]
#[command(name = "xtask", about = "SDR-- workspace tasks")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Codegen,
    Licenses,
    Dev {
        #[arg(long)]
        watch: bool,
    },
    Check,
    Test,
    Perf,
    Sanitize,
    Fuzz {
        #[arg(long)]
        target: Option<String>,
        #[arg(long, default_value_t = 60)]
        seconds: u64,
        #[arg(long, default_value_t = 1)]
        jobs: u8,
        #[arg(long)]
        minimize: bool,
    },
    Audit,
    Smoke,
    Screenshots,
    Fixtures,
    BroadcastFixtures {
        #[arg(long)]
        out: PathBuf,
    },
    Excerpt(excerpt::Excerpt),
    Replay(replay::Replay),
    NetCapture(capture::NetCapture),
    Bandplan {
        #[arg(long)]
        offline: bool,
    },
    Ber {
        entry: String,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        full: bool,
    },
    DenoiseModel,
    Icons,
    IdentMatrix,
    NixHash,
    Desktop {
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        bundles: Option<String>,
    },
    #[command(flatten)]
    Release(xtask_release::Cmd),
    Mobile(mobile::Mobile),
    Ios {
        #[command(subcommand)]
        action: ios::IosAction,
    },
    Compare {
        #[command(subcommand)]
        suite: compare::Compare,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Codegen => codegen(&root()),
        Cmd::Licenses => licenses::run(&root(), PNPM),
        Cmd::Dev { watch } => dev(&root(), watch),
        Cmd::Check => check(&root()),
        Cmd::Test => test(&root()),
        Cmd::Perf => perf(&root()),
        Cmd::Sanitize => sanitize(&root()),
        Cmd::Fuzz {
            target,
            seconds,
            jobs,
            minimize,
        } => fuzz(&root(), target.as_deref(), seconds, jobs, minimize),
        Cmd::Audit => audit(&root()),
        Cmd::Smoke => smoke(&root()),
        Cmd::Screenshots => screenshots(&root()),
        Cmd::Fixtures => fixtures(&root()),
        Cmd::BroadcastFixtures { out } => broadcast_fixtures::run(&out),
        Cmd::Excerpt(args) => excerpt::run(&root(), &args),
        Cmd::Replay(args) => replay::run(&args),
        Cmd::NetCapture(args) => capture::run(&args),
        Cmd::Compare { suite } => compare::run(&root(), &suite),
        Cmd::Bandplan { offline } => bandplan::run(&root(), offline),
        Cmd::Ber { entry, out, full } => ber::run(&root(), &entry, out.as_deref(), full),
        Cmd::DenoiseModel => denoise_model::run(&root()),
        Cmd::Icons => icons::icons(&root()),
        Cmd::IdentMatrix => ident_matrix::run(&root()),
        Cmd::NixHash => nixhash::run(&root()),
        Cmd::Desktop { target, bundles } => desktop(&root(), target.as_deref(), bundles.as_deref()),
        Cmd::Release(cmd) => xtask_release::run(&root(), &cmd),
        Cmd::Mobile(args) => mobile::run(&root(), &args),
        Cmd::Ios { action } => ios::run(&root(), &action),
    }
}

#[expect(clippy::expect_used)]
fn root() -> PathBuf {
    xtask_release::root().expect("workspace root")
}

fn codegen(root: &Path) -> Result<()> {
    std::fs::write(
        root.join("web/src/generated/frame-fixtures.json"),
        serde_json::to_string_pretty(&frame_fixtures::frames()?)?,
    )
    .context("write binary frame fixtures")?;
    std::fs::write(
        root.join("web/src/generated/catalog-fixture.json"),
        serde_json::to_string_pretty(&sdrmm_wire::PatchCatalog::build())?,
    )
    .context("write catalog test fixture")?;
    std::fs::write(
        root.join("web/src/generated/channel-defaults.json"),
        serde_json::to_string_pretty(&channel_defaults())?,
    )
    .context("write channel defaults")?;
    resolved_fixtures::write(root)?;
    std::fs::write(
        root.join("web/src/generated/labels.json"),
        serde_json::to_string_pretty(&sdrmm_wire::labels::generated()?)?,
    )
    .context("write labels")?;
    std::fs::write(
        root.join("web/src/generated/limits.json"),
        sdrmm_wire::limits::generated()?,
    )
    .context("write limits")?;
    std::fs::write(
        root.join("web/src/generated/backends.json"),
        serde_json::to_string_pretty(&sdrmm_wire::backend_table())?,
    )
    .context("write device backends")?;
    std::fs::write(
        root.join("docs/src/wires.md"),
        wires::page(&sdrmm_wire::PatchCatalog::build()),
    )
    .context("write the wires page")?;
    let frames = root.join("web/src/generated/frame.ts");
    std::fs::write(&frames, sdrmm_wire::typescript_frames()).context("write binary frame codec")?;
    let spec = sdrmm_server::openapi()
        .to_pretty_json()
        .context("serialize OpenAPI")?;
    let openapi_path = root.join("openapi.json");
    std::fs::write(&openapi_path, format!("{spec}\n")).context("write openapi.json")?;
    println!("wrote {}", openapi_path.display());

    let out = root.join("web/src/generated/schema.d.ts");
    let generated = out.parent().context("schema path has no directory")?;
    std::fs::create_dir_all(generated).context("create generated dir")?;

    run(
        PNPM,
        &[
            "--dir",
            "codegen",
            "exec",
            "openapi-typescript",
            utf8(&openapi_path)?,
            "-o",
            utf8(&out)?,
            "--alphabetize",
        ],
        root,
    )?;
    println!("wrote {}", out.display());
    Ok(())
}

fn channel_defaults() -> std::collections::BTreeMap<String, sdrmm_wire::ChannelParams> {
    sdrmm_channels::descriptors()
        .into_iter()
        .filter_map(|descriptor| {
            descriptor
                .defaults
                .map(|settings| (descriptor.type_id, settings.params))
        })
        .collect()
}

fn dev(root: &Path, watch: bool) -> Result<()> {
    ensure_web_deps(root)?;
    #[cfg(unix)]
    let _interrupt_handler = InterruptHandler::install()?;

    let mut vite = None;
    let result = if watch {
        watch_rust_server(root, &mut vite)
    } else {
        run_rust_server(root, &mut vite)
    };

    if let Some(mut vite) = vite {
        kill_process_tree(&mut vite);
    }
    result
}

fn spawn_vite(root: &Path) -> Result<Child> {
    let mut vite = Command::new(PNPM);
    vite.args(["--dir", "web", "dev"]).current_dir(root);
    vite.stdin(Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut vite, 0);
    vite.spawn()
        .context("spawn vite dev server (is pnpm installed?)")
}

fn listening(addr: SocketAddr) -> bool {
    TcpStream::connect_timeout(&addr, DEV_POLL_INTERVAL).is_ok()
}

fn supervise_vite(root: &Path, vite: &mut Option<Child>) -> Result<()> {
    match vite {
        Some(child) => match child.try_wait().context("poll Vite dev server")? {
            Some(status) => bail!("Vite dev server exited with {status}"),
            None => Ok(()),
        },
        None if listening(DEV_SERVER_ADDR) => {
            *vite = Some(spawn_vite(root)?);
            Ok(())
        }
        None => Ok(()),
    }
}

fn run_rust_server(root: &Path, vite: &mut Option<Child>) -> Result<()> {
    let mut server = spawn_rust_server(root)?;
    let result = (|| -> Result<()> {
        loop {
            if dev_interrupted() {
                return Ok(());
            }
            supervise_vite(root, vite)?;
            if let Some(status) = server.try_wait().context("poll Rust server")? {
                ensure!(status.success(), "Rust server exited with {status}");
                return Ok(());
            }
            std::thread::sleep(DEV_POLL_INTERVAL);
        }
    })();

    kill_process_tree(&mut server);
    result
}

const DEV_POLL_INTERVAL: Duration = Duration::from_millis(100);
const DEV_SERVER_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);
const DEV_FILE_SCAN_INTERVAL: Duration = Duration::from_millis(500);
const DEV_RESTART_DEBOUNCE: Duration = Duration::from_millis(250);

fn watch_rust_server(root: &Path, vite: &mut Option<Child>) -> Result<()> {
    let (changes_tx, changes_rx) = mpsc::channel();
    let mut watcher = PollWatcher::new(
        changes_tx,
        Config::default().with_poll_interval(DEV_FILE_SCAN_INTERVAL),
    )
    .context("start backend watcher")?;
    watcher
        .watch(root, RecursiveMode::NonRecursive)
        .context("watch workspace manifests")?;
    for path in [root.join("crates"), root.join("apps/sdrmm")] {
        watcher
            .watch(&path, RecursiveMode::Recursive)
            .with_context(|| format!("watch {}", path.display()))?;
    }

    let mut server = Some(spawn_rust_server(root)?);
    let result = (|| -> Result<()> {
        let mut last_change = None;
        loop {
            if dev_interrupted() {
                return Ok(());
            }
            supervise_vite(root, vite)?;
            if let Some(child) = server.as_mut()
                && let Some(status) = child.try_wait().context("poll Rust server")?
            {
                eprintln!("Rust server exited with {status}; waiting for a backend change");
                server = None;
            }

            match changes_rx.recv_timeout(DEV_POLL_INTERVAL) {
                Ok(Ok(event)) if is_backend_change(root, &event) => {
                    last_change = Some(Instant::now());
                }
                Ok(Ok(_)) => {}
                Ok(Err(error)) => return Err(error).context("watch backend inputs"),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => bail!("backend watcher stopped"),
            }

            if last_change.is_some_and(|changed| changed.elapsed() >= DEV_RESTART_DEBOUNCE) {
                if let Some(mut child) = server.take() {
                    kill_process_tree(&mut child);
                }
                println!("backend changed; restarting Rust server");
                server = Some(spawn_rust_server(root)?);
                last_change = None;
            }
        }
    })();

    if let Some(mut child) = server {
        kill_process_tree(&mut child);
    }
    result
}

fn is_backend_change(root: &Path, event: &Event) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    let crates = root.join("crates");
    let server = root.join("apps/sdrmm");
    let manifest = root.join("Cargo.toml");
    let lockfile = root.join("Cargo.lock");
    event.paths.iter().any(|path| {
        path == &manifest
            || path == &lockfile
            || path.starts_with(&crates)
            || path.starts_with(&server)
    })
}

fn rust_server_command(root: &Path, media: Option<&Path>) -> Command {
    let mut server = Command::new("cargo");
    server
        .args(["run", "-p", "sdrmm", "--", "--dev-cors"])
        .current_dir(root);
    if let Some(dir) = media {
        server.envs(media_env(dir));
    }
    server
}

fn spawn_rust_server(root: &Path) -> Result<Child> {
    let mut server = rust_server_command(root, media_dir(root, None)?.as_deref());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut server, 0);
    server.spawn().context("spawn Rust server")
}

#[cfg(unix)]
struct InterruptHandler(libc::sigaction);

#[cfg(unix)]
static DEV_INTERRUPTED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
impl InterruptHandler {
    fn install() -> Result<Self> {
        DEV_INTERRUPTED.store(false, Ordering::Relaxed);
        let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
        action.sa_sigaction = preserve_dev_supervisor as *const () as libc::sighandler_t;
        action.sa_flags = libc::SA_RESTART;
        unsafe { libc::sigemptyset(&mut action.sa_mask) };

        let mut previous = unsafe { std::mem::zeroed::<libc::sigaction>() };
        if unsafe { libc::sigaction(libc::SIGINT, &action, &mut previous) } != 0 {
            return Err(std::io::Error::last_os_error()).context("install Ctrl-C handler");
        }
        Ok(Self(previous))
    }
}

#[cfg(unix)]
impl Drop for InterruptHandler {
    fn drop(&mut self) {
        unsafe { libc::sigaction(libc::SIGINT, &self.0, std::ptr::null_mut()) };
    }
}

#[cfg(unix)]
extern "C" fn preserve_dev_supervisor(_: libc::c_int) {
    DEV_INTERRUPTED.store(true, Ordering::Relaxed);
}

#[cfg(unix)]
fn dev_interrupted() -> bool {
    DEV_INTERRUPTED.load(Ordering::Relaxed)
}

#[cfg(not(unix))]
fn dev_interrupted() -> bool {
    false
}

#[cfg(unix)]
fn kill_process_tree(child: &mut Child) {
    let group = -(child.id() as i32);
    unsafe { libc::kill(group, libc::SIGTERM) };
    for _ in 0..50 {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
            Err(_) => break,
        }
    }
    unsafe { libc::kill(group, libc::SIGKILL) };
    let _ = child.wait();
}

#[cfg(not(unix))]
fn kill_process_tree(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(all(test, unix))]
mod dev_tests {
    use super::*;

    const HELPER_ENV: &str = "SDRMM_XTASK_INTERRUPT_HELPER";

    #[test]
    fn interrupt_handler_keeps_supervisor_alive() {
        let status = Command::new(std::env::current_exe().expect("locate test binary"))
            .args([
                "--ignored",
                "--exact",
                "dev_tests::interrupt_handler_helper",
            ])
            .env(HELPER_ENV, "1")
            .status()
            .expect("run interrupt helper");
        assert!(status.success(), "interrupt helper exited with {status}");
    }

    #[test]
    #[ignore]
    fn interrupt_handler_helper() {
        if std::env::var_os(HELPER_ENV).is_none() {
            return;
        }
        let _handler = InterruptHandler::install().expect("install Ctrl-C handler");
        assert_eq!(unsafe { libc::raise(libc::SIGINT) }, 0);
        assert!(dev_interrupted());
    }
}

#[cfg(test)]
mod dev_command_tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn dev_watches_backend_only_with_the_flag() {
        let plain = Cli::try_parse_from(["xtask", "dev"]).expect("parse dev");
        assert!(matches!(plain.cmd, Cmd::Dev { watch: false }));

        let watching = Cli::try_parse_from(["xtask", "dev", "--watch"]).expect("parse dev --watch");
        assert!(matches!(watching.cmd, Cmd::Dev { watch: true }));
    }

    #[test]
    fn listening_detects_a_bound_rust_server() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe port");
        let addr = listener.local_addr().expect("probe address");
        assert!(listening(addr));
    }

    #[test]
    fn rust_server_command_enables_development_cors() {
        let root = Path::new("workspace");
        let server = rust_server_command(root, None);

        assert_eq!(server.get_program(), "cargo");
        assert_eq!(server.get_current_dir(), Some(root));
        assert_eq!(
            server.get_args().collect::<Vec<_>>(),
            ["run", "-p", "sdrmm", "--", "--dev-cors"]
        );
    }

    #[test]
    fn rust_server_command_builds_and_runs_against_the_bundled_media() {
        let media = Path::new("workspace/.media/host");
        let server = rust_server_command(Path::new("workspace"), Some(media));
        let env: Vec<_> = server.get_envs().collect();

        assert!(env.contains(&(OsStr::new("FFMPEG_DIR"), Some(media.as_os_str()))));
        assert_eq!(
            env.iter().any(|(key, _)| *key == "LD_LIBRARY_PATH"),
            cfg!(target_os = "linux")
        );
        assert_eq!(
            env.iter().any(|(key, _)| *key == "DYLD_LIBRARY_PATH"),
            cfg!(target_os = "macos")
        );
    }

    #[test]
    fn backend_change_filter_accepts_only_watched_build_inputs() {
        let root = Path::new("/workspace");
        for path in [
            "/workspace/Cargo.toml",
            "/workspace/Cargo.lock",
            "/workspace/crates/server/src/lib.rs",
            "/workspace/apps/sdrmm/src/main.rs",
        ] {
            let event = Event::new(EventKind::Any).add_path(path.into());
            assert!(is_backend_change(root, &event), "ignored {path}");
        }

        for path in [
            "/workspace/README.md",
            "/workspace/web/src/App.tsx",
            "/workspace/xtask/src/main.rs",
        ] {
            let event = Event::new(EventKind::Any).add_path(path.into());
            assert!(!is_backend_change(root, &event), "accepted {path}");
        }
    }

    #[test]
    fn backend_change_filter_ignores_file_access() {
        use notify::event::AccessKind;

        let event = Event::new(EventKind::Access(AccessKind::Any))
            .add_path("/workspace/crates/server/src/lib.rs".into());
        assert!(!is_backend_change(Path::new("/workspace"), &event));
    }
}

fn check(root: &Path) -> Result<()> {
    architecture::check(root)?;
    units::check(root)?;
    bundle::check_resources(root)?;
    nixhash::check(root)?;
    check_toolchain_pins(root)?;
    comments::check(root)?;
    check_windows_rs_alignment(root)?;
    check_baked_in_fixtures(root)?;
    mobile::check(root)?;
    ios::check(root)?;
    xtask_release::changeset::check(root)?;
    run("cargo", &["fmt", "--all", "--", "--check"], root)?;

    ensure_web_deps(root)?;
    run(PNPM, &["--dir", "web", "exec", "biome", "ci", "."], root)?;
    run(
        PNPM,
        &["--dir", "web", "exec", "oxlint", "--type-aware"],
        root,
    )?;
    run(PNPM, &["--dir", "web", "exec", "tsc", "--noEmit"], root)?;
    run(PNPM, &["--dir", "site", "exec", "biome", "ci", "."], root)?;
    run(PNPM, &["--dir", "site", "typecheck"], root)?;

    run(
        "cargo",
        &["clippy", "--all-targets", "--", "-D", "warnings"],
        root,
    )?;
    run(
        "cargo",
        &["check", "-p", "sdrmm", "--no-default-features"],
        root,
    )?;
    run(
        "cargo",
        &[
            "check",
            "-p",
            "sdrmm",
            &release_features()[0],
            &release_features()[1],
            &release_features()[2],
        ],
        root,
    )?;

    web_build(root)?;

    codegen(root)?;
    run(
        "git",
        &[
            "diff",
            "--exit-code",
            "--",
            "openapi.json",
            "web/src/generated",
            "docs/src/wires.md",
        ],
        root,
    )
    .context("codegen drift: regenerate with `cargo xtask codegen` and commit")?;

    licenses::run(root, PNPM)?;
    run(
        "git",
        &[
            "diff",
            "--exit-code",
            "--",
            licenses::NOTICES_JSON,
            licenses::NOTICES_MARKDOWN,
        ],
        root,
    )
    .context("notices drift: regenerate with `cargo xtask licenses` and commit")?;
    println!("check: all gates green");
    Ok(())
}

fn check_toolchain_pins(root: &Path) -> Result<()> {
    let file = |rel: &str| -> Result<String> {
        std::fs::read_to_string(root.join(rel)).with_context(|| format!("read {rel}"))
    };
    let pin = |text: &str, open: &str, close: &str, rel: &str, what: &str| -> Result<String> {
        slice_between(text, open, close)
            .map(str::to_string)
            .with_context(|| format!("{rel} declares no {what} (looked for `{open}`)"))
    };

    let dockerfile = file("Dockerfile")?;
    let mut pnpm = vec![(
        "Dockerfile".to_string(),
        pin(&dockerfile, "pnpm@", "\n", "Dockerfile", "pnpm pin")?,
    )];
    for rel in ["web/package.json", "site/package.json"] {
        pnpm.push((
            rel.to_string(),
            pin(
                &file(rel)?,
                "\"packageManager\": \"pnpm@",
                "\"",
                rel,
                "packageManager pin",
            )?,
        ));
    }
    let mut node = vec![(
        "Dockerfile".to_string(),
        pin(
            &dockerfile,
            "FROM node:",
            "-slim",
            "Dockerfile",
            "node base image",
        )?,
    )];

    let action = ".github/actions/node/action.yml";
    node.push((
        action.to_string(),
        pin(
            &file(action)?,
            "node-version: ",
            "\n",
            action,
            "node-version",
        )?,
    ));

    let workers = "site/.node-version";
    node.push((workers.to_string(), file(workers)?.trim().to_string()));

    agree("pnpm", &pnpm)?;
    agree("the Node major", &node)?;
    check_ndk_pin(root)
}

const ANDROID_PROPERTIES: &str = "apps/android/gradle.properties";
const NDK_PACKAGE_PIN: &str = "xtask/src/mobile.rs";
const NDK_DOCS_PIN: &str = "docs/src/development/phone-apps.md";

fn check_ndk_pin(root: &Path) -> Result<()> {
    let read = |rel: &str| -> Result<String> {
        std::fs::read_to_string(root.join(rel)).with_context(|| format!("read {rel}"))
    };
    let declared = slice_between(&read(ANDROID_PROPERTIES)?, "sdrmm.ndkVersion=", "\n")
        .map(str::to_owned)
        .with_context(|| format!("{ANDROID_PROPERTIES} declares no sdrmm.ndkVersion"))?;
    let package = slice_between(&read(NDK_PACKAGE_PIN)?, "NDK_PACKAGE: &str = \"ndk;", "\"")
        .map(str::to_owned)
        .with_context(|| format!("{NDK_PACKAGE_PIN} declares no NDK_PACKAGE"))?;

    let mut pins = vec![
        (ANDROID_PROPERTIES.to_owned(), declared),
        (NDK_PACKAGE_PIN.to_owned(), package),
    ];
    let docs = read(NDK_DOCS_PIN)?;
    let documented = ndk_revisions(&docs);
    ensure!(
        !documented.is_empty(),
        "{NDK_DOCS_PIN} names no NDK revision, so nobody can set up a phone build"
    );
    pins.extend(
        documented
            .into_iter()
            .map(|revision| (NDK_DOCS_PIN.to_owned(), revision)),
    );
    agree("the Android NDK revision", &pins)
}

fn ndk_revisions(text: &str) -> Vec<String> {
    ["ndk;", "ndk/"]
        .iter()
        .flat_map(|marker| text.split(marker).skip(1))
        .map(|rest| {
            rest.chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect::<String>()
        })
        .filter(|revision| !revision.is_empty())
        .collect()
}

#[cfg(test)]
mod ndk_pin_tests {
    use super::*;

    fn tree(properties: &str, package: &str, docs: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (rel, body) in [
            (
                ANDROID_PROPERTIES,
                format!("sdrmm.ndkVersion={properties}\n"),
            ),
            (
                NDK_PACKAGE_PIN,
                format!("pub(crate) const NDK_PACKAGE: &str = \"ndk;{package}\";\n"),
            ),
            (NDK_DOCS_PIN, format!("sdkmanager \"ndk;{docs}\"\n")),
        ] {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        dir
    }

    #[test]
    fn the_tree_pins_one_ndk_revision() {
        check_ndk_pin(&root()).unwrap();
    }

    #[test]
    fn the_tree_agrees_on_pnpm_and_the_node_major() {
        check_toolchain_pins(&root()).unwrap();
    }

    #[test]
    fn revisions_come_from_sdkmanager_names_and_paths() {
        let text = "sdkmanager \"ndk;30.0.1\"\nANDROID_NDK_HOME=$HOME/ndk/30.0.1\nndk/notes\n";
        assert_eq!(ndk_revisions(text), ["30.0.1", "30.0.1"]);
    }

    #[test]
    fn a_revision_that_drifted_in_the_docs_fails() {
        let dir = tree("30.0.1", "30.0.1", "29.0.1");
        let error = check_ndk_pin(dir.path()).unwrap_err().to_string();
        assert!(error.contains("the Android NDK revision"), "{error}");
        assert!(error.contains(NDK_DOCS_PIN), "{error}");
    }

    #[test]
    fn a_revision_that_drifted_in_the_build_tooling_fails() {
        let dir = tree("30.0.1", "28.0.1", "30.0.1");
        let error = check_ndk_pin(dir.path()).unwrap_err().to_string();
        assert!(error.contains(NDK_PACKAGE_PIN), "{error}");
    }
}

fn check_windows_rs_alignment(root: &Path) -> Result<()> {
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).context("read Cargo.lock")?;
    let (Some(hal), Some(allocator)) = (
        locked_dependency(&lock, "wgpu-hal", "windows"),
        locked_dependency(&lock, "gpu-allocator", "windows"),
    ) else {
        return Ok(());
    };
    ensure!(
        hal == allocator,
        "Cargo.lock resolves gpu-allocator against windows {allocator} and wgpu-hal against \
         windows {hal}: wgpu-hal's dx12 backend cannot compile against the pair. Point \
         gpu-allocator's `dependencies` entry in Cargo.lock at `windows {hal}`."
    );
    Ok(())
}

fn locked_dependency(lock: &str, package: &str, dependency: &str) -> Option<String> {
    let entry = locked_package(lock, package)?
        .lines()
        .map(|line| line.trim().trim_end_matches(',').trim_matches('"'))
        .find(|entry| *entry == dependency || entry.starts_with(&format!("{dependency} ")))?;
    match entry.split_once(' ') {
        Some((_, version)) => Some(version.to_string()),
        None => locked_package(lock, dependency)
            .and_then(|block| slice_between(block, "\nversion = \"", "\""))
            .map(str::to_string),
    }
}

fn locked_package<'a>(lock: &'a str, package: &str) -> Option<&'a str> {
    lock.split("[[package]]")
        .find(|block| block.starts_with(&format!("\nname = \"{package}\"\n")))
}

fn check_baked_in_fixtures(root: &Path) -> Result<()> {
    let mut sources = Vec::new();
    for dir in ["crates", "apps", "xtask"] {
        sources.extend(rust_sources(&root.join(dir))?);
    }
    for source in sources {
        let text = std::fs::read_to_string(&source)
            .with_context(|| format!("read {}", source.display()))?;
        for stem in text
            .split("include_bytes!(\"")
            .skip(1)
            .filter_map(|rest| rest.split_once('"').map(|(path, _)| path))
            .filter_map(|path| path.rsplit_once("fixtures/").map(|(_, stem)| stem))
        {
            let rel = format!("fixtures/{stem}");
            let tracked = Command::new("git")
                .args(["ls-files", "--error-unmatch", "--", &rel])
                .current_dir(root)
                .output()
                .context("git ls-files")?
                .status
                .success();
            ensure!(
                tracked,
                "{} bakes in {rel}, which git does not track: the build only works where that \
                 file was generated. Commit it with `git add -f {rel}` (and say why in \
                 fixtures/README.md), or point the test at a fixture it generates itself.",
                source.display()
            );
        }
    }
    Ok(())
}

fn rust_sources(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let path = entry
            .with_context(|| format!("read {}", dir.display()))?
            .path();
        if path.is_dir() {
            out.extend(rust_sources(&path)?);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(out)
}

fn slice_between<'a>(haystack: &'a str, open: &str, close: &str) -> Option<&'a str> {
    Some(haystack.split_once(open)?.1.split_once(close)?.0.trim())
}

fn agree(what: &str, pins: &[(String, String)]) -> Result<()> {
    let (canonical_at, canonical) = &pins[0];
    for (at, value) in pins {
        ensure!(
            value == canonical,
            "{what} is pinned to {canonical} in {canonical_at} but {value} in {at}. \
             These are updated by different tools and must be changed together."
        );
    }
    Ok(())
}

/// Tests, smoke runs and screenshots must not reach whatever SoapySDR a developer has
/// installed, so they are pointed at a library that cannot exist and find none.
const NO_SOAPY_RUNTIME: (&str, &str) = ("SDRMM_SOAPY_LIBRARY", "/nonexistent/libSoapySDR");
const NO_SOAPY_TARGET: &str = "target/no-soapy";

fn desktop(root: &Path, target: Option<&str>, bundles: Option<&str>) -> Result<()> {
    ensure_target(root, target)?;
    let features = release_features();
    let media = media_dir(root, target)?;

    let Some(bundles) = bundles else {
        let mut args = vec![
            "clippy",
            "-p",
            "sdrmm-desktop",
            "--all-targets",
            &features[0],
            &features[1],
            &features[2],
        ];
        if let Some(triple) = target {
            args.push("--target");
            args.push(triple);
        }
        args.extend(["--", "-D", "warnings"]);
        return run_against_media(&args, root, media.as_deref());
    };

    web_build(root)?;
    bundle::check_resources(root)?;

    let installed = Command::new("cargo")
        .args(["tauri", "--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    ensure!(
        installed,
        "the Tauri CLI is missing: `cargo install --locked tauri-cli`"
    );
    let bundled_config = bundled::desktop_config_for(root, target)?;
    let unsigned = std::env::var_os("TAURI_SIGNING_PRIVATE_KEY").is_none();
    if unsigned {
        println!(
            "note: TAURI_SIGNING_PRIVATE_KEY is unset: bundling unsigned, so these installers \
             cannot be served to the updater."
        );
    }
    let mut args = vec![
        "tauri",
        "build",
        "--config",
        &bundled_config,
        "--bundles",
        bundles,
        "--",
        &features[0],
        &features[1],
        &features[2],
    ];
    if let Some(triple) = target {
        args.insert(2, "--target");
        args.insert(3, triple);
    }
    if unsigned {
        args.insert(2, "--no-sign");
    }
    run_against_media(&args, &root.join("apps/desktop"), media.as_deref())
}

fn perf(root: &Path) -> Result<()> {
    run(
        "cargo",
        &[
            "test",
            "-p",
            "sdrmm-dsp",
            "--release",
            "--test",
            "performance",
        ],
        root,
    )?;
    run(
        "cargo",
        &[
            "test",
            "-p",
            "sdrmm-channels",
            "--release",
            "--test",
            "channel_perf",
            "--",
            "--include-ignored",
            "--skip",
            "write_perf_baseline",
            "--test-threads=1",
        ],
        root,
    )?;
    let mut engine = vec![
        "test",
        "-p",
        "sdrmm-engine",
        "--no-default-features",
        "--release",
        "--lib",
        "--",
    ];
    engine.extend(ENGINE_PERF_TESTS.iter().map(|(filter, _)| *filter));
    engine.push("--test-threads=1");
    run("cargo", &engine, root)
}

const ENGINE_PERF_TESTS: &[(&str, &str)] = &[
    (
        "runtime::channel::tests",
        "crates/engine/src/runtime/channel.rs",
    ),
    ("capture_ring::tests", "crates/engine/src/capture_ring.rs"),
    ("publishing::tests", "crates/engine/src/publishing.rs"),
];

#[cfg(test)]
mod perf_tests {
    use super::*;

    #[test]
    fn every_engine_perf_filter_names_a_test_module_that_exists() {
        for (filter, path) in ENGINE_PERF_TESTS {
            let source = std::fs::read_to_string(root().join(path))
                .unwrap_or_else(|error| panic!("{filter} moved away from {path}: {error}"));
            assert!(
                source.contains("mod tests {"),
                "{path} no longer holds the {filter} module"
            );
        }
    }
}

/// Runs every workspace crate under nextest, which schedules all test binaries into one pool
/// rather than running them one at a time. The synthesized SigMF pairs come first because they
/// are never committed, so the tests that read them off disk have nothing to read until the
/// generator has run. Benches stay out rather than arriving with `--all-targets`: a
/// `harness = false` target cannot answer `--list`, and `cargo xtask check` clippies them.
fn test(root: &Path) -> Result<()> {
    ensure_tool("nextest", "cargo-nextest")?;
    fixtures(root)?;
    let media = media_dir(root, None)?
        .map(|dir| media_env(&dir))
        .unwrap_or_default();
    let mut env: Vec<(&str, &str)> = media.iter().map(|(k, v)| (*k, v.as_str())).collect();
    env.push(NO_SOAPY_RUNTIME);
    run_with_env(
        "cargo",
        &[
            "nextest",
            "run",
            "--workspace",
            "--lib",
            "--bins",
            "--tests",
        ],
        root,
        &env,
    )?;
    ensure_web_deps(root)?;
    run(PNPM, &["--dir", "web", "test"], root)?;
    run(PNPM, &["--dir", "site", "test"], root)?;
    Ok(())
}

fn ensure_tool(subcommand: &str, crate_name: &str) -> Result<()> {
    let installed = Command::new("cargo")
        .args([subcommand, "--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    ensure!(
        installed,
        "{crate_name} is missing: `cargo install --locked {crate_name}`"
    );
    Ok(())
}

fn host_target() -> Result<String> {
    let output = Command::new("rustc")
        .arg("-vV")
        .output()
        .context("failed to spawn `rustc`")?;
    let report = String::from_utf8(output.stdout).context("rustc -vV is not UTF-8")?;
    report
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_owned)
        .context("rustc -vV reported no host triple")
}

fn clang_runtime_dir() -> Result<PathBuf> {
    let output = Command::new("clang")
        .arg("-print-runtime-dir")
        .output()
        .context("failed to spawn `clang` (the sanitizer gate compiles the vendored C with it)")?;
    ensure!(output.status.success(), "`clang -print-runtime-dir` failed");
    let dir = PathBuf::from(
        String::from_utf8(output.stdout)
            .context("clang -print-runtime-dir is not UTF-8")?
            .trim(),
    );
    ensure!(
        dir.is_dir(),
        "clang has no sanitizer runtimes at {}",
        dir.display()
    );
    Ok(dir)
}

/// Rust links its own AddressSanitizer runtime through `-Zsanitizer`, but has no flag for
/// UndefinedBehaviorSanitizer, and `-nodefaultlibs` keeps clang from adding one at link time.
fn ubsan_runtime(dir: &Path, target: &str) -> Result<PathBuf> {
    let arch = target.split('-').next().unwrap_or_default();
    let names = if cfg!(target_os = "macos") {
        vec!["libclang_rt.ubsan_osx_dynamic.dylib".to_owned()]
    } else {
        vec![
            format!("libclang_rt.ubsan_standalone-{arch}.so"),
            "libclang_rt.ubsan_standalone.so".to_owned(),
            format!("libclang_rt.ubsan_standalone-{arch}.a"),
            "libclang_rt.ubsan_standalone.a".to_owned(),
        ]
    };
    names
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
        .with_context(|| {
            format!(
                "no UndefinedBehaviorSanitizer runtime in {}: install the clang runtime package",
                dir.display()
            )
        })
}

/// The wall-clock gates read this and stop holding the decoders to real time, because
/// instrumentation costs several times the runtime. Every other assertion still runs.
const SANITIZED_CFG: &str = "--cfg=sanitized";
const NIGHTLY: &str = "+nightly";

/// The version-mismatch guard is off because Apple's clang emits a check that only its own
/// runtime answers, and the sanitized build links Rust's.
const ADDRESS_SANITIZER_CFLAGS: &str =
    "-fsanitize=address -mllvm -asan-guard-against-version-mismatch=0 -fno-omit-frame-pointer -g";

fn encoded_rustflags(flags: &[String]) -> String {
    flags.join("\u{1f}")
}

fn sanitize(root: &Path) -> Result<()> {
    let target = host_target()?;
    let runtime_dir = clang_runtime_dir()?;
    let ubsan = ubsan_runtime(&runtime_dir, &target)?;
    let args = [
        NIGHTLY,
        "test",
        "-p",
        "sdrmm-channels",
        "--target",
        target.as_str(),
        "--lib",
    ];

    let address = encoded_rustflags(&["-Zsanitizer=address".to_owned(), SANITIZED_CFG.to_owned()]);
    run_with_env(
        "cargo",
        &args,
        root,
        &[
            ("CARGO_TARGET_DIR", "target/sanitize-address"),
            ("CC", "clang"),
            ("CFLAGS", ADDRESS_SANITIZER_CFLAGS),
            ("CARGO_ENCODED_RUSTFLAGS", &address),
        ],
    )?;

    let link = encoded_rustflags(&[
        format!("-Clink-arg={}", ubsan.display()),
        format!("-Clink-arg=-Wl,-rpath,{}", runtime_dir.display()),
        SANITIZED_CFG.to_owned(),
    ]);
    run_with_env(
        "cargo",
        &args,
        root,
        &[
            ("CARGO_TARGET_DIR", "target/sanitize-undefined"),
            ("CC", "clang"),
            (
                "CFLAGS",
                "-fsanitize=undefined -fno-sanitize-recover=undefined -fno-omit-frame-pointer -g",
            ),
            ("CARGO_ENCODED_RUSTFLAGS", &link),
        ],
    )
}

const FUZZ_TARGETS: [&str; 9] = [
    "channel_chain",
    "channel_settings",
    "cps_image",
    "dv_voice",
    "iqlink_header",
    "media_bitstream",
    "net_radio_proto",
    "recorder_import",
    "workspace_import",
];
const FUZZ_MEMORY_MB: u32 = 8_192;

fn fuzz_targets(target: Option<&str>) -> Result<Vec<&str>> {
    let Some(name) = target else {
        return Ok(FUZZ_TARGETS.to_vec());
    };
    ensure!(
        FUZZ_TARGETS.contains(&name),
        "unknown fuzz target `{name}`, expected one of: {}",
        FUZZ_TARGETS.join(", ")
    );
    Ok(vec![name])
}

/// Fuzzes on the host triple: cargo-fuzz otherwise defaults to the triple its own binary was
/// built for, and the prebuilt one is a musl static build that no sanitizer can link against.
/// The memory budget is split across workers because each holds its own copy of the corpus and
/// of the target's state, so the runner's memory caps it rather than any one input.
fn fuzz(root: &Path, target: Option<&str>, seconds: u64, jobs: u8, minimize: bool) -> Result<()> {
    ensure_tool("fuzz", "cargo-fuzz")?;
    fuzz_seeds::run(root)?;
    let host = host_target()?;
    let budget = format!("-max_total_time={seconds}");
    let workers = jobs.to_string();
    let rss = format!("-rss_limit_mb={}", FUZZ_MEMORY_MB / u32::from(jobs.max(1)));
    for name in fuzz_targets(target)? {
        if minimize {
            run(
                "cargo",
                &[NIGHTLY, "fuzz", "cmin", "--target", &host, name],
                root,
            )?;
            continue;
        }
        run(
            "cargo",
            &[
                NIGHTLY, "fuzz", "run", "--target", &host, "--jobs", &workers, name, "--", &budget,
                &rss,
            ],
            root,
        )?;
    }
    Ok(())
}

fn audit(root: &Path) -> Result<()> {
    ensure_tool("deny", "cargo-deny")?;
    run(
        "cargo",
        &["deny", "check", "advisories", "licenses", "bans", "sources"],
        root,
    )
}

/// Playwright starts the server with `cargo run -p sdrmm` under a fixed webServer timeout, and
/// that binary pulls in the hardware and GPU features nothing else in the gate builds: a cold
/// cache turns the launch into a full rebuild and the timeout fires before the port opens.
fn build_smoke_server(root: &Path) -> Result<()> {
    run("cargo", &["build", "-p", "sdrmm"], root)
}

fn smoke(root: &Path) -> Result<()> {
    ensure_web_deps(root)?;
    run_with_env(
        PNPM,
        &["--dir", "web", "build"],
        root,
        &[("VITE_ENABLE_SYNTHETIC_DEVICES", "true")],
    )?;
    run(
        "cargo",
        &[
            "build",
            "-p",
            "sdrmm",
            "--no-default-features",
            "--target-dir",
            NO_SOAPY_TARGET,
        ],
        root,
    )?;
    let scratch = root.join("web/.e2e-tmp");
    if scratch.exists() {
        std::fs::remove_dir_all(&scratch)
            .with_context(|| format!("remove {}", scratch.display()))?;
    }
    broadcast_fixtures::run(&scratch.join("recordings"))?;
    run_with_env(
        PNPM,
        &["--dir", "web", "exec", "playwright", "test"],
        root,
        &[NO_SOAPY_RUNTIME, ("E2E_PREBUILT", "1")],
    )?;
    Ok(())
}

fn screenshots(root: &Path) -> Result<()> {
    ensure_web_deps(root)?;
    build_smoke_server(root)?;
    let out = root.join("assets/screenshots");
    std::fs::create_dir_all(&out).context("create screenshot directory")?;
    run_with_env(
        PNPM,
        &[
            "--dir",
            "web",
            "exec",
            "playwright",
            "test",
            "--config",
            "playwright.screenshots.config.ts",
        ],
        root,
        &[NO_SOAPY_RUNTIME],
    )?;
    println!("wrote {}", out.display());
    Ok(())
}

fn fixtures(root: &Path) -> Result<()> {
    const CENTER_HZ: f64 = 100_000_000.0;

    let dir = root.join("fixtures");
    std::fs::create_dir_all(&dir).context("create fixtures dir")?;

    const BAND_RATE: f64 = 2_400_000.0;
    let samples = sdrmm_device_virtual::render(BAND_RATE, BAND_RATE as usize);
    write_fixture(
        &dir,
        "siggen_2m4_1s",
        &samples,
        BAND_RATE,
        CENTER_HZ,
        "Signal Generator (virtual)",
        "1 s of the virtual test band: the record/replay fixture",
    )?;

    for recipe in sdrmm_channels::synth::decoder_fixtures::RECIPES {
        write_fixture(
            &dir,
            recipe.stem,
            &(recipe.iq)(),
            recipe.rate,
            CENTER_HZ,
            "SDR-- reference modulator",
            recipe.note,
        )?;
    }
    Ok(())
}

fn write_fixture(
    dir: &Path,
    stem_name: &str,
    iq: &[Complex<f32>],
    rate: f64,
    center_hz: f64,
    hw: &str,
    note: &str,
) -> Result<()> {
    let stem = dir.join(stem_name);
    for path in [
        sdrmm_recorder::meta_path(&stem),
        sdrmm_recorder::data_path(&stem),
    ] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err).with_context(|| format!("replace {}", path.display())),
        }
    }
    let mut writer = sdrmm_recorder::SigmfWriter::create(&stem, rate, center_hz, hw)
        .with_context(|| format!("create fixture {stem_name}"))?;
    writer
        .write_block(iq)
        .with_context(|| format!("write fixture {stem_name}"))?;
    writer
        .finalize()
        .with_context(|| format!("finalize fixture {stem_name}"))?;

    let reader = sdrmm_recorder::SigmfReader::open(&stem)
        .with_context(|| format!("re-open fixture {stem_name}"))?;
    ensure!(
        reader.total_samples() == iq.len() as u64,
        "fixture {stem_name} readback: {} samples on disk, {} rendered",
        reader.total_samples(),
        iq.len()
    );
    println!(
        "{stem_name}: {} samples, {:.2} s @ {}: {note}",
        iq.len(),
        iq.len() as f64 / rate,
        sdrmm_wire::units::sample_rate(rate),
    );
    Ok(())
}

#[cfg(test)]
mod lock_tests {
    use super::*;

    const LOCK: &str = r#"# This file is automatically @generated by Cargo.

[[package]]
name = "gpu-allocator"
version = "0.28.0"
dependencies = [
 "log",
 "windows 0.62.2",
]

[[package]]
name = "wgpu-hal"
version = "30.0.0"
dependencies = [
 "naga",
 "windows 0.62.2",
 "windows-core 0.62.2",
]

[[package]]
name = "tao"
version = "0.35.3"
dependencies = [
 "windows",
]

[[package]]
name = "windows"
version = "0.61.3"
"#;

    #[test]
    fn reads_the_version_a_duplicated_dependency_resolves_to() {
        assert_eq!(
            locked_dependency(LOCK, "wgpu-hal", "windows").as_deref(),
            Some("0.62.2")
        );
    }

    #[test]
    fn falls_back_to_the_package_entry_when_the_name_stands_alone() {
        assert_eq!(
            locked_dependency(LOCK, "tao", "windows").as_deref(),
            Some("0.61.3")
        );
    }

    #[test]
    fn a_prefix_of_another_crate_name_is_not_a_match() {
        assert_eq!(
            locked_dependency(LOCK, "wgpu-hal", "windows-core").as_deref(),
            Some("0.62.2")
        );
        assert_eq!(locked_dependency(LOCK, "gpu-allocator", "ash"), None);
        assert_eq!(locked_dependency(LOCK, "absent", "windows"), None);
    }

    #[test]
    fn the_workspace_lock_pairs_wgpu_hal_and_gpu_allocator_on_one_windows_rs() {
        check_windows_rs_alignment(&root()).expect("windows-rs alignment");
    }
}
