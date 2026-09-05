#!/usr/bin/env bash
# #454: modal preview lists must render every row.
#
# The amend and discard cards used to print a total ("172") and then render
# only the first 10-20 rows with no "+N more" and no scroll, so the rest was
# unreachable. Both now use `uniform_list` over the full slice; a fixed
# `.take(N)` on those lists is the exact regression that hides rows again.
#
# Why a script and not an inline `grep`: a line-based grep misses the
# rustfmt-wrapped form
#     plan.preview_files
#         .iter()
#         .take(10)
# which is the same blind spot #396 found in the klog gate. This scans each
# file as one string with newlines collapsed, so a wrapped chain is caught.
#
# Row-level truncation is NOT matched and stays allowed on purpose
# (`p.chars().take(80)` shortens one line, not the list) — the pattern below
# only fires on `.iter()`/direct `.take(` applied to a preview list.
#
# `preview_commits` (shared plan card, "Commits to push") is covered as of
# the #454 layer-3 slice: the producer caps the preview at 100
# (`build_push_preview_for_oid`), so the card renders every row in a capped
# scroll box instead of the old `take(min(10))` + "… and N more".
set -uo pipefail

LISTS='preview_files|preview_commits|skipped'
fail=0

while IFS= read -r -d '' file; do
  # Collapse the file to one line so a wrapped method chain is still one
  # string, then look for <list> … .take( with only chain punctuation and
  # whitespace between them.
  if tr '\n' ' ' <"$file" |
    grep -qE "\.($LISTS)[[:space:]]*(\.[a-z_]+\(\)[[:space:]]*)*\.take\("; then
    echo "::error file=$file::a modal preview list is truncated with .take(N) — render every row (uniform_list + scroll), see #454"
    # Show the offending region to make the failure actionable.
    grep -nE "($LISTS)|\.take\(" "$file" | head -20
    fail=1
  fi
done < <(find src/ui -name '*.rs' -print0)

if [ "$fail" -ne 0 ]; then
  echo "::error::modal preview lists must be reachable in full (#454)."
  exit 1
fi
echo "OK: no truncated modal preview lists."
