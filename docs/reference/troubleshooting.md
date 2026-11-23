---
title: Troubleshooting
description: Quick fixes for radios, silent audio, blank windows, drops, tokens, and recordings.
---

First step: click **Check hardware** on an empty Device node, or run `sdrmm --doctor`.

## Page does not open or token fails

- Check `SDRmm ready` log port; desktop app is loopback-only (`sdrmm --bind 0.0.0.0:8080` for LAN).
- Non-loopback binds serve `https://` (accept self-signed warning once). Behind a reverse proxy, mount at `/` (no path prefix).
- `Host not allowed`: open servers restrict `Host` names; connect by IP or set a [token](../server/configuration.md#token). API callers send `Authorization: Bearer <token>`.

## AppImage prints squashfuse or `(null) compression` errors

AppImageLauncher 2.x intercepts the AppImage and cannot read its zstd image. Update AppImageLauncher to 3.x, uninstall it, or run:

```sh
./sdrmm-app_*.AppImage --appimage-extract-and-run
```

## Blank window or missing waterfall

Disable hardware WebGL2 if a graphics driver breaks rendering:

```sh
sdrmm-app --disable-gpu                    # Linux
open -a SDRmm --args --disable-gpu         # macOS
```

Fallback: run the [portable server](../start/install.md#portable-server) on `127.0.0.1:8080` in a regular browser.

## A radio is missing

1. Verify OS detection (`lsusb` / System Information / Device Manager) and close other SDR apps.
2. Run `sdrmm --doctor` for missing libraries or Linux udev/`plugdev` permissions (Docker: [`group_add`](../server/deployment.md#usb-devices)).
3. **SoapySDR:** run `SoapySDRUtil --find` and match core 0.8/0.9.
4. **SDRplay:** install API 3.15+ and run `sdrplay_apiService`. **Aaronia:** close RTSA-Suite PRO; run `setup_udev.sh` on Linux. **Tektronix:** install the RSA API; close SignalVu-PC.
5. **Stays disconnected:** press **Open radio**, **Start** (if stopped), or **Forget radio** to bind a different unit.

## Spectrum works, but audio is silent

- Wire channel `audio` → **Speaker**, press Play, and click **Resume audio** if shown.
- Drag squelch far left, confirm the channel sits inside the Device span, and check Speaker **Out** / OS mute.
- Audio stuttering on plain `http://` LAN fixes itself over `https://` or `localhost`.

## Decoder shows nothing, drops, or missing recordings

- **No decodes:** verify signal in Scope, wiring (`Decoder log` / `Readout` / `Map`), gain/clipping, and [Catalog](../operate/decoders.md#catalog) verification status.
- **Drops:** lower sample rate, close unused views, use `--release`, wire Ethernet for network radios, put 20 MS/s HackRF on its own USB bus. On Linux, add `@audio - nice -10` to `/etc/security/limits.conf` and join `audio`.
- **Missing recordings:** check `--recordings-dir` write permissions (Docker `/data/recordings`), press **Stop** to finalize metadata, and verify `.sigmf-meta` + `.sigmf-data` pairs.
