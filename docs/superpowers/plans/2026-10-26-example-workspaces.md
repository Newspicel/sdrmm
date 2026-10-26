# Example Workspaces Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship eleven complete workspaces inside the binary so a newcomer can import a working receiver and read, on the canvas, which nodes the job needs.

**Architecture:** Each example is a plain `WorkspaceExport` JSON file under `crates/server/data/examples/`, already gzipped by the existing `build.rs` tree walk and reached through `packed_data!`. A static index in `crates/server/src/examples.rs` names them. Listing derives every requirement from the parsed graph, so metadata cannot drift. Import reuses the exact path a hand-dropped file takes.

**Tech Stack:** Rust (axum, utoipa, serde, rusqlite), React with TanStack Query, Base UI tabs, Biome.

**Spec:** `docs/superpowers/specs/2026-10-26-example-workspaces-design.md`

## Global Constraints

- One source of truth for wire types: `ExampleInfo` and `ExampleNeeds` live in `crates/wire/src/rest.rs` and nowhere else.
- Never write a comment. Names carry the meaning.
- No `unwrap()` or `expect()` outside tests and startup.
- Max 3000 lines per file, 200 lines per function.
- Keep `crates/wire` free of I/O; derivation takes a `&PatchGraph` and returns a value.
- `WORKSPACE_EXPORT_VERSION` is `2`. Every shipped example file carries `"version": 2`.
- Every example file is a plain `WorkspaceExport`: `version`, `name`, `snapshot`, optional `state`. No extra fields.
- Every example ships with at least one `note` node pinned in the rack.
- Every `DeviceNode` and `RecordingNode` in an example file omits `device` and `recording`, so it imports unbound.
- Format, lint, check and test only what the task changed. `cargo fmt`, `cargo clippy`, `cargo test -p <crate>`, and `pnpm biome check` plus `pnpm vitest run <path>` for web work.
- User-visible change gets `cargo xtask changeset minor "..."`, once, in the last task.
- Text stays short. No em dash anywhere, including example descriptions and Note bodies.

## Review Focus

- An example id in the URL that is not in the index must be 404, not a panic or a 500.
- An example whose embedded bytes fail to parse must surface as an error naming the id, never a silent skip that shortens the list.
- Importing the same example twice must give two workspaces, the second renamed by `free_workspace_name`, not a collision or an overwrite.
- An example with zero `Device` nodes must report `radios: 0` and stay importable, since `no-radio` is the first thing a newcomer without hardware opens.
- An `Array` node wired with one lane must derive `lanes: 1`, not panic on an empty max, and must not be reported as a two-lane requirement.

---

### Task 1: Example requirements as a wire type

