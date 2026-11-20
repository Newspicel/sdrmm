---
title: Deployment
description: Run SDRmm headless with Docker Compose or as a system service, with USB and SDRplay passthrough.
---

## Docker Compose

```sh
git clone https://github.com/Newspicel/sdrmm.git
cd sdrmm
docker compose pull
docker compose up -d
```

Open `https://<host>:8080`. Read the auto-generated token with `docker compose logs sdrmm | grep token` (or `docker compose exec sdrmm cat /data/access-token`). Back up the `sdrmm-data` volume (`/data`). Images: `ghcr.io/newspicel/sdrmm` (`:latest`, `:<major>.<minor>`, `:nightly`).

- **Custom token / tunnel:** put `SDRMM_TOKEN=...` (and `SDRMM_NO_TLS=1` behind a [tunnel](tunnels.md)) in `.env`.
- **Custom TLS / WebTransport / Phones:** pass `--tls-name <host>` or mount PEM certs readable by UID `10001`; publish `8080:8080/udp` for WebTransport and `8443:8443` for phones (pair via **Manual** host IP).

### USB devices

Default `docker-compose.yml` passes `/dev/bus/usb` and hotplug cgroup rules (`c 189:* rmw`). Match `group_add` to your host USB GID (`stat -c '%g %G %a' /dev/bus/usb/*/*`; `46` is typically `plugdev`, or `"0"` if no udev rules are installed).

<details>
<summary>Extra SoapySDR modules in Docker</summary>

The image includes bladeRF, LimeSDR, and SoapyRemote. Derive an image for others:

```dockerfile
FROM ghcr.io/newspicel/sdrmm:latest
USER root
RUN apt-get update && apt-get install -y --no-install-recommends soapysdr-module-audio && rm -rf /var/lib/apt/lists/*
USER sdrmm
```

</details>

### SDRplay receivers

Run `sdrplay_apiService` on the host, then mount the library and enable `ipc: host` (exposes host shared memory; trusted images only):

```yaml
volumes:
  - sdrmm-data:/data
  - /usr/local/lib/libsdrplay_api.so.3:/usr/local/lib/libsdrplay_api.so.3:ro
ipc: host
```

## As a system service

Run under a non-root user with USB group access and absolute paths; stop with `SIGTERM` so recordings finalize:

```sh
/usr/local/bin/sdrmm --bind 0.0.0.0:8080 --db /var/lib/sdrmm/sdrmm.db --recordings-dir /var/lib/sdrmm/recordings
```

**Unattended checklist:** save `sdrmm --doctor`, stream 30 min checking **Drops**, test controls, unplug/replug recovery, and record/replay a short SigMF capture.
