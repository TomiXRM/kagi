#!/usr/bin/env bash
# T-LOC-GATE-001: file-LOC ratchet (advisory, non-blocking CI check).
#
# Regenerate/update the baseline (run from repo root):
#   find src crates/*/src -name '*.rs' -not -path '*/tests/*' -not -path '*/vendor/*' \
#     -not -path '*/target/*' -exec awk 'END{print FILENAME, NR}' {} \; \
#     | awk '$2>800' | sort > ci/loc-baseline.txt
#
# Rule: any src/**/*.rs or crates/*/src/**/*.rs file over 800 LOC must be listed
# in ci/loc-baseline.txt with an LOC ceiling >= its current size. New files over
# 800 LOC, or existing ones that grew past their baseline, fail the check. Files
# that shrank below their baseline pass but print a notice to re-run the command
# above so the baseline keeps tracking reality.
set -u

BASELINE="$(dirname "$0")/loc-baseline.txt"
fail=0

while IFS= read -r -d '' file; do
  loc=$(wc -l <"$file" | tr -d ' ')
  [ "$loc" -le 800 ] && continue

  baseline_loc=$(awk -v f="$file" '$1==f{print $2}' "$BASELINE")

  if [ -z "$baseline_loc" ]; then
    echo "::error::$file has $loc LOC (>800) and is not in $BASELINE."
    fail=1
  elif [ "$loc" -gt "$baseline_loc" ]; then
    echo "::error::$file grew to $loc LOC, exceeding its baseline of $baseline_loc in $BASELINE."
    fail=1
  elif [ "$loc" -lt "$baseline_loc" ]; then
    echo "::notice::$file shrank to $loc LOC (baseline $baseline_loc) — consider updating the baseline:"
    echo "  find src crates/*/src -name '*.rs' -not -path '*/tests/*' -not -path '*/vendor/*' -not -path '*/target/*' -exec awk 'END{print FILENAME, NR}' {} \\; | awk '\$2>800' | sort > ci/loc-baseline.txt"
  fi
done < <(find src crates/*/src -name '*.rs' \
  -not -path '*/tests/*' -not -path '*/vendor/*' -not -path '*/target/*' -print0)

if [ "$fail" -ne 0 ]; then
  echo "::error::LOC ratchet failed. Split the file(s) above, or if the growth is deliberate, regenerate the baseline with:"
  echo "  find src crates/*/src -name '*.rs' -not -path '*/tests/*' -not -path '*/vendor/*' -not -path '*/target/*' -exec awk 'END{print FILENAME, NR}' {} \\; | awk '\$2>800' | sort > ci/loc-baseline.txt"
  exit 1
fi

echo "OK: no file exceeds its LOC baseline."
