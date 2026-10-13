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

const KIND_MATCHES: [&str; 10] = [
    "calls/tracker.rs",
    "df_fusion/drive/mod.rs",
    "df_fusion/worker.rs",
    "event_output.rs",
    "event_output/beast.rs",
    "event_output/tunnel.rs",
    "fusion_routes.rs",
    "monitor.rs",
    "radar.rs",
    "satellites.rs",
];

fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != "tests") {
                sources(&path, found);
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
    sources(&root, &mut files);
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
