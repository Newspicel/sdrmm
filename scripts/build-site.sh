#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
out=${1:-$root/dist/site}

pnpm --dir "$root/web" install --frozen-lockfile
pnpm --dir "$root/site" install --frozen-lockfile
pnpm --dir "$root/site" build

rm -rf "$out"
mkdir -p "$out"
cp -R "$root/site/dist/." "$out/"

echo "site assembled in $out"