**Files:**
- Modify: `crates/wire/src/rest.rs` (add beside `TemplateInfo`, around line 571)
- Modify: `crates/wire/src/lib.rs` (re-export `ExampleInfo`, `ExampleNeeds`, `ExamplesResponse`)
- Test: `crates/wire/src/rest.rs` in its existing `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `PatchGraph`, `NodeBody`, `NodeKind`, `ArrayNode`, `ARRAY_LANE_PORT` from `crate::patch` and `crate::array`.
- Produces:
  - `pub struct ExampleNeeds { pub radios: u32, pub lanes: u32, pub recordings: u32, pub min_freq_hz: f64, pub max_freq_hz: f64 }`
  - `pub struct ExampleInfo { pub id: String, pub name: String, pub description: String, pub needs: ExampleNeeds, pub node_kinds: Vec<NodeKind> }`
  - `pub struct ExamplesResponse { pub examples: Vec<ExampleInfo> }`
  - `impl ExampleNeeds { pub fn of(graph: &PatchGraph) -> Self }`
  - `pub fn kinds_in(graph: &PatchGraph) -> Vec<NodeKind>` sorted and deduplicated, named apart from the `node_kinds` field it fills

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn needs_counts_sources_and_lanes() {
    let graph = graph_with(&[
        ("dev", NodeBody::Device(DeviceNode::default())),
        ("rec", NodeBody::Recording(RecordingNode::default())),
        ("arr", NodeBody::Array(ArrayNode::default())),
    ], &[("dev", ARRAY_LANE_PORT, "arr"), ("rec", ARRAY_LANE_PORT, "arr")]);
    let needs = ExampleNeeds::of(&graph);
    assert_eq!(needs.radios, 1);
    assert_eq!(needs.recordings, 1);
    assert_eq!(needs.lanes, 2);
}

#[test]
fn needs_of_a_graph_without_sources_is_zero() {
    let graph = graph_with(&[("gen", NodeBody::SignalGen(SignalGenNode::default()))], &[]);
    let needs = ExampleNeeds::of(&graph);
    assert_eq!(needs.radios, 0);
    assert_eq!(needs.recordings, 0);
    assert_eq!(needs.lanes, 0);
}

#[test]
fn a_single_lane_array_needs_one_lane() {
    let graph = graph_with(&[
        ("dev", NodeBody::Device(DeviceNode::default())),
        ("arr", NodeBody::Array(ArrayNode::default())),
    ], &[("dev", ARRAY_LANE_PORT, "arr")]);
    assert_eq!(ExampleNeeds::of(&graph).lanes, 1);
}

#[test]
fn kinds_in_is_sorted_and_deduplicated() {
    let graph = graph_with(&[
        ("s", NodeBody::Speaker),
        ("d", NodeBody::Device(DeviceNode::default())),
        ("s2", NodeBody::Speaker),
    ], &[]);
    assert_eq!(kinds_in(&graph), vec![NodeKind::Device, NodeKind::Speaker]);
}
```

Write a `graph_with(nodes, lane_edges)` helper in the test module that builds a `PatchGraph` from id and body pairs and from lane edges given as source id, port, target id.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sdrmm-wire needs_ kinds_in`
Expected: FAIL, `ExampleNeeds` not found.

- [ ] **Step 3: Implement the types and `ExampleNeeds::of` plus `kinds_in` in `crates/wire/src/rest.rs`**

`radios` and `recordings` count matching `NodeBody` variants. `lanes` is the largest number of edges whose `to.port` is `ARRAY_LANE_PORT` arriving at any one `Array` node, and is `0` when the graph has no `Array` node, so the max is taken over an empty iterator with `unwrap_or(0)` rather than indexing. Frequency span comes from each `Channel` node's settings against the center of the device feeding it; when the graph has no channel both bounds are `0.0`. Derive `Serialize`, `Deserialize`, `ToSchema` on all three structs; `NodeKind` already derives `Ord`, so sort and dedup directly.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p sdrmm-wire needs_ kinds_in`
Expected: PASS

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt && cargo clippy -p sdrmm-wire --all-targets
git add crates/wire/src/rest.rs crates/wire/src/lib.rs
git commit -m "Examples: requirements derived from the graph"
```

---

### Task 2: The index, the first example, and the file tests

**Files:**
- Create: `crates/server/data/examples/no-radio.json`
- Create: `crates/server/src/examples.rs`
- Modify: `crates/server/src/lib.rs` (add `mod examples;`)
- Test: `crates/server/src/examples.rs` in a `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `ExampleInfo`, `ExampleNeeds`, `kinds_in` from Task 1; `crate::packed::{packed_data, inflate}`; `crate::store::{upgrade_import, Store}`.
- Produces:
  - `struct Example { id: &'static str, name: &'static str, description: &'static str, body: &'static [u8] }`
  - `static EXAMPLES: &[Example]`
  - `pub(crate) fn all() -> Result<Vec<ExampleInfo>, ExampleError>`
  - `pub(crate) fn export(id: &str) -> Result<serde_json::Value, ExampleError>` returning the inflated document ready for `upgrade_import`
  - `fn parse(id: &str, body: &[u8]) -> Result<serde_json::Value, ExampleError>`, the seam `export` calls, so malformed bytes can be tested without shipping a bad file
  - `pub(crate) enum ExampleError { Unknown(String), Malformed { id: String, detail: String } }`

