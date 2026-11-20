---
title: Install
description: Desktop app for a local radio, or server for a machine by the antenna. Both run the same engine.
---

## Desktop app

Download an installer from the [download page](/download):

| Platform | Packages |
|---|---|
| macOS | `.dmg` (Apple silicon, Intel), or `brew install sdrmm-app` |
| Linux | `.deb`, `.rpm`, `.AppImage` (x86-64, ARM64), APT, DNF, Nix |
| Windows | `.msi` / `.exe` (x86-64), `.exe` (ARM64) |

The desktop app binds a random loopback port reachable only on this machine.

## Portable server

Unpack the `sdrmm` archive from the [download page](/download) (or `brew install sdrmm`) and run `./sdrmm` (`sdrmm.exe` on Windows), then open `http://localhost:8080`.

Pass `--bind 0.0.0.0:8080` to reach it from other machines; `sdrmm` then creates a [token](../server/configuration.md#token) and serves [HTTPS](../server/configuration.md#https-and-webtransport). For Docker USB passthrough and certificates, see [Deployment](../server/deployment.md#docker-compose).

## Nix

The flake exports `sdrmm-desktop`, `sdrmm`, and `default` (x86-64 and ARM64 Linux). From a checkout, run `pnpm --dir web build`, then `nix build path:.`.

<details>
<summary>NixOS module override (SoapySDR plugins and SDRplay)</summary>

Pass extra SoapySDR modules via `soapyPlugins`; for SDRplay, enable `services.sdrplayApi` and pass unfree `pkgs.sdrplay` as `sdrplayApi`:

```nix
environment.systemPackages = [
  (inputs.sdrmm.packages.${pkgs.stdenv.hostPlatform.system}.sdrmm.override {
    soapyPlugins = with pkgs; [ soapybladerf soapyremote ];
  })
];

hardware.rtl-sdr.enable = true;
users.users.your-user.extraGroups = [ "plugdev" ];
```

</details>

## Stable or nightly

Use stable releases for daily operation (desktop self-updates on macOS, Windows, and Linux AppImage; APT/DNF/Nix update through their package manager). [Nightly builds](https://github.com/Newspicel/sdrmm/releases/tag/nightly) follow `main` and never self-update.

> [!WARNING]
> Databases opened by a newer build cannot be opened in an older one. Back up `sdrmm.db` before trying nightly builds.
