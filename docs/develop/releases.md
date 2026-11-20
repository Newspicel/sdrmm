---
title: Releases
description: Changesets, semantic versioning, packaging, signed auto-updates, mirrors, and CI secrets.
---

Nightly rebuilds `:nightly` / `nightly` tag at 03:07 UTC when `main` changes.

## Cut a release

Add a changeset for every user-visible change, then tag from clean `main`:

```sh
cargo xtask changeset patch "RTL-SDR: keep gain after reconnect"
cargo xtask release --dry-run
cargo xtask release
git push origin v1.2.3
```

`release` derives the bump from `.changeset/` and tags `HEAD` without committing; CI compiles release notes (`cargo xtask release-notes 1.2.3`). Workspace version (`cargo xtask set-version`) respects Windows MSI limits (8-bit major/minor, 16-bit patch, no prerelease suffix).

## Desktop bundles and auto-updates

- **Portable archives:** `cargo xtask-release dist [--target <triple>]`, `source-dist`, and `link-check <dir>`.
- **Desktop installers:** `cargo xtask desktop --bundles app,dmg` (`deb,rpm,appimage` on Linux; `wix,nsis` on Windows, `nsis` only on ARM64; requires `cargo-packager`).
- **Auto-updates:** release builds check `downloads.sdrmm.com` then GitHub; signed with minisign (`CARGO_PACKAGER_SIGN_PRIVATE_KEY`, manifest via `cargo xtask updater-manifest`).
- **Distribution:** `ghcr.io/newspicel/sdrmm` (amd64 + arm64), Homebrew (`sdrmm` / `sdrmm-app`), AUR (`cargo xtask aur`), WinGet (`Newspicel.SDRmm`), R2 bucket `sdrmm` (`downloads.sdrmm.com`), signed APT/RPM (`linux-repo` workflow). Upload neural denoise models immutably via `scripts/r2-upload.sh denoise/v1 target/denoise-model/*.sdrmmnn`.

<details>
<summary>CI secrets &amp; pre-release checklist</summary>

- **Required secrets:** `CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ACCOUNT_ID`, `TAURI_SIGNING_PRIVATE_KEY` + `_PASSWORD`, `APPLE_*` (signing/notarization), `APP_STORE_CONNECT_*` (iOS).
- **Optional secrets (job skips with warning if absent):** `AUR_SSH_KEY`, `WINGET_TOKEN`, `PACKAGES_GPG_KEY`, `DISCORD_WEBHOOK`.
- **Checklist:** `cargo xtask check`, `test`, `smoke`, `audit`, `desktop`, Docker build, physical radio reconnect + SigMF check, tag & watch matrix, verify published artifact `--version` and `--doctor`.

</details>
