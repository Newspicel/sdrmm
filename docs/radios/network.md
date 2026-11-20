---
title: Network and virtual sources
description: Connect rtl_tcp, SpyServer, SDRconnect, KiwiSDR, SigMF recordings, and the signal generator.
---

## Connect a network radio

On an empty Device node, open **Network**, choose **Via**, and enter `host:port` (bookmark icon saves addresses):

| Protocol | Default port |
|---|---:|
| `rtl_tcp` | 1234 |
| SpyServer | 5555 |
| SDRconnect | 5454 |
| KiwiSDR | 8073 |
| AntSDR / Pluto | 30431 |
| ESP-SDR | 80 |

If the link cannot keep up, **Lost** climbs; lower sample rate or use wired Ethernet.

## SDRconnect

Reaches an SDRplay RSP via [SDRconnect](https://www.sdrplay.com/sdrconnect/) WebSocket API (`SDRconnect_headless --websocket_port=5454`) without local SDRplay API. Append `/secondary` for RSPduo's second tuner.

- **LNA** counts LNA states (lower number = more gain; no IF gain). Also exposes Device VFO, Filter, Receiver selector, Network mode, Device profile, and remote Recording.

> [!WARNING]
> SDRconnect uses unencrypted `ws://`. Use only on a trusted LAN or through a tunnel.

## KiwiSDR

Paste any `http://` or `https://` URL from [rx.kiwisdr.com](http://rx.kiwisdr.com/) (or `password@host:8073` for private/unlocked Kiwis). Streams 12 or 20 kHz across 0–30 MHz for SSB, CW, AM, and narrowband modes. Stops without reconnecting when kicked or time-limited; retries ordinary network drops.

## Other sources

- **Recording tab:** replays a [SigMF recording](../operate/recording.md#play-a-recording) with seek and recorded GPS output.
- **Virtual tab:** built-in signal generator (tone, FM, LoRa, BSS, DVB-T, ATV, PER, noise figure, VLF). Debug builds also list [synthetic array/TX radios](../develop/building.md#test-without-a-radio).
