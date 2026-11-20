---
title: SDRmm Documentation
description: Wire a radio, a channel, and a display in Patch view, then pin daily controls to Rack view.
---

One Rust server talks to the radio and runs all DSP. The desktop app and any browser share the same live workspace:

- **On your desk:** the [desktop app](start/install.md#desktop-app) starts a private loopback server automatically.
- **By the antenna:** run the [portable server](start/install.md#portable-server) or [Docker container](server/deployment.md#docker-compose). Raw IQ stays on the server; only audio, events, and display frames cross the network.