The `no-radio` graph: a `signal_gen` wired to a `scope`, a `channel` on it, the channel's baseband into a `baseband_scope`, its audio into a `speaker`, and a `note` pinned to the rack. Hand-write the JSON against `WorkspaceExport` and `WorkspaceSnapshot` in `crates/wire/src/workspace.rs`; `crates/server/src/store/steps/tests.rs` has export documents to copy the shape from. The import test is what proves the file is valid, so write the file, run the test, and fix what it reports.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn every_example_parses_and_lists() {
    let listed = all().expect("examples list");
    assert_eq!(listed.len(), EXAMPLES.len());
}

#[test]
fn ids_are_unique_and_match_their_filenames() {
    let mut seen = std::collections::BTreeSet::new();
    for example in EXAMPLES {
        assert!(seen.insert(example.id), "duplicate id {}", example.id);
        let path = format!("data/examples/{}.json", example.id);
        assert!(std::path::Path::new(&path).exists(), "missing {path}");
    }
}

#[test]
fn every_example_carries_a_note() {
    for info in all().expect("examples list") {
        assert!(info.node_kinds.contains(&NodeKind::Note), "{} has no note", info.id);
    }
}

#[test]
fn an_unknown_id_is_an_unknown_error() {
    assert!(matches!(export("nope"), Err(ExampleError::Unknown(_))));
}

#[test]
fn malformed_bytes_name_the_example() {
    let err = parse("broken", b"not gzip").expect_err("malformed");
    assert!(matches!(&err, ExampleError::Malformed { id, .. } if id == "broken"));
}

#[test]
fn the_no_radio_example_needs_no_hardware() {
    let info = all().expect("examples list").into_iter().find(|e| e.id == "no-radio").expect("no-radio");
    assert_eq!(info.needs.radios, 0);
    assert_eq!(info.needs.recordings, 0);
}

#[test]
fn every_example_imports() {
    let store = crate::store::tests::memory_store();
    for example in EXAMPLES {
        let mut document = export(example.id).expect("export");
        let notices = crate::store::upgrade_import(&mut document);
        let parsed: sdrmm_wire::WorkspaceExport = crate::json::from_value(&document).expect(example.id);
        store.import_workspace(&parsed, &notices).expect(example.id);
    }
}
```

Use the in-memory store helper the existing `crates/server/src/tests/workspaces.rs` uses; match its name rather than adding one.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sdrmm-server examples::`
Expected: FAIL, module `examples` not found.

- [ ] **Step 3: Write `crates/server/data/examples/no-radio.json` and implement `examples.rs`**

`body` is `packed_data!("examples/no-radio.json")`. `export` inflates, parses to `serde_json::Value`, and maps any failure to `ExampleError::Malformed` carrying the id, so a bad file is loud rather than a short list. `all` maps every entry through `export`, parses to `WorkspaceExport`, and fills `ExampleInfo` from the index plus `ExampleNeeds::of` and `kinds_in` over `export.snapshot.graph`; a single malformed file fails the whole call.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p sdrmm-server examples::`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy -p sdrmm-server --all-targets
git add crates/server/data/examples/no-radio.json crates/server/src/examples.rs crates/server/src/lib.rs
git commit -m "Examples: the index and a workspace that needs no radio"
```

---

### Task 3: List and import over REST

**Files:**
- Modify: `crates/server/src/rest/workspaces.rs` (add beside `list_templates` at line 13 and `import_workspace` at line 480)
- Modify: `crates/server/src/rest.rs` (two `routes!` lines beside `list_templates` at line 436)
- Modify: `openapi.json` (regenerated, not hand-edited)
- Test: `crates/server/src/tests/examples.rs`, registered in `crates/server/src/tests.rs`

