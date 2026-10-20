#!/usr/bin/env bash
set -euo pipefail
shopt -s nullglob

STAPLE_ATTEMPTS=5

staple() {
  local dmg="$1"
  for attempt in $(seq "$STAPLE_ATTEMPTS"); do
    if xcrun stapler staple "$dmg"; then
      return
    fi
    test "$attempt" -lt "$STAPLE_ATTEMPTS" || { echo "ticket for $dmg unreachable after $STAPLE_ATTEMPTS attempts" >&2; exit 1; }
    sleep $((2 ** attempt))
  done
}

for dmg in bundles/*.dmg; do
  xcrun notarytool submit "$dmg" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" --wait
  staple "$dmg"
  xcrun stapler validate "$dmg"
  spctl --assess --type open --context context:primary-signature -v "$dmg"
done
