#!/bin/sh
# Dependencies/runtime are declared in ../pyproject.toml and locked by ../uv.lock.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
exec "$ROOT/run.sh" --suite pure "$@"