**Interfaces:**
- Consumes: `crate::examples::{all, export, ExampleError}` from Task 2.
- Produces:
  - `GET /api/examples` returning `ExamplesResponse`
  - `POST /api/examples/{id}/import` returning `CreatedRowId`
  - `ExampleError` converted to `AppError`: `Unknown` to 404 with `ErrorCode::NotFound`, `Malformed` to 500 naming the id

- [ ] **Step 1: Write the failing tests**

```rust
#[tokio::test]
async fn examples_list_every_shipped_workspace() {
    let app = test_app().await;
    let listed: ExamplesResponse = get_json(&app, "/api/examples").await;
    assert!(listed.examples.iter().any(|e| e.id == "no-radio"));
}

#[tokio::test]
async fn importing_an_example_creates_a_workspace() {
    let app = test_app().await;
    let created: CreatedRowId = post_json(&app, "/api/examples/no-radio/import", ()).await;
    let workspaces: WorkspacesResponse = get_json(&app, "/api/workspaces").await;
    assert!(workspaces.workspaces.iter().any(|w| w.id == created.id));
}

#[tokio::test]
async fn importing_twice_gives_two_workspaces_with_distinct_names() {
    let app = test_app().await;
    let first: CreatedRowId = post_json(&app, "/api/examples/no-radio/import", ()).await;
    let second: CreatedRowId = post_json(&app, "/api/examples/no-radio/import", ()).await;
    assert_ne!(first.id, second.id);
    let workspaces: WorkspacesResponse = get_json(&app, "/api/workspaces").await;
    let names: Vec<_> = workspaces.workspaces.iter().map(|w| w.name.clone()).collect();
    assert_eq!(names.iter().collect::<std::collections::BTreeSet<_>>().len(), names.len());
}

#[tokio::test]
async fn an_unknown_example_is_not_found() {
    let app = test_app().await;
    let status = post_status(&app, "/api/examples/nope/import").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
```

Reuse the harness helpers `crates/server/src/tests/workspaces.rs` already uses for `test_app`, `get_json` and posting; match their names rather than adding new ones.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sdrmm-server tests::examples`
Expected: FAIL, route not found, 404 on the list.

- [ ] **Step 3: Implement `list_examples` and `import_example` in `crates/server/src/rest/workspaces.rs`**

`list_examples` calls `examples::all` on a blocking task. `import_example` takes `Path<String>`, calls `examples::export`, then the same two calls `import_workspace` makes at line 484: `crate::store::upgrade_import` then `store.import_workspace`, followed by `engine.emit_scope(StateScope::Workspaces)`. Both carry `#[utoipa::path]` annotations matching the shape of the template pair above them.

- [ ] **Step 4: Run tests and regenerate the API document**

