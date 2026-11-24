#!/usr/bin/env bash
set -uo pipefail
limit=$1
shift
for attempt in 1 2 3; do
  timeout "$limit" "$@" && exit 0
  echo "::warning::$* failed or passed ${limit}s on attempt $attempt"
done
exit 1
