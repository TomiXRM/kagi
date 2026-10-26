#!/bin/sh
# Python >=3.11 (stdlib only), Java 21, curl; TLC v1.7.4 is downloaded to cache
# and SHA-256 verified by run.py before Java can load it. No jar is vendored.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec uv run --frozen --project "$ROOT" python "$ROOT/run.py" "$@"
