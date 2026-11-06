#!/usr/bin/env bash
set -euo pipefail

image="$(find "$1" -maxdepth 1 -name '*.AppImage' -print -quit)"
test -n "$image" || { echo "no AppImage in $1" >&2; exit 1; }
image="$(readlink -f "$image")"

check="$(mktemp -d)"
trap 'rm -rf "$check"' EXIT
(cd "$check" && "$image" --appimage-extract > /dev/null)
root="$check/squashfs-root"
runtime="$root/usr/lib/sdrmm-app"

need() { test -e "$runtime/$1" || { echo "missing from the AppImage: $1" >&2; exit 1; }; }
need libcef.so
need icudtl.dat
need v8_context_snapshot.bin
need resources.pak
need locales/en-US.pak
test -x "$runtime/sdrmm-desktop-helper" || { echo "the AppImage has no Chromium helper" >&2; exit 1; }
test ! -e "$root/usr/lib/libcef.so" || { echo "libcef.so is in the AppImage twice" >&2; exit 1; }
