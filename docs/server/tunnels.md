---
title: HTTPS with a tunnel
description: Reach SDRmm over HTTPS without opening router ports using Tailscale Serve or Cloudflare Tunnel.
---

> [!NOTE]
> Reverse-proxy tunnels terminate TLS and cannot carry [paired phones](../operate/phones.md); connect phones over LAN or directly to the server's Tailscale IP (`:8443`).

## Prepare SDRmm

Bind loopback on plain HTTP with a token set (in Docker, publish `127.0.0.1:8080:8080` and add `SDRMM_NO_TLS=1` + `SDRMM_TOKEN` to `.env`; run the tunnel on the host):

```sh
export SDRMM_TOKEN='replace-with-a-long-random-secret'
sdrmm --bind 127.0.0.1:8080
```

## Tailscale

Enable **MagicDNS** and **HTTPS Certificates** in the Tailscale admin console (names enter CT logs), then on the server:

```sh
tailscale serve --bg --https=443 http://127.0.0.1:8080
```

Open the full `https://<machine>.<tailnet>.ts.net` URL and enter your SDRmm token. Turn off with `tailscale serve --bg --https=443 off`.

## Cloudflare Tunnel

1. **Zero Trust → Access → Applications:** create a **Self-hosted** app for `radio.example.com` (no path) with an **Allow** policy first (a tunnel without Access is public).
2. **Networking → Tunnels:** install `cloudflared` on the SDRmm host and route `radio.example.com` (no path) to `http://127.0.0.1:8080`.
3. Ensure Cloudflare **WebSockets** is enabled. Open `https://radio.example.com`, pass Access login, then enter your SDRmm token.

Verify `/api/ws` connects in DevTools and that an incognito window prompts for Access login first.
