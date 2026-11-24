#!/usr/bin/env bash
set -euo pipefail

image="$(find "$1" -maxdepth 1 -name '*.AppImage' -print -quit)"
test -n "$image" || { echo "no AppImage in $1" >&2; exit 1; }
image="$(readlink -f "$image")"

check="$(readlink -f "$(mktemp -d)")"
trap 'rm -rf "$check"' EXIT
(cd "$check" && "$image" --appimage-extract > /dev/null)
root="$check/squashfs-root"
runtime="$root/usr/lib"

need() { test -e "$runtime/$1" || { echo "missing from the AppImage: $1" >&2; exit 1; }; }
need libcef.so
need icudtl.dat
need v8_context_snapshot.bin
need resources.pak
need locales/en-US.pak
test -x "$runtime/sdrmm-desktop-helper" || { echo "the AppImage has no Chromium helper" >&2; exit 1; }
test ! -e "$runtime/sdrmm-app/libcef.so" || { echo "libcef.so is in the AppImage twice" >&2; exit 1; }

loads() { ldd "$1" | grep -qF "libcef.so => $root/" || { echo "$1 cannot find libcef.so" >&2; ldd "$1" >&2; exit 1; }; }
loads "$root/usr/bin/sdrmm-app"
loads "$runtime/sdrmm-desktop-helper"
