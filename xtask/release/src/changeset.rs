use std::{
    collections::BTreeSet,
    fmt::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail, ensure};
use clap::ValueEnum;

const DIR: &str = ".changeset";
const GUIDE: &str = "README.md";
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

const VISIBLE_ROOTS: &[&str] = &["crates/", "apps/", "web/src/"];
const VISIBLE_SUFFIXES: &[&str] = &[".rs", ".ts", ".tsx", ".css", ".kt", ".swift"];
const EXEMPT_SEGMENTS: &[&str] = &[
    "/tests/",
    "/fixtures/",
    "/fuzz/",
    "/benches/",
    "/examples/",
    "/docs/",
    "/__tests__/",
    "web/src/generated/",
];
const EXEMPT_SUFFIXES: &[&str] = &[
    "/tests.rs",
    ".test.ts",
    ".test.tsx",
    ".spec.ts",
    ".spec.tsx",
    ".d.ts",
];
const DEFAULT_BRANCHES: &[&str] = &["origin/main", "origin/master", "main", "master"];
const LISTED_PATHS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum Bump {
    Patch,
    Minor,
    Major,
}

impl Bump {
    fn name(self) -> &'static str {
        match self {
            Self::Patch => "patch",
            Self::Minor => "minor",
            Self::Major => "major",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        [Self::Patch, Self::Minor, Self::Major]
            .into_iter()
            .find(|bump| bump.name() == text)
    }

