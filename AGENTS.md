## Non-negotiables
1. One source of truth for wire types.
2. Tests are part of the work.
3. Never explain code with a `//` comment; code explains itself. A `///` doc comment on an
   item is the one place for rationale a name cannot carry.
4. Fix problems directly if you find them, without asking
5. Prefer self-written pure Rust. Use compatible open-source code or translate C/C++ only when necessary.

## Coding structure
- Respect crate boundaries: `dsp` has no I/O and no internal deps; `modem` builds reusable
  modulation algorithms on `dsp`; `codec` (audio codecs, vocoders) builds on `dsp`; `crypto` has
  no internal deps; `aviation` (ACARS, ARINC 622, CPDLC) builds on `dsp`; `decoder` is the one
  decoder API, on `dsp`, `modem`, `codec` and `wire`; `media` (native broadcast playout) builds on
  `decoder`; `channels` pulls no internal crates beyond `dsp`, `modem`, `codec`, `crypto`,
  `aviation`, `decoder`, `media` and `wire`; `array` (array processors and their registry)
  builds on `dsp`, `modem`, `decoder` and `wire`, and nothing outside it names a processor.
- A decoder is one module in `channels` (or one WASM plugin): its settings struct, event
  payloads, presenters and registration live there. Nothing else names it: not `engine`, not
  `server`, not `wire`, not the frontend. Shared behavior is a capability or facet the decoder
  declares, never a match on its type id. If adding a decoder needs more, fix the abstraction.
- The frontend is generated from the backend: settings forms, readouts and event views come from
  the decoder registry. No per-decoder React code.
- Code two decoders share moves to `dsp` (primitives) or `modem` (sync, FEC, framing), so plugin
  authors get it too.
- One job, one node. Never two nodes, or a node and a device kind, that do the same thing: a radio
  is picked by a Device node and by nothing else. An abstraction over devices is a node wired to
  Device nodes, never a second kind of device that opens radios of its own. 
- No hidden flows, a flow should always be a wire. No hidden checkmark. It should always be visble that there is a connection trough a wire. 
- Hot DSP path: no locks, no allocation, no async. Settings via command queue, state via
   snapshot channels. Keep the control plane and DSP plane separated.
- Errors: no `unwrap()`/`expect()` outside tests and startup. Use `Result` and the project's
  error types. No silent failure: a dropped decoder frame or truncated result must surface.
- Keep functions small and single-purpose. Prefer clear names over comments.
- Always use the newest stable versions of every tool and dependency, and their current
  recommended patterns. Check the latest docs before writing.
- User-visible changes add a changeset: `cargo xtask changeset <patch|minor|major> "..."`.
- Always Format, Lint, Check & Test at the End of Every Change. But only test what you changed, full test suite will run in the CI.
- Max 3000 lines per file, 200 lines per function. Split large files into modules, large functions into helpers.
- Never hide a readout or meter because it has no value yet. Show it empty or at zero, so a
  face keeps its shape.
- A missing wire is never an error and never said in words: the empty port shows it. Red is
  only for real faults.
- One control language for settings: on/off and choices are dropdowns, numbers are fields,
  levels are sliders, each with its name small above it. No switches or checkboxes.
- Never long text of ours on a node. A face carries controls and readouts; what a setting means
  belongs in a title attribute or the docs, not in a paragraph on the canvas. Text the
  operator writes about their own patch is theirs, and the Note node is where it lives.
- Always write text, commits, docs as short as possible. Always try to reduce words, keep useless text out of the ui. And always write short and coherend text. That on which you are currently working on is not the most important thing in the world, you don't need to write everywhere about it. And never include an em dash.
- If you need any physical hardware, ask the user for that to plug in.
- Phones are additonal apps, not the entire app. so don't think about phones, if not explicitly asked 
