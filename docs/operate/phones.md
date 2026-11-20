---
title: Phones
description: Optional iPhone and Android companion remote head for field missions, GPS/heading, CarPlay, and Android Auto.
---

Build from source ([iPhone iOS 27](../develop/phone-apps.md#iphone) / [Android 10+](../develop/phone-apps.md#android)); **Demo → Try without a server** previews offline.

## Pair

Phones pin the server's self-signed TLS key directly (use LAN or a direct Tailscale IP; reverse-proxy tunnels do not carry phones).

1. Enable **Library → Phones → Allow phones** (port `8443` default, mDNS `_sdrmm._tcp`, allow in firewall).
2. Click **Pair phone** (or run `sdrmm phone` headless), scan the QR or enter `host:port` + 8-digit code, confirm the **Key**, and **Trust**.

Each phone gets a revocable token; 5 wrong codes cancel the offer.

## Missions, GPS, and CarPlay / Android Auto

| Mission | Needs | Controls / view |
|---|---|---|
| Hunt | Signal hunt + channel | Level, Sweep, Mark, Clicks, Haptics |
| DF drive | Direction finder / Triangulation | Rose, map (rays/heat/ellipse), **Navigate**, Calibrate, Clear |
| Radar | Passive radar | Range-Doppler and tracks |
| Survey | Signal survey + GPS | Level-coloured trail |

Wire **GPS position → Phone** to feed fused compass/gyro/GPS heading (configure **Mount** / **Align with car** in phone Settings; grant **Allow always** location on iOS). **CarPlay** and **Android Auto** mirror DF drive and missions (for sideloaded Android Auto, tap **Version** 10× → Developer settings → **Unknown sources**).