    fn heading(self) -> &'static str {
        match self {
            Self::Major => "Breaking changes",
            Self::Minor => "Features",
            Self::Patch => "Fixes",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Change {
    bump: Bump,
    summary: String,
    commit: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Dropped {
    path: String,
    summary: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Span {
    changes: Vec<Change>,
    dropped: Vec<Dropped>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Base {
    Rev(String),
    Unknown(String),
}

pub fn add(root: &Path, bump: Bump, summary: &str) -> Result<()> {
    let summary = summary.trim();
    ensure!(!summary.is_empty(), "a changeset needs a summary");
    let dir = root.join(DIR);
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let path = dir.join(format!("{}.md", slug(summary)));
    ensure!(!path.exists(), "{} already exists", path.display());
    let text = format!("---\nbump: {}\n---\n\n{summary}\n", bump.name());
    std::fs::write(&path, text).with_context(|| format!("write {}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

pub fn check(root: &Path) -> Result<()> {
    load(root).map(drop)?;
    match comparison_base(root) {
        Base::Rev(base) => require_changeset(root, &base),
        Base::Unknown(reason) => {
            println!("changeset gate skipped: {reason}");
            Ok(())
        }
    }
}

fn comparison_base(root: &Path) -> Base {
    if !has_commit(root, "HEAD") {
        return Base::Unknown("the repository has no commit to compare against".to_string());
    }
    for branch in DEFAULT_BRANCHES {
        if !has_commit(root, branch) {
            continue;
        }
        if let Ok(base) = git(root, &["merge-base", branch, "HEAD"]) {
            let base = base.trim();
            if !base.is_empty() {
                return Base::Rev(base.to_string());
            }
        }
    }
    let shallow = git(root, &["rev-parse", "--is-shallow-repository"])
        .is_ok_and(|answer| answer.trim() == "true");
    Base::Unknown(if shallow {
        "a shallow clone has no merge base with the default branch".to_string()
    } else {
        "no default branch to take a merge base from".to_string()
    })
}

fn require_changeset(root: &Path, base: &str) -> Result<()> {
    let changed = user_visible_changes(root, base)?;
    if changed.is_empty() {
        return Ok(());
    }
    ensure!(
        !added_changesets(root, base)?.is_empty(),
        "user-visible changes since {base} carry no changeset. Add one with \
         `cargo xtask changeset <patch|minor|major> \"...\"`:\n{}",
        listing(&changed)
    );
    Ok(())
}

fn user_visible_changes(root: &Path, base: &str) -> Result<Vec<String>> {
    let tracked = git(root, &["diff", "--name-only", base])?;
    let untracked = git(root, &["ls-files", "--others", "--exclude-standard"])?;
    let mut paths: Vec<String> = tracked
        .lines()
        .chain(untracked.lines())
        .map(str::trim)
        .filter(|path| user_visible(path))
        .map(str::to_string)
        .collect();
    paths.sort_unstable();
    paths.dedup();
    Ok(paths)
}

fn user_visible(path: &str) -> bool {
    VISIBLE_ROOTS.iter().any(|root| path.starts_with(root))
        && VISIBLE_SUFFIXES.iter().any(|suffix| path.ends_with(suffix))
        && !EXEMPT_SEGMENTS.iter().any(|segment| path.contains(segment))
        && !EXEMPT_SUFFIXES.iter().any(|suffix| path.ends_with(suffix))
}

fn added_changesets(root: &Path, base: &str) -> Result<Vec<String>> {
    let before: BTreeSet<String> = changesets_at(root, base)?.into_iter().collect();
    Ok(working_changesets(root)?
        .into_iter()
        .filter(|path| !before.contains(path))
        .collect())
}

fn working_changesets(root: &Path) -> Result<Vec<String>> {
    Ok(changeset_files(root)?
        .iter()
        .filter_map(|path| path.file_name())
        .map(|name| format!("{DIR}/{}", name.to_string_lossy()))
        .collect())
}

fn listing(paths: &[String]) -> String {
    let mut out = String::new();
    for path in paths.iter().take(LISTED_PATHS) {
        let _ = writeln!(out, "  {path}");
    }
    if paths.len() > LISTED_PATHS {
        let _ = writeln!(out, "  and {} more", paths.len() - LISTED_PATHS);
    }
    out
}

pub fn release(root: &Path, dry_run: bool) -> Result<()> {
    let previous = latest_version(root)?;
    let from = format!("v{previous}");
    let span = changes_between(root, &from, "HEAD")?;
    report_dropped(&span.dropped, &from, "HEAD");
    let changes = span.changes;
    ensure!(
        !changes.is_empty(),
        "nothing to release: no changesets since v{previous}. Add one with `cargo xtask changeset`"
    );
    let bump = changes
        .iter()
        .map(|change| change.bump)
        .max()
        .context("no changesets")?;
    let version = next_version(&previous, bump)?;
    if dry_run {
        print!("v{version}\n\n{}", render(&changes));
        return Ok(());
    }
    ensure_clean(root)?;
    tag(root, &version)?;
    println!("tagged v{version}. Publish with `git push origin v{version}`");
    Ok(())
}

pub fn notes(root: &Path, version: &str, out: Option<&Path>) -> Result<()> {
    let version = version.strip_prefix('v').unwrap_or(version);
    let previous = previous_version(root, version)?;
    let from = format!("v{previous}");
    let to = format!("v{version}");
    let span = changes_between(root, &from, &to)?;
    report_dropped(&span.dropped, &from, &to);
    let changes = span.changes;
    ensure!(
        !changes.is_empty(),
        "v{version} has no changesets since v{previous}"
    );
    let notes = render(&changes);
    match out {
        Some(out) => {
            std::fs::write(out, notes).with_context(|| format!("write {}", out.display()))?;
        }
        None => print!("{notes}"),
    }
    Ok(())
}

fn changes_between(root: &Path, from: &str, to: &str) -> Result<Span> {
    let before: BTreeSet<String> = changesets_at(root, from)?.into_iter().collect();
    let present = changesets_at(root, to)?;
    let changes = present
        .iter()
        .filter(|path| !before.contains(*path))
        .map(|path| {
            let text = git(root, &["show", &format!("{to}:{path}")])?;
            let mut change = parse(&text).with_context(|| path.clone())?;
            change.commit = added_in(root, to, path)?;
            Ok(change)
        })
        .collect::<Result<Vec<Change>>>()?;
    Ok(Span {
        changes,
        dropped: dropped_between(root, from, to, &present)?,
    })
}

fn dropped_between(root: &Path, from: &str, to: &str, present: &[String]) -> Result<Vec<Dropped>> {
    if !has_commit(root, to) {
        return Ok(Vec::new());
    }
    let range = if has_commit(root, from) {
        format!("{from}..{to}")
    } else {
        to.to_string()
    };
    let listing = git(
        root,
        &[
            "log",
            "--diff-filter=A",
            "--format=",
            "--name-only",
            &range,
            "--",
            &format!("{DIR}/"),
        ],
    )?;
    let mut paths: Vec<String> = listing
        .lines()
        .filter(|path| is_changeset(path) && !present.contains(&(*path).to_string()))
        .map(str::to_string)
        .collect();
    paths.sort_unstable();
    paths.dedup();
    Ok(paths
        .into_iter()
        .map(|path| Dropped {
            summary: dropped_summary(root, to, &path),
            path,
        })
        .collect())
}

fn dropped_summary(root: &Path, rev: &str, path: &str) -> Option<String> {
    let hash = added_in(root, rev, path).ok().flatten()?;
    let text = git(root, &["show", &format!("{hash}:{path}")]).ok()?;
    parse(&text).ok().map(|change| change.summary)
}

fn report_dropped(dropped: &[Dropped], from: &str, to: &str) {
    if dropped.is_empty() {
        return;
    }
    eprintln!(
        "warning: {} changesets were added after {from} and deleted before {to}, so no release \
         notes carry them:",
        dropped.len()
    );
    for item in dropped {
        match item.summary.as_deref().and_then(|text| text.lines().next()) {
            Some(summary) => eprintln!("  {}: {summary}", item.path),
            None => eprintln!("  {}", item.path),
        }
    }
}

fn is_changeset(path: &str) -> bool {
    path.ends_with(".md") && path.rsplit('/').next() != Some(GUIDE)
}

fn has_commit(root: &Path, rev: &str) -> bool {
    git(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{rev}^{{commit}}"),
        ],
    )
    .is_ok()
}

fn changesets_at(root: &Path, rev: &str) -> Result<Vec<String>> {
    if !has_commit(root, rev) {
        return Ok(Vec::new());
    }
    let listing = git(root, &["ls-tree", "--name-only", rev, &format!("{DIR}/")])?;
    Ok(listing
        .lines()
        .filter(|path| is_changeset(path))
        .map(str::to_string)
        .collect())
}

fn changeset_files(root: &Path) -> Result<Vec<PathBuf>> {
    let dir = root.join(DIR);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
        let path = entry?.path();
        let markdown = path.extension().is_some_and(|ext| ext == "md");
        if markdown && path.file_name().is_some_and(|name| name != GUIDE) {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn load(root: &Path) -> Result<Vec<Change>> {
    changeset_files(root)?
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("read {}", path.display()))?;
            parse(&text).with_context(|| format!("{}", path.display()))
        })
        .collect()
}

fn parse(text: &str) -> Result<Change> {
    let text = text.replace("\r\n", "\n");
    let rest = text
        .strip_prefix("---\n")
        .context("starts without a `---` front matter block")?;
    let (front, body) = rest
        .split_once("\n---\n")
        .context("front matter has no closing `---`")?;
    let mut bump = None;
    for line in front.lines().filter(|line| !line.trim().is_empty()) {
        let (key, value) = line
            .split_once(':')
            .with_context(|| format!("`{line}` is not `key: value`"))?;
        match key.trim() {
            "bump" => {
                let value = value.trim().trim_matches('"');
                bump = Some(Bump::parse(value).with_context(|| {
                    format!("bump `{value}` is not one of patch, minor, major")
                })?);
            }
            other => bail!("unknown front matter key `{other}`"),
        }
    }
    let summary = body.trim();
    ensure!(!summary.is_empty(), "has no summary");
    Ok(Change {
        bump: bump.context("front matter has no `bump`")?,
        summary: summary.to_string(),
        commit: None,
    })
}

fn slug(summary: &str) -> String {
    let words: Vec<String> = summary
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(6)
        .map(str::to_ascii_lowercase)
        .collect();
    if words.is_empty() {
        "change".to_string()
    } else {
        words.join("-")
    }
}

fn render(changes: &[Change]) -> String {
    let mut out = String::new();
    for bump in [Bump::Major, Bump::Minor, Bump::Patch] {
        let group: Vec<&Change> = changes.iter().filter(|c| c.bump == bump).collect();
        if group.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        let _ = write!(out, "### {}\n\n", bump.heading());
        for change in group {
            out.push_str(&item(change));
        }
    }
    out
}

fn item(change: &Change) -> String {
    let summary = match &change.commit {
        Some(hash) => with_link(&change.summary, hash),
        None => change.summary.clone(),
    };
    let mut out = String::new();
    for (index, line) in summary.lines().enumerate() {
        let prefix = match (index, line.is_empty()) {
            (0, _) => "- ",
            (_, true) => "",
            _ => "  ",
        };
        let _ = writeln!(out, "{prefix}{line}");
    }
    out
}

fn with_link(summary: &str, hash: &str) -> String {
    let short = hash.get(..7).unwrap_or(hash);
    let link = format!(" ([{short}]({REPOSITORY}/commit/{hash}))");
    match summary.split_once("\n\n") {
        Some((first, rest)) => format!("{first}{link}\n\n{rest}"),
        None => format!("{summary}{link}"),
    }
}

fn parse_version(text: &str) -> Option<[u64; 3]> {
    let mut parts = text.split('.').map(|part| part.parse::<u64>().ok());
    let version = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some(version)
}

fn next_version(previous: &str, bump: Bump) -> Result<String> {
    let [major, minor, patch] = parse_version(previous)
        .with_context(|| format!("`{previous}` is not a major.minor.patch version"))?;
    let [major, minor, patch] = match bump {
        Bump::Major => [major + 1, 0, 0],
        Bump::Minor => [major, minor + 1, 0],
        Bump::Patch => [major, minor, patch + 1],
    };
    Ok(format!("{major}.{minor}.{patch}"))
}

fn versions(root: &Path) -> Result<Vec<[u64; 3]>> {
    let tags = git(root, &["tag", "--list", "v*"])?;
    let mut versions: Vec<[u64; 3]> = tags
        .lines()
        .filter_map(|tag| tag.strip_prefix('v').and_then(parse_version))
        .collect();
    versions.sort_unstable();
    Ok(versions)
}

fn format_version([major, minor, patch]: [u64; 3]) -> String {
    format!("{major}.{minor}.{patch}")
}

fn latest_version(root: &Path) -> Result<String> {
    Ok(format_version(
        versions(root)?.last().copied().unwrap_or_default(),
    ))
}

fn previous_version(root: &Path, version: &str) -> Result<String> {
    let current = parse_version(version)
        .with_context(|| format!("`{version}` is not a major.minor.patch version"))?;
    Ok(format_version(
        versions(root)?
            .into_iter()
            .rfind(|tag| *tag < current)
            .unwrap_or_default(),
    ))
}

fn added_in(root: &Path, rev: &str, path: &str) -> Result<Option<String>> {
    let hash = git(
        root,
        &[
            "log",
            "--diff-filter=A",
            "--format=%H",
            "-1",
            rev,
            "--",
            path,
        ],
    )?;
    let hash = hash.trim();
    Ok((!hash.is_empty()).then(|| hash.to_string()))
}

fn ensure_clean(root: &Path) -> Result<()> {
    let status = git(root, &["status", "--porcelain"])?;
    ensure!(
        status.trim().is_empty(),
        "the working tree has uncommitted changes. Commit them first, so every changeset links \
         to the commit that added it:\n{status}"
    );
    Ok(())
}

fn tag(root: &Path, version: &str) -> Result<()> {
    git(
        root,
        &[
            "tag",
            "--annotate",
            &format!("v{version}"),
            "--message",
            &format!("v{version}"),
        ],
    )?;
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .context("failed to spawn git")?;
    ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    String::from_utf8(output.stdout).context("git printed non UTF-8 output")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn change(bump: Bump, summary: &str, commit: Option<&str>) -> Change {
        Change {
            bump,
            summary: summary.to_string(),
            commit: commit.map(str::to_string),
        }
    }

    #[test]
    fn parses_front_matter_and_summary() {
        let parsed = parse("---\nbump: minor\n---\n\nAdds a thing.\n").expect("parses");
        assert_eq!(parsed, change(Bump::Minor, "Adds a thing.", None));
        let quoted = parse("---\r\nbump: \"patch\"\r\n---\r\nFix.\r\n").expect("parses");
        assert_eq!(quoted.bump, Bump::Patch);
    }

    #[test]
    fn rejects_broken_changesets() {
        for text in [
            "Adds a thing.",
            "---\nbump: minor\n",
            "---\nbump: huge\n---\nText",
            "---\nkind: minor\n---\nText",
            "---\n---\nText",
            "---\nbump: minor\n---\n\n",
        ] {
            assert!(parse(text).is_err(), "{text:?} parsed");
        }
    }

    #[test]
    fn slugs_follow_the_summary() {
        assert_eq!(
            slug("RTL-SDR: fix gain after reconnect on macOS 15"),
            "rtl-sdr-fix-gain-after-reconnect"
        );
        assert_eq!(slug("…"), "change");
    }

    #[test]
    fn bumps_from_the_previous_version() {
        assert_eq!(next_version("1.9.0", Bump::Patch).expect("bumps"), "1.9.1");
        assert_eq!(next_version("1.9.3", Bump::Minor).expect("bumps"), "1.10.0");
        assert_eq!(next_version("1.9.3", Bump::Major).expect("bumps"), "2.0.0");
        assert!(next_version("1.9", Bump::Patch).is_err());
        assert!(parse_version("1.2.3.4").is_none());
    }

    #[test]
    fn renders_groups_in_order_with_links() {
        let changes = [
            change(Bump::Patch, "Fix A.", None),
            change(
                Bump::Minor,
                "Add B.\n\n```yaml\nb: 1\n```",
                Some("abcdef123456"),
            ),
            change(Bump::Major, "Drop C.", None),
        ];
        let expected = format!(
            "### Breaking changes\n\n- Drop C.\n\n### Features\n\n\
             - Add B. ([abcdef1]({REPOSITORY}/commit/abcdef123456))\n\n  ```yaml\n  b: 1\n  ```\n\n\
             ### Fixes\n\n- Fix A.\n"
        );
        assert_eq!(render(&changes), expected);
    }

    fn repo(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("changeset-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(DIR)).expect("create repo");
        for args in [
            &["init", "--quiet", "--initial-branch", "main"][..],
            &["config", "user.name", "Test"],
            &["config", "user.email", "test@example.com"],
            &["config", "commit.gpgsign", "false"],
            &["config", "tag.gpgsign", "false"],
        ] {
            git(&dir, args).expect("git setup");
        }
        dir
    }

    fn commit_changeset(root: &Path, name: &str, bump: Bump) {
        add(root, bump, name).expect("add changeset");
        git(root, &["add", "--all"]).expect("stage");
        git(root, &["commit", "--quiet", "--message", name]).expect("commit");
    }

    #[test]
    fn notes_cover_changesets_added_since_the_previous_tag() {
        let root = repo("notes");
        commit_changeset(&root, "Old fix", Bump::Patch);
        tag(&root, "1.0.0").expect("tag");
        commit_changeset(&root, "New feature", Bump::Minor);
        tag(&root, "1.1.0").expect("tag");
        commit_changeset(&root, "Pending fix", Bump::Patch);

        let released = changes_between(&root, "v1.0.0", "v1.1.0").expect("released");
        assert_eq!(released.changes.len(), 1);
        assert_eq!(released.changes[0].summary, "New feature");
        assert!(released.changes[0].commit.is_some());
        assert!(released.dropped.is_empty());
        assert_eq!(previous_version(&root, "1.1.0").expect("previous"), "1.0.0");
        assert_eq!(previous_version(&root, "1.0.0").expect("previous"), "0.0.0");
        assert_eq!(latest_version(&root).expect("latest"), "1.1.0");
        let pending = changes_between(&root, "v1.1.0", "HEAD").expect("pending");
        assert_eq!(pending.changes.len(), 1);
        assert_eq!(pending.changes[0].summary, "Pending fix");
        assert_eq!(
            changes_between(&root, "v0.0.0", "v1.0.0")
                .expect("first")
                .changes
                .len(),
            1
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_changeset_added_and_then_deleted_between_tags_surfaces() {
        let root = repo("dropped");
        commit_changeset(&root, "Old fix", Bump::Patch);
        tag(&root, "1.0.0").expect("tag");
        commit_changeset(&root, "Reworked later", Bump::Minor);
        commit_changeset(&root, "Kept feature", Bump::Minor);
        std::fs::remove_file(root.join(DIR).join("reworked-later.md")).expect("remove");
        git(&root, &["add", "--all"]).expect("stage");
        git(&root, &["commit", "--quiet", "--message", "Rework"]).expect("commit");
        tag(&root, "1.1.0").expect("tag");

        let span = changes_between(&root, "v1.0.0", "v1.1.0").expect("span");
        assert_eq!(span.changes.len(), 1);
        assert_eq!(span.changes[0].summary, "Kept feature");
        assert_eq!(
            span.dropped,
            vec![Dropped {
                path: format!("{DIR}/reworked-later.md"),
                summary: Some("Reworked later".to_string()),
            }]
        );
        let released = changes_between(&root, "v0.0.0", "v1.0.0").expect("first");
        assert!(released.dropped.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn released_changesets_pruned_after_their_tag_are_not_dropped() {
        let root = repo("pruned");
        commit_changeset(&root, "Shipped fix", Bump::Patch);
        tag(&root, "1.0.0").expect("tag");
        std::fs::remove_file(root.join(DIR).join("shipped-fix.md")).expect("remove");
        git(&root, &["add", "--all"]).expect("stage");
        git(&root, &["commit", "--quiet", "--message", "Prune"]).expect("commit");
        commit_changeset(&root, "Next fix", Bump::Patch);
        tag(&root, "1.0.1").expect("tag");

        let span = changes_between(&root, "v1.0.0", "v1.0.1").expect("span");
        assert_eq!(span.changes.len(), 1);
        assert!(span.dropped.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn user_visible_covers_shipped_source_only() {
        for path in [
            "crates/channels/src/aprs.rs",
            "apps/sdrmm/src/main.rs",
            "web/src/panels/Scope.tsx",
            "web/src/theme.css",
            "apps/android/app/src/main/java/Main.kt",
        ] {
            assert!(user_visible(path), "{path}");
        }
        for path in [
            "crates/channels/src/dv/dmr/tests.rs",
            "crates/wire/src/patch/tests/registry.rs",
            "crates/channels/fuzz/fuzz_targets/aprs.rs",
            "crates/dsp/benches/fft.rs",
            "crates/channels/tests/fixtures/aprs.bin",
            "web/src/generated/frame.ts",
            "web/src/panels/Scope.test.tsx",
            "xtask/src/main.rs",
            "docs/src/wires.md",
            ".github/workflows/ci.yml",
            "Cargo.lock",
            "crates/channels/README.md",
        ] {
            assert!(!user_visible(path), "{path}");
        }
    }

    fn write_source(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create dir");
        }
        std::fs::write(&path, text).expect("write source");
    }

    #[test]
    fn the_gate_wants_a_changeset_for_user_visible_work() {
        let root = repo("gate");
        write_source(&root, "crates/dsp/src/lib.rs", "pub fn one() {}\n");
        git(&root, &["add", "--all"]).expect("stage");
        git(&root, &["commit", "--quiet", "--message", "Base"]).expect("commit");
        let base = git(&root, &["rev-parse", "HEAD"]).expect("base");
        let base = base.trim();

        write_source(&root, "crates/dsp/src/tests.rs", "fn two() {}\n");
        assert!(require_changeset(&root, base).is_ok());

        write_source(&root, "crates/dsp/src/lib.rs", "pub fn two() {}\n");
        let refused = require_changeset(&root, base).expect_err("refused");
        assert!(
            refused.to_string().contains("crates/dsp/src/lib.rs"),
            "{refused}"
        );

        add(&root, Bump::Patch, "DSP counts to two").expect("add changeset");
        require_changeset(&root, base).expect("accepted");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_gate_skips_without_a_base() {
        let root = repo("base");
        assert_eq!(
            comparison_base(&root),
            Base::Unknown("the repository has no commit to compare against".to_string())
        );
        commit_changeset(&root, "First fix", Bump::Patch);
        assert!(matches!(comparison_base(&root), Base::Rev(_)));
        let _ = std::fs::remove_dir_all(&root);
    }
}
