---
title: Network export
description: Stream raw IQ or rtl_tcp, serve ADS-B Beast feeds, and forward events to webhooks, Matrix, MQTT, databases, files, or TUN.
---

> [!WARNING]
> Network IQ and Feed listener ports skip the server token and TLS. Bind `127.0.0.1` or use only on trusted networks.

```patch
[source] Device : Radio
[output] Network IQ : UDP, TCP, or rtl_tcp
[channel] ADS-B : 1090 MHz
[output] Event output : Beast, MQTT, DB, Webhook

Device --iq--> Network IQ
Device --iq--> ADS-B
ADS-B --events--> Event output
```

## Raw IQ and `rtl_tcp`

Wire `Device iq` or channel `baseband` → **Network IQ** (sample rate locks; interleaved `I, Q` with no header):

| Mode / format | Details |
|---|---|
| `cf32_le` / `ci16_le` / `cu8` | 8 / 4 / 2 bytes per I/Q pair (GNU Radio Complex / Short / RTL-SDR byte) |
| **UDP datagrams** | Max 1,400 B payload, header `None` in GNU Radio UDP Source (no sequence numbers) |
| **TCP stream** | Connects out to your listener; slow readers stop with an error |
| **`rtl_tcp` server** | Default `127.0.0.1:1234`, max 8 clients (cannot retune remotely). Example: `rtl_433 -d rtl_tcp:127.0.0.1:1234 -s 1024000 -f 433920000 -F json` |

## Feeds

Wire ADS-B `events` → **Event output → Feed (Beast)** (default `127.0.0.1:30005`, max 16 clients, one ADS-B channel per output). Sends Mode-S Beast binary frames with 12 MHz sample timestamps.

## Webhooks, Matrix, and MQTT

Wire `events` → **Event output** (messages cut at 1,900 chars; rate limits retry 4×):

- **Webhook JSON / Discord:** HTTP POST JSON, or Discord message + call audio.
- **Matrix:** room message + uploaded call audio.
- **MQTT:** QoS 1 JSON to `mqtt://` or `mqtts://`.
- **Notification:** OS alert (desktop app only).

## Databases and files

- **PostgreSQL:** auto-creates table (`sdrmm_events`) with `jsonb` record; non-local hosts require TLS unless `?sslmode=disable`.
- **InfluxDB v2/v3:** one point per event (measurement = event kind).
- **CSV / Text file:** appends under `Recordings/events/` (CSV prefixes formula-like strings with `'`).

### Credentials

Secrets live only in `sdrmm.db` (never exported with workspaces); re-enter them after importing onto another server.

## IP data

For DAB IP and DVB-S2 GSE packets, wire `events` → **Event output → Network interface** to create a TUN device: **Linux** needs `CAP_NET_ADMIN`, **macOS** needs `utun` permission (`utun8`), **Windows** needs Admin + `wintun.dll` beside the binary.
