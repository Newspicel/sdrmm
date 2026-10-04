#!/usr/bin/env sh
set -eu

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <owner/repo>" >&2
  echo "  uploads stable release notes to https://downloads.sdrmm.com/releases/changelog.json" >&2
  exit 2
fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT

gh api --paginate --slurp "repos/$1/releases?per_page=100" |
  jq '[.[][] | select((.draft or .prerelease) | not)
      | {tag: .tag_name, published_at, body}]
      | sort_by(.published_at) | reverse' > "$dir/changelog.json"

CACHE_CONTROL="public, max-age=300" "$root/scripts/r2-upload.sh" releases "$dir/changelog.json"
