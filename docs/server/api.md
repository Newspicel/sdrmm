---
title: API and MCP
description: Drive the live engine via REST, WebSocket, WebTransport, and MCP from one OpenAPI schema.
---

| Endpoint | Serves |
|---|---|
| `/api/docs`, `/api/openapi.json` | Swagger UI and live OpenAPI schema (committed copy: [`openapi.json`](https://github.com/Newspicel/sdrmm/blob/main/openapi.json)) |
| `/api/ws` | WebSocket streams and JSON events |
| `/api/wt` | [WebTransport](#websocket-and-webtransport) over QUIC |
| `/mcp` | MCP over streamable HTTP (edits live canvas, reads logs/spectrum, runs Tools) |

Send `Authorization: Bearer $SDRMM_TOKEN` when a [token](configuration.md#token) is set.

## REST

| Area | Key routes |
|---|---|
| State & catalog | `/api/state`, `/api/devices`, `/api/saved-radios`, `/api/channeltypes`, `/api/patch/catalog`, `/api/clients` |
| Nodes & wiring | `/api/workspaces/active/nodes/{node}` (`channel`, `device`, `actions`), `/api/devices/network`, `/api/devices/serial`, `/api/patch/wiring` |
| Workspaces & setups | `/api/workspaces` (activate, apply, undo, redo, export, import, `resolved`), `/api/templates`, `/api/examples`, `/api/presets`, `/api/bookmarks` |
| Data & arrays | `/api/decoderlog`, `/api/recordings`, `/api/audiorecordings`, `/api/calls`, `/api/images`, `/api/arrays`, `/api/processors`, `/api/fusion`, `/api/survey`, `/api/missions` |
| Tools, ref & server | `/api/tools`, `/api/nanovna`, `/api/cps`, `/api/bandplan`, `/api/satellites`, `/api/ionosonde`, `/api/secrets` (write-only), `/api/auth`, `/api/phones`, `/api/status`, `/api/doctor`, `/api/transport` |

Errors return `{ error, detail?, code? }`. Workspace `PUT` merges older revisions, channel `PATCH` with `base` applies diffs, and `x-sdrmm-author` scopes undo/redo.

## WebSocket and WebTransport

- **WebSocket (`/api/ws`):** subscribes to decoded records, node status, invalidations (refetch durable state via REST), `Present`/`Point` collaboration, `PipelineHealth`, and binary spectrum/audio/video/IQ/symbol/surface frames (`ClientCommand` / `ServerEvent` in OpenAPI; `web/src` is the reference client).
- **WebTransport (`/api/wt`):** query `GET /api/transport` before every connect for UDP port and rotating `serverCertificateHashes`. Streams use little-endian `u32` length framing: client bidirectional stream for JSON commands/events, unreliable datagrams for audio, per-stream-ID server unidirectional streams for binary frames.
