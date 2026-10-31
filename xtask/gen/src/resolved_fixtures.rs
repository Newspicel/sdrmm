use std::{collections::BTreeMap, path::Path};

use anyhow::{Context, Result, ensure};
use sdrmm_wire::{
    PatchGraph, ResolveCtx, StateSnapshot,
    patch::resolve::{bind, compile_draft},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Input {
    name: String,
    graph: Value,
    live: Value,
}

pub(crate) fn contents(root: &Path) -> Result<String> {
    let inputs = root.join("web/src/test/resolved-inputs");
    let mut fixtures = BTreeMap::new();
    let mut files: Vec<_> = std::fs::read_dir(&inputs)
        .with_context(|| format!("read {}", inputs.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    let mut failed = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path)?;
        let input: Input =
            serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
        match resolve(&input) {
            Ok(patch) => {
                fixtures.insert(input.name, json!({ "patch": patch }));
            }
            Err(error) => failed.push(format!("{}: {error:#}", input.name)),
        }
    }
    ensure!(
        failed.is_empty(),
        "unresolvable fixtures:\n{}",
        failed.join("\n")
    );
    Ok(format!("{}\n", serde_json::to_string_pretty(&fixtures)?))
}

fn resolve(input: &Input) -> Result<Value> {
    let graph: PatchGraph = serde_json::from_value(input.graph.clone()).context("graph")?;
    let live: StateSnapshot = serde_json::from_value(input.live.clone()).context("live")?;
    let ctx = ResolveCtx {
        descriptors: Some(sdrmm_channels::descriptor_table()),
        live: Some(&live),
    };
    let (mut patch, _) = compile_draft(&graph, &ctx);
    bind(&mut patch, &graph, &live, &[]);
    Ok(serde_json::to_value(patch)?)
}
