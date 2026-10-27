#!/usr/bin/env bash
set -euo pipefail

bundle="$1"
version="$2"
checked=0

mismatch() {
  echo "$1 reports version $2, the stamp is $version" >&2
  exit 1
}

check_app() {
  local reported
  reported="$(plutil -extract CFBundleShortVersionString raw -o - "$1/Contents/Info.plist")"
  test "$reported" = "$version" || mismatch "$1" "$reported"
}

check_deb() {
  local reported
  reported="$(dpkg-deb --field "$1" Version)"
  test "${reported%%-*}" = "$version" || mismatch "$1" "$reported"
}

check_name() {
  local name
  name="$(basename "$1")"
  case "$name" in
    *"$version"*) ;;
    *) mismatch "$1" "$name" ;;
  esac
}

while IFS= read -r path; do
  case "$path" in
    *.app) check_app "$path" ;;
    *.deb) check_deb "$path" ;;
    *) check_name "$path" ;;
  esac
  checked=$((checked + 1))
done < <(find "$bundle" -maxdepth 2 \
  \( -name '*.app' -o -name '*.deb' -o -name '*.rpm' -o -name '*.AppImage' \
     -o -name '*.dmg' -o -name '*.msi' -o -name '*-setup.exe' \) | sort)

test "$checked" -gt 0 || { echo "no bundles under $bundle, nothing reports $version" >&2; exit 1; }
echo "$checked bundles report $version"