Run: `cargo test -p sdrmm-server tests::examples` then `cargo xtask openapi`
Expected: PASS, and `openapi.json` gains the two paths.

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy -p sdrmm-server --all-targets
git add crates/server/src/rest/workspaces.rs crates/server/src/rest.rs crates/server/src/tests.rs crates/server/src/tests/examples.rs openapi.json
git commit -m "Examples: list them and import one"
```

---

### Task 4: The Examples tab

**Files:**
- Create: `web/src/components/ExamplesPanel.tsx`
- Create: `web/src/components/examples.ts`
- Create: `web/src/components/examples.test.ts`
- Modify: `web/src/canvas/Library.tsx` (the `TABS` array at line 18 and the panel switch)
- Modify: `web/src/lib/api.ts` (add `examplesQuery` and `importExample` beside `templatesQuery` and `applyTemplate`)
- Modify: `web/src/lib/types.ts` (regenerated from `openapi.json`, not hand-edited)

**Interfaces:**
- Consumes: `GET /api/examples` and `POST /api/examples/{id}/import` from Task 3.
- Produces: `exampleNeeds(example: ExampleInfo): string` for the row badge.

- [ ] **Step 1: Write the failing test**

```ts
describe("exampleNeeds", () => {
  it("says nothing is needed when there are no sources", () => {
    expect(exampleNeeds(example({ radios: 0, lanes: 0, recordings: 0 }))).toBe("no radio");
  });
  it("counts radios", () => {
    expect(exampleNeeds(example({ radios: 1, lanes: 0, recordings: 0 }))).toBe("1 radio");
  });
  it("prefers lanes when the example is an array", () => {
    expect(exampleNeeds(example({ radios: 5, lanes: 5, recordings: 0 }))).toBe("5 lanes");
  });
  it("counts recordings", () => {
    expect(exampleNeeds(example({ radios: 0, lanes: 0, recordings: 1 }))).toBe("1 recording");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm vitest run src/components/examples.test.ts`
Expected: FAIL, `exampleNeeds` not exported.

- [ ] **Step 3: Regenerate types, implement `exampleNeeds` and `ExamplesPanel`**

Run the project's type generation from `openapi.json` rather than editing `types.ts`. `ExamplesPanel` mirrors `TemplatesPanel`: a `Panel` holding a `List` of `ListRow`, each with `primary` the name, `secondary` the description, `badge` the `exampleNeeds` string, the node kinds as chips, and an Import button driving a `useMutation`. No Device prop and no greying out, since import creates a workspace. On success push a toast naming the new workspace and invalidate `STATE_KEY`. Add `{ id: "examples", label: "Examples" }` to `TABS` directly after `templates`.

- [ ] **Step 4: Run tests and lint**

Run: `cd web && pnpm vitest run src/components/examples.test.ts && pnpm biome check src`
Expected: PASS, no diagnostics.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/ExamplesPanel.tsx web/src/components/examples.ts web/src/components/examples.test.ts web/src/canvas/Library.tsx web/src/lib/api.ts web/src/lib/types.ts
git commit -m "Examples: a Library tab that imports one"
```

---

### Task 5: The six single-radio examples

**Files:**
- Create: `crates/server/data/examples/satellite-pass.json`
- Create: `crates/server/data/examples/record-replay.json`
- Create: `crates/server/data/examples/scanner-sweep.json`
- Create: `crates/server/data/examples/hf-station.json`
- Create: `crates/server/data/examples/trunked-talkgroups.json`
- Create: `crates/server/data/examples/network-feed.json`
- Modify: `crates/server/src/examples.rs` (six rows in `EXAMPLES`)
- Test: the Task 2 tests cover all six once indexed

**Interfaces:**
- Consumes: the `Example` struct and tests from Task 2. Nothing new is produced.

Each file must contain the node kinds the spec's table assigns it, plus a `note` naming each node and why it is there:

| id | name | node kinds it must contain beyond device, scope, channel |
|---|---|---|
| `satellite-pass` | Satellite pass | `gps`, `satellite`, `rotator`, `recorder`, `map`, `note` |
| `record-replay` | Record and replay | `recording`, `time_machine`, `baseband_recorder`, `baseband_scope`, `note` |
| `scanner-sweep` | Scanner sweep | `scanner`, `hunt`, `spectrum_monitor`, `speaker`, `note` |
| `hf-station` | HF station | `propagation`, `event_filter`, `decoder_log`, `note` |
| `trunked-talkgroups` | Trunked talkgroups | `dmr_trunk`, `audio_fx`, `audio_recorder`, `speaker`, `note` |
| `network-feed` | Network feed | `network_export`, `export`, `event_output`, `note` |

- [ ] **Step 1: Add the six index rows with their files absent, and run the tests to see them fail**

Run: `cargo test -p sdrmm-server examples::`
Expected: FAIL, `ids_are_unique_and_match_their_filenames` names each missing path.

- [ ] **Step 2: Write the six JSON files**

Wire each graph so the ports match `crates/wire/src/patch/registry.rs`: the registry is the only authority on which port a node offers. `record-replay` uses a `recording` source with `recording` omitted, so it imports asking for a file. Keep each Note to a few short lines, one per node.

- [ ] **Step 3: Run the tests to verify every file parses, imports and carries a Note**

Run: `cargo test -p sdrmm-server examples::`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
cargo fmt && cargo clippy -p sdrmm-server --all-targets
git add crates/server/data/examples crates/server/src/examples.rs
git commit -m "Examples: six receivers for one radio"
```

---

### Task 6: The four array examples

**Files:**
- Create: `crates/server/data/examples/df-drive.json`
- Create: `crates/server/data/examples/passive-radar.json`
- Create: `crates/server/data/examples/beam-and-stitch.json`
- Create: `crates/server/data/examples/array-lab.json`
- Modify: `crates/server/src/examples.rs` (four rows in `EXAMPLES`)
- Test: the Task 2 tests, plus one lane assertion

**Interfaces:**
- Consumes: the `Example` struct and tests from Task 2. Nothing new is produced.

| id | name | node kinds it must contain beyond device, array |
|---|---|---|
| `df-drive` | DF drive | `gps`, `df`, `triangulation`, `signal_map`, `map`, `note` |
| `passive-radar` | Passive radar | `passive_radar`, `probe`, `gps`, `channel`, `map`, `note` |
| `beam-and-stitch` | Beam and stitch | `beamformer`, `stitch`, `df`, `scope`, `note` |
| `array-lab` | Array lab | `spatial_spectrum`, `correlator`, `polarimeter`, `note` |

`df-drive` carries the graph `crates/server/src/templates/array.rs` builds today, at five lanes and 433.920 MHz, plus a `signal_map` on the array's lanes and a Note. Read that file before writing the JSON; Task 7 deletes it.

- [ ] **Step 1: Write the failing lane test in `crates/server/src/examples.rs`**

```rust
#[test]
fn the_array_examples_ask_for_lanes() {
    let listed = all().expect("examples list");
    for id in ["df-drive", "passive-radar", "beam-and-stitch", "array-lab"] {
        let info = listed.iter().find(|e| e.id == id).expect(id);
        assert!(info.needs.lanes >= 2, "{id} asks for {} lanes", info.needs.lanes);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sdrmm-server examples::`
Expected: FAIL, the four ids are not in the list.

- [ ] **Step 3: Add the four index rows and write the four JSON files**

Every array processor takes its lanes through `ARRAY_PORT` from the `array` node, never from a device directly; check each port against the `processors!` list in `crates/wire/src/processor/mod.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p sdrmm-server examples::`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy -p sdrmm-server --all-targets
git add crates/server/data/examples crates/server/src/examples.rs
git commit -m "Examples: four workspaces for an array"
```

---

### Task 7: Templates go back to one radio

**Files:**
- Delete: `crates/server/src/templates/array.rs`
- Modify: `crates/server/src/templates.rs` (`mod array;` at line 3, `min_lanes: 1` at line 896, the `.chain(std::iter::once(array::df_drive()))` at line 899, the `min_lanes > 1` filter at line 1010)
- Modify: `crates/wire/src/rest.rs` (`min_lanes` and `is_one_lane` at lines 587 to 593, the `min_lanes > 1` branch at lines 610 to 614, the fixture at line 860)
- Modify: `crates/server/src/tests/templates.rs` (the filters at lines 13 and 129)
- Modify: `crates/engine/src/array/host/tests.rs` (the fixture at line 29)
- Modify: `web/src/components/templates.ts` (`templateSize`), `web/src/components/templates.test.ts`
- Modify: `openapi.json` (regenerated)

`min_lanes` on `ArrayProcessorDescriptor` in `crates/channels/` is a different field and stays.

- [ ] **Step 1: Change the tests first**

In `crates/server/src/tests/templates.rs` drop the two lane filters, so every listed template is asserted against single-radio behavior. In `web/src/components/templates.test.ts` drop the lanes case and keep the channel-count case. Add to `crates/server/src/templates.rs` tests:

```rust
#[test]
fn every_template_runs_on_one_radio() {
    for template in all() {
        assert!(template.patch.as_ref().is_some_and(|p| p.nodes.iter()
            .filter(|n| matches!(n.body, NodeBody::Device(_))).count() == 1), "{}", template.id);
        assert!(!template.patch.as_ref().is_some_and(|p| p.nodes.iter()
            .any(|n| matches!(n.body, NodeBody::Array(_)))), "{}", template.id);
    }
}
```

- [ ] **Step 2: Run tests to verify the new one fails**

Run: `cargo test -p sdrmm-server templates`
Expected: FAIL on `df-drive`, which has an Array and five devices.

- [ ] **Step 3: Delete the array template and the `min_lanes` plumbing**

Remove the file, the `mod`, the `chain`, the struct field, `is_one_lane`, the lane branch in `rest.rs`, and every fixture line that sets `min_lanes`. `templateSize` returns the channel count alone.

- [ ] **Step 4: Run the full check and regenerate the API document**

Run: `cargo fmt && cargo clippy --all-targets && cargo test -p sdrmm-server -p sdrmm-wire -p sdrmm-engine && cargo xtask openapi && cd web && pnpm biome check src && pnpm vitest run src/components/templates.test.ts`
Expected: PASS, and `min_lanes` is gone from `openapi.json`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Templates: one radio, some channels, some sinks"
```

---

### Task 8: A node kind cannot stay unexplained

**Files:**
- Modify: `crates/server/src/examples.rs` (add `UNCOVERED` and the test)

**Interfaces:**
- Consumes: `examples::all` from Task 2, `crate::templates::all` from Task 7, `NodeKind::ALL`.
- Produces: `const UNCOVERED: &[NodeKind] = &[];`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn every_node_kind_is_covered_or_listed() {
    let mut covered: std::collections::BTreeSet<NodeKind> = UNCOVERED.iter().copied().collect();
    for info in all().expect("examples list") {
        covered.extend(info.node_kinds);
    }
    for template in crate::templates::all() {
        if let Some(patch) = &template.patch {
            covered.extend(patch.nodes.iter().map(|n| n.body.kind()));
        }
    }
    let missing: Vec<_> = NodeKind::ALL.iter().filter(|k| !covered.contains(k)).collect();
    assert!(missing.is_empty(), "no example shows {missing:?}; add one or list it in UNCOVERED");
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p sdrmm-server every_node_kind_is_covered_or_listed`
Expected: FAIL while `UNCOVERED` is undefined, then PASS with it empty once the eleven examples are in. If it names a kind, the eleven files are missing something the spec's table promised; fix the file, do not grow `UNCOVERED`.

- [ ] **Step 3: Commit**

```bash
cargo fmt && cargo clippy -p sdrmm-server --all-targets
git add crates/server/src/examples.rs
git commit -m "Examples: a new node kind blocks until it is shown or excused"
```

---

### Task 9: Docs and changeset

**Files:**
- Modify: `docs/src/user-guide/workspaces.md` (the table at the top, and a section after Templates)

- [ ] **Step 1: Add Examples to the table and write the section**

One table row: `| Example | A whole working workspace, built in | Learn which nodes a job needs |`. A section after Templates saying where the tab is, that Import makes a new workspace rather than changing the open one, that Device and Recording nodes arrive empty for you to fill, and that each one carries a Note naming its nodes. Keep it to the page's register: short, no em dash.

- [ ] **Step 2: Build the book to check it renders**

Run: `cargo xtask docs`
Expected: PASS

- [ ] **Step 3: Write the changeset and commit**

```bash
cargo xtask changeset minor "Examples: eleven working workspaces you can import and change"
git add docs .changeset
git commit -m "Examples: document the tab"
```
