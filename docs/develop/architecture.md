---
title: Architecture
description: Crate boundaries, single-source wire types, control vs DSP planes, array runtime, and channel placement.
---

Desktop (`apps/desktop`, CEF) and headless server (`apps/sdrmm`) run the exact same Rust engine and React UI:

```text
React client <-> REST / WS / WebTransport / MCP <-> Control plane (HTTP, SQLite, graph)
                                                          | lock-free commands
Radio / network / SigMF -> DSP plane (no locks/alloc/async) -> Opus, JPEG, FFT, JSON events
```

## Crate boundaries

Enforced by `cargo xtask check`:

| Layer | Crates | Allowed internal deps |
|---|---|---|
| Primitives | `dsp`, `crypto` | None (zero I/O) |
| Algorithms | `modem`, `codec`, `aviation`, `orbit` | `dsp` only |
| Wire & API | `wire`, `decoder`, `media` | `decoder` → `dsp`, `modem`, `codec`, `wire`; `media` → `decoder`, `modem`, `codec` |
| Built-ins | `channels`, `array` | `channels` → `dsp`, `modem`, `codec`, `crypto`, `aviation`, `decoder`, `media`, `wire`; `array` → `dsp`, `modem`, `decoder`, `wire` |
| Plugins | `plugin-sdk`, `plugins`, `plugin-example` | WASM host & guest SDK |
| Devices | `device`, `device-*`, `usb-stream`, `serial`, `iqlink*`, `dvsi` | Never depend on `channels`, `engine`, or `server` |
| Runtime | `recorder`, `tools`, `cps`, `engine`, `server` | Full orchestration |
| Companion | `mobile-core` | `wire` only (no engine, server, DSP, USB, FFmpeg, SQLite) |

Nothing outside `sdrmm-channels` names a decoder; nothing outside `sdrmm-array` names a processor. Engine, server, and UI read descriptors, forms, presenters, and capabilities from the registries.

## Core invariants

- **One source of truth:** `crates/wire` owns all DTOs, settings, events, and patch types; OpenAPI, TypeScript types, `wires.md`, and `modes.json` generate from it.
- **Control vs DSP:** DSP takes settings over lock-free command queues and publishes via bounded snapshots and preallocated SPSC pools. Every queue is bounded; drops, lag, and recording faults surface explicitly.
- **Arrays:** capture blocks carry sample index and timestamp. Arrays wake dormant lane taps, align/calibrate via one filter per lane, and expose processor lane outputs (`beam`, `wide`) as virtual lanes on the first radio. Arrays never open hardware directly.
- **Auto placement:** branch-and-bound solver (50 ms / 100k nodes cap) maximizes covered channels across Devices (`placement.heard` vs `upper_bound`). Exhaustive check: `cargo test -p sdrmm-engine --lib compares_realistic_sizes -- --ignored --nocapture`.
- **ETSI tables:** DAB, DVB-S/S2/S2X, DVB-T/T2, GSE, and DRM tables live in `crates/modem` and `crates/channels` and are verified against official ETSI PDFs by the CI `etsi` job (`crates/modem-test-support/scripts/`).
