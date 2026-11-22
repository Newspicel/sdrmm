---
title: Configuration and security
description: CLI options, environment variables, tokens, HTTPS, WebTransport, phones, and reverse proxies.
---

Default bind is `127.0.0.1:8080` (plain HTTP, no auth). Any non-loopback bind automatically enables a [token](#token) and [HTTPS](#https-and-webtransport).

## Options

| Flag | Env var | Default | Purpose |
|---|---|---|---|
| `--bind <ADDR>` | | `127.0.0.1:8080` | Listen address |
| `--db <PATH>` | | `<data>/sdrmm/sdrmm.db` | SQLite database (`<data>`: `~/.local/share`, `~/Library/Application Support`, `%APPDATA%`) |
| `--recordings-dir <PATH>` | | `<data>/sdrmm/recordings` | Recordings directory |
| `--plugins-dir <PATH>` | | `<data>/sdrmm/plugins` | [WASM plugins](../develop/plugins.md) |
| `--token <TOKEN>` | `SDRMM_TOKEN` | Auto on network bind | Shared bearer token |
| `--no-auth` | `SDRMM_NO_AUTH` | Off | Open network bind (trusted LAN only) |
| `--oidc-issuer` and others | `SDRMM_OIDC_*` | Off | [Sign-in and roles](sign-in.md) |
| `--tls-cert`, `--tls-key` | | None | PEM cert chain + key (paired) |
| `--tls-self-signed` | | Auto on network bind | Managed self-signed cert |
| `--tls-name <NAME>` | `SDRMM_TLS_NAMES` | Auto IPs + `<host>.local` | Custom SANs (repeatable / comma-separated) |
| `--no-tls` | `SDRMM_NO_TLS` | Off | Plain HTTP on network bind (for proxies/tunnels) |
| `--dev-cors` | | Off | Separate dev frontend origin |
| `--doctor`, `--doctor-rates` | | | Print diagnostics or probe sample rates and exit |

Subcommand: `sdrmm phone [--name <label>] [--db <path>] [--plain]` prints a pairing QR in the terminal.

## Token

Export `SDRMM_TOKEN` (avoids process list) or let a network bind auto-generate `access-token` beside `sdrmm.db`. API clients send `Authorization: Bearer <token>` (WS/downloads also accept `?token=...`). Open servers (`--no-auth`) reject unknown `Host` headers (only IPs, `localhost`, `--tls-name`, and `<host>.local` pass).

All routes require the token or a [signed-in session](sign-in.md) except the UI shell, `/api/auth` (and its login, callback and logout), `/api/status`, `/api/about`, `/api/openapi.json`, and `/api/docs`.

## HTTPS and WebTransport

- Non-loopback binds serve HTTPS and redirect `http://` → `https://`. Self-signed certs store in `tls/` (397-day validity, auto-renewed keeping the same keypair). Pass `--tls-name` behind NAT or in Docker.
- Direct HTTPS automatically enables **WebTransport on the same UDP port** (open UDP in firewall for lower-latency Chromium audio/video; falls back to WebSocket).

## Data, logs, and reverse proxies

- **Backup:** stop `sdrmm` (or SQLite backup) and back up `sdrmm.db` (holds sign-in sessions), `tls/`, `access-token`, and `--recordings-dir`.
- **Logs:** default `info,sdrmm=debug`; override via `RUST_LOG=info` or `RUST_LOG=sdrmm=trace,info`.
- **Reverse proxy:** bind loopback (or `--no-tls` + firewalled), set a token, serve at origin root `/` (no path prefix), preserve `Host`, and proxy WebSocket `/api/ws` and `/mcp`.
- **Phones:** LAN self-signed servers take phones on the main port; loopback or CA-cert servers use **Allow phones** (port `8443`, mDNS `_sdrmm._tcp`) so cert renewals never break pinned keys.
