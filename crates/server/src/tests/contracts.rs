use std::path::{Path, PathBuf};

const EDITORS: [&str; 4] = [
    "mcp/canvas.rs",
    "rest/actions/retype.rs",
    "rest/presets.rs",
    "rest/workspaces.rs",
];

const WALKERS: [&str; 10] = [
    ".lanes_of(",
    ".sources_of(",
    ".targets_of(",
    ".channels_of(",
    ".array_lanes(",
    ".array_wired_lanes(",
    ".array_of_processor(",
    ".array_of_lane(",
    ".steer_source(",
    ".position_source(",
];

const KIND_MATCHES: [&str; 8] = [
    "calls/tracker.rs",
    "df_fusion/worker.rs",
    "event_output/beast.rs",
    "event_output/tunnel.rs",
    "fusion_routes.rs",
    "monitor.rs",
    "radar.rs",
    "satellites.rs",
];

const MIGRATIONS: &str = "store/steps/";

const TEST_MODULE: &str = "#[cfg(test)]\nmod ";

fn test_modules(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut skipped = Vec::new();
    for path in entries.flatten().map(|entry| entry.path()) {
        let Ok(code) = std::fs::read_to_string(&path) else {
            continue;
        };
        let home = match path.file_stem().and_then(|stem| stem.to_str()) {
            Some("mod" | "lib") => dir.to_path_buf(),
            Some(stem) => dir.join(stem),
            None => continue,
        };
        for declared in code.split(TEST_MODULE).skip(1) {
            if let Some(name) = declared.split(';').next()
                && !name.contains(['{', '\n', ' '])
            {
                skipped.push(home.join(format!("{name}.rs")));
                skipped.push(home.join(name));
            }
        }
    }
    skipped
}

fn sources(dir: &Path, found: &mut Vec<PathBuf>, skipped: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    skipped.extend(test_modules(dir));
    for entry in entries.flatten() {
        let path = entry.path();
        if skipped.contains(&path) {
            continue;
        }
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != "tests") {
                sources(&path, found, skipped);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && path.file_name().is_some_and(|name| name != "tests.rs")
        {
            found.push(path);
        }
    }
}

fn production(code: &str) -> &str {
    let mut from = 0;
    while let Some(at) = code[from..].find("#[cfg(test)]\nmod ") {
        let start = from + at;
        let line = code[start..].lines().nth(1).unwrap_or_default();
        if line.ends_with('{') {
            return &code[..start];
        }
        from = start + 1;
    }
    code
}

fn production_sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&root, &mut files, &mut Vec::new());
    assert!(files.len() > 50, "the walk found the sources");
    files
        .into_iter()
        .map(|path| {
            let name = path
                .strip_prefix(&root)
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let code = std::fs::read_to_string(&path).unwrap_or_default();
            (name, production(&code).to_owned())
        })
        .collect()
}

#[test]
fn no_server_code_walks_the_graph_for_meaning() {
    let mut offenders = Vec::new();
    for (name, code) in production_sources() {
        for walker in WALKERS {
            if code.contains(walker) {
                offenders.push(format!("{name} calls {walker}"));
            }
        }
        if !EDITORS.contains(&name.as_str()) && code.contains(".edges") {
            offenders.push(format!("{name} reads .edges"));
        }
    }
    assert!(offenders.is_empty(), "{offenders:#?}");
}

#[test]
fn only_typed_consumers_and_producers_name_an_event_kind() {
    let sources = production_sources();
    let naming = |name: &str| {
        sources
            .iter()
            .any(|(held, code)| held == name && code.contains("DecoderEvent::"))
    };
    let offenders: Vec<&str> = sources
        .iter()
        .map(|(name, _)| name.as_str())
        .filter(|name| naming(name) && !KIND_MATCHES.contains(name))
        .collect();
    assert!(offenders.is_empty(), "{offenders:#?}");
    let stale: Vec<&str> = KIND_MATCHES
        .into_iter()
        .filter(|name| !naming(name))
        .collect();
    assert!(stale.is_empty(), "no longer name a kind: {stale:#?}");
}

#[test]
fn test_only_modules_are_not_production() {
    let names: Vec<String> = production_sources()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert!(names.iter().any(|name| name == "df_fusion/mod.rs"));
    assert!(!names.iter().any(|name| name.starts_with("df_fusion/drive")));
}

#[test]
fn no_server_code_keys_meaning_on_a_kind_or_a_port_name() {
    let offenders: Vec<String> = production_sources()
        .into_iter()
        .filter(|(name, _)| !name.starts_with(MIGRATIONS))
        .flat_map(|(name, code)| {
            ["kind == NodeKind::", "port_stream("]
                .into_iter()
                .filter(move |pattern| code.contains(pattern))
                .map(move |pattern| format!("{name} uses {pattern}"))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(offenders.is_empty(), "{offenders:#?}");
}
