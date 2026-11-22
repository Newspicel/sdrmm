mod aur;
pub mod bundled;
pub mod changeset;
pub mod desktop;
mod discord;
pub mod dist;
mod ios;
pub mod linkage;
pub mod media;
pub mod pack;
pub mod proc;
mod sums;
mod updater;
mod version;
pub mod web;

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Subcommand;

const HOMEPAGE: &str = "https://sdrmm.com";
const DOWNLOADS: &str = "https://downloads.sdrmm.com/releases";

#[derive(Subcommand)]
pub enum Cmd {
    IosChanged {
        #[arg(long)]
        base: Option<String>,
    },
    SetVersion {
        version: String,
    },
    Changeset {
        bump: changeset::Bump,
        summary: String,
    },
    Release {
        #[arg(long)]
        dry_run: bool,
    },
    ReleaseNotes {
        version: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    Discord {
        version: String,
        #[arg(long)]
        notes: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    UpdaterManifest {
        #[arg(long)]
        version: String,
        #[arg(long)]
        dir: PathBuf,
        #[arg(long)]
        base_url: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    Aur {
        #[arg(long)]
        version: String,
        #[arg(long)]
        sums: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    Dist {
        #[arg(long)]
        target: Option<String>,
    },
    SourceDist,
    LinkCheck {
        path: PathBuf,
        #[arg(long = "external")]
        external: Vec<String>,
    },
}

pub fn run(root: &Path, cmd: &Cmd) -> Result<()> {
    match cmd {
        Cmd::IosChanged { base } => ios::changed(root, base.as_deref()),
        Cmd::SetVersion { version } => version::set(root, version),
        Cmd::Changeset { bump, summary } => changeset::add(root, *bump, summary),
        Cmd::Release { dry_run } => changeset::release(root, *dry_run),
        Cmd::ReleaseNotes { version, out } => changeset::notes(root, version, out.as_deref()),
        Cmd::Discord {
            version,
            notes,
            out,
        } => discord::payload(version, notes, out),
        Cmd::UpdaterManifest {
            version,
            dir,
            base_url,
            out,
        } => updater::manifest(dir, version, base_url, out.as_deref()),
        Cmd::Aur { version, sums, out } => aur::packages(sums, version, out),
        Cmd::Dist { target } => dist::dist(root, target.as_deref()),
        Cmd::SourceDist => dist::source_dist(root),
        Cmd::LinkCheck { path, external } => linkage::check(path, external),
    }
}

/// Found at run time: a path baked in at compile time goes stale when worktrees share a
/// target directory, and then writes land in the wrong checkout.
pub fn root() -> Result<PathBuf> {
    let start = match std::env::var_os("CARGO_MANIFEST_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => std::env::current_dir()?,
    };
    start
        .ancestors()
        .find(|dir| is_workspace_root(dir))
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow::anyhow!("no workspace root above {}", start.display()))
}

fn is_workspace_root(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join("Cargo.toml"))
        .is_ok_and(|manifest| manifest.lines().any(|line| line.trim() == "[workspace]"))
}
