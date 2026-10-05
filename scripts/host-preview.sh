#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
host="$(rustc -vV | sed -n 's/^host: //p')"
exec cargo run --locked -p pulp-host --bin host-preview --target "$host" \
  --config 'unstable.build-std=["std","test"]' -- "$@"
