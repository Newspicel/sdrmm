#!/usr/bin/env bash
set -euo pipefail

arch="$1"
tools="${XDG_CACHE_HOME:-$HOME/.cache}/.cargo-packager/AppImage"
mkdir -p "$tools"

apprun_release=apprun-old
linuxdeploy_release=1-alpha-20251107-1
appimage_plugin_release=1-alpha-20250213-1

case "$arch" in
  x86_64)
    apprun_sha256=f30140a43a0a59e46db21bdefdf749b9e9f2c6946e92afabbacf98b8ae73fb4f
    linuxdeploy_sha256=c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d
    appimage_plugin_sha256=992d502a248e14ab185448ddf6f6e7d25558cb84d4623c354c3af350c25fccb3
    ;;
  aarch64)
    apprun_sha256=072f17c0895a85c490282fe5395c5007e5fc75da727e553b3b8fb680feb11578
    linuxdeploy_sha256=620095110d693282b8ebeb244a95b5e911cf8f65f76c88b4b47d16ae6346fcff
    appimage_plugin_sha256=83c292149274965a865dcd44c135cfca8ba28c6b7de3eb628d4b8b5f248af17c
    ;;
  *)
    echo "no pinned AppImage tools for $arch" >&2
    exit 1
    ;;
esac

sha256_of() {
  if command -v sha256sum > /dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

fetch() {
  local url="$1" name="$2" want="$3" part="$tools/$2.part" got
  if ! curl --fail --location --silent --show-error \
    --retry 5 --retry-delay 5 --retry-all-errors --connect-timeout 30 \
    --output "$part" "$url" || [ ! -s "$part" ]; then
    rm -f "$part"
    echo "$url is unreachable" >&2
    exit 1
  fi
  got="$(sha256_of "$part")"
  if [ "$got" != "$want" ]; then
    rm -f "$part"
    echo "$name is not the pinned build: expected $want, got $got" >&2
    exit 1
  fi
  chmod +x "$part"
  mv "$part" "$tools/$name"
}

releases=https://github.com

fetch "$releases/tauri-apps/binary-releases/releases/download/$apprun_release/AppRun-$arch" \
  "AppRun-$arch" "$apprun_sha256"
fetch "$releases/linuxdeploy/linuxdeploy/releases/download/$linuxdeploy_release/linuxdeploy-$arch.AppImage" \
  "linuxdeploy-$arch.AppImage" "$linuxdeploy_sha256"
fetch "$releases/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/$appimage_plugin_release/linuxdeploy-plugin-appimage-$arch.AppImage" \
  linuxdeploy-plugin-appimage.AppImage "$appimage_plugin_sha256"
ls -l "$tools"
