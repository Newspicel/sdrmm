# Example workspaces

## Problem

Templates do not tell you which nodes a job needs. There are 36 of them and they
instantiate 12 of the 39 node kinds. The other 27 appear in no template at all:

```
beamformer  passive_radar  stitch  spatial_spectrum  correlator  polarimeter
recording  signal_gen  baseband_scope  signal_map  propagation
spectrum_monitor  dmr_trunk  event_filter  audio_fx  event_output  recorder
audio_recorder  baseband_recorder  time_machine  network_export  export
scanner  hunt  satellite  rotator  note
```

Every template is the same shape: one Device, some Channels, some sinks. That
shape cannot express a rotator following a pass, a recorder feeding playback, or
an array feeding a beamformer. Adding more templates adds more rows of the same
shape and leaves all 28 unexplained.

## Decision

Ship example workspaces: complete `WorkspaceExport` files, bundled in the
binary, imported as a new workspace, each carrying a Note that names its own
nodes.

Templates keep the single-decoder job: retune the radio you already have.
Examples take the composite job: whole graphs. No overlap to explain.

Authored by PR, not hosted. `app.sdrmm.com` and the relay were removed in
b4dd06d7 and this does not walk that back.

## Non-goals

No upload, no accounts, no moderation, no runtime fetch. An example arrives with
an app update.

## Storage

`crates/server/data/examples/<id>.json`, each a plain `WorkspaceExport`,
byte-identical to what the export button writes.

`build.rs:compress_tree` already walks `data/` recursively and gzips every file,
under `cargo:rerun-if-changed=data`. A new example needs no build change and
ships compressed through the existing `packed_data!` macro, the same path
`notices.json` takes.

`crates/server/src/examples.rs` holds the index:

```rust
struct Example {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    body: &'static [u8],
}
```

Authoring one: build it in the app, export, drop the file in, add one table row,
PR. No Rust graph building.

## Wire types

In `crates/wire/src/rest.rs`, beside `TemplateInfo`:

```rust
pub struct ExampleInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub needs: ExampleNeeds,
    pub node_kinds: Vec<NodeKind>,
}

pub struct ExampleNeeds {
    pub radios: u32,
    pub lanes: u32,
    pub recordings: u32,
    pub min_freq_hz: f64,
    pub max_freq_hz: f64,
}
```

`needs` and `node_kinds` are derived from the parsed graph on every list, never
authored, so they cannot drift from the file.

`node_kinds` is what puts "this one shows you the Rotator" on the row. It makes
the list answer the question backwards: pick a node, find the example that uses
it.

## Derivation

One function, `PatchGraph` in and `ExampleNeeds` out:

- `radios`: count of `Device` nodes
- `recordings`: count of `Recording` nodes
- `lanes`: largest number of lane edges into any one `Array` node, since
  `ArrayNode` carries no lane count of its own
- frequency span: channel offsets against their device centers

## REST

Two routes beside the template pair in `crates/server/src/rest.rs`:

- `GET /api/examples` returns `ExamplesResponse { examples }`
- `POST /api/examples/{id}/import` returns `CreatedRowId`

Import inflates the embedded bytes to a `serde_json::Value`, then runs the same
two calls a dropped file runs: `store::upgrade_import` for version migration and
notices, then `store::import_workspace`. That brings `settle_arrays`,
`validate_against(descriptor_table())`, `forget_absent_nodes` and
`free_workspace_name` for free, so an example imported twice becomes
"Satellite pass 2" rather than colliding.

Unknown id is 404.

## UI

`web/src/components/ExamplesPanel.tsx` beside `TemplatesPanel.tsx`, and one row
in `TABS` in `web/src/canvas/Library.tsx`.

A row shows name, description, a needs badge, node-kind chips and an Import
button. No Device selection, since import creates a workspace rather than
editing the open one. Nothing is greyed out: an example you lack the hardware
for still imports, and the Device nodes stand empty and say so.

## The Note convention

Every example ships with a Note pinned to the rack, naming each node and why it
is there. A test asserts every example contains at least one `Note` node.

This is the part that answers the original question, and it survives the file
being modified and re-shared, which a list description does not.

## The twelve examples

| id | needs | orphan kinds covered |
|---|---|---|
| `no-radio` | nothing | `signal_gen`, `baseband_scope` |
| `satellite-pass` | 1 radio | `satellite`, `rotator`, `recorder` |
| `record-replay` | 1 recording | `recording`, `time_machine`, `baseband_recorder` |
| `scanner-sweep` | 1 radio | `scanner`, `hunt`, `spectrum_monitor` |
| `hf-station` | 1 radio | `propagation`, `event_filter` |
| `trunked-talkgroups` | 1 radio | `dmr_trunk`, `audio_fx`, `audio_recorder` |
| `network-feed` | 1 radio | `network_export`, `export`, `event_output` |
| `df-drive` | 2 lanes | `signal_map` |
| `passive-radar` | 2 lanes | `passive_radar` |
| `beam-steer` | 2 lanes | `beamformer` |
| `stitch-wide` | 2 lanes | `stitch` |
| `array-lab` | 2 lanes | `spatial_spectrum`, `correlator`, `polarimeter` |

`note` is covered by all twelve. Together they cover all 27.

Beam steering and Stitch wide are separate files because their nodes demand opposite
Array tuning: the beamformer and direction finder need `together`, stitch needs `spread`,
and each refuses the other. One file would ship half of itself refused.

`no-radio` runs on a Signal generator, so the canvas can be learned before
buying a dongle.

The last four need a coherent array to build and verify, so the plan stages them
last.

## The array template moves

`crates/server/src/templates/array.rs` builds a composite today: array, df, gps,
triangulation and map, as a template with `min_lanes`. That is the job this spec
gives to examples.

Keeping both would be two things doing one job. So `df-drive` becomes an example
and `templates/array.rs` is deleted, along with the `min_lanes` plumbing that
exists only for it: `TemplateInfo::min_lanes`, `is_one_lane`, and
`templateSize`'s lane branch in `web/src/components/templates.ts`.

Templates then hold only what they claim to hold: one Device, some Channels,
some sinks.

This deletes working code and is the one part of the spec to check before the
plan is written.

## Tests

- every indexed id resolves to a file that parses as a current `WorkspaceExport`
- every example imports through the real `upgrade_import` plus
  `import_workspace` path
- every example contains a `Note` node
- ids are unique and match their filenames
- derivation gives the needs the table above claims
- the union of every example's `node_kinds` plus the templates' covers
  `NodeKind::ALL`, except the kinds named in an `UNCOVERED` list in
  `examples.rs`, which starts empty. A node kind added later fails this test
  until it is either covered by an example or added to that list with a reason

That last test is the one that keeps this from rotting back into the state that
prompted it.

## Docs and changeset

A section in `docs/src/user-guide/workspaces.md`, in the table at the top and
beside Templates. `cargo xtask changeset minor` for the user-visible change.
