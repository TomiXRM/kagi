#!/bin/sh
# The shared launcher downloads and verifies the pinned TLC release.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
exec "$ROOT/run.sh" --suite tla "$@"
