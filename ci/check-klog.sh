#!/usr/bin/env bash
# ADR-0096 / #396: klog single-channel ratchet (wrapped-form detection).
#
# The blocking grep in ci.yml catches `eprintln!("[kagi]` on a single line,
# but rustfmt wraps long calls so `eprintln!(` and `"[kagi]` land on different
# lines and a line-based grep misses them. This check scans whole files for
# any eprintln!/println! whose first string literal starts with "[kagi]",
# wrapped or not, and ratchets the per-file count against ci/klog-baseline.txt:
# pre-existing violations are tolerated at their baseline count; a new raw
# [kagi] line (or growth in an existing file) fails. Migrating a file to
# klog! shrinks its count — regenerate the baseline afterwards with:
#   ci/check-klog.sh --write-baseline
# The ratchet becomes a pure prohibition once the baseline is empty.
set -u
cd "$(dirname "$0")/.."
python3 - "${1:-check}" <<'PY'
import pathlib, re, sys

mode = sys.argv[1]
baseline_path = pathlib.Path("ci/klog-baseline.txt")
pat = re.compile(r'(eprintln|println)!\(\s*"\[kagi\]')
skip_dirs = {"target", "vendor", ".claude", ".git"}

counts = {}
for p in sorted(pathlib.Path(".").rglob("*.rs")):
    s = p.as_posix()
    if s == "crates/kagi-ui-core/src/klog.rs":
        continue
    if skip_dirs & set(p.parts):
        continue
    n = len(pat.findall(p.read_text(encoding="utf-8", errors="replace")))
    if n:
        counts[s] = n

if mode == "--write-baseline":
    baseline_path.write_text("".join(f"{f} {n}\n" for f, n in sorted(counts.items())))
    print(f"wrote {baseline_path} ({sum(counts.values())} violations in {len(counts)} files)")
    sys.exit(0)

baseline = {}
if baseline_path.exists():
    for line in baseline_path.read_text().splitlines():
        f, n = line.rsplit(" ", 1)
        baseline[f] = int(n)

fail = 0
for f, n in sorted(counts.items()):
    b = baseline.get(f, 0)
    if n > b:
        print(f"::error::{f} has {n} raw (e)println!(\"[kagi]…\") call(s), baseline {b} — [kagi] contract lines must go through klog! (ADR-0096 / #396).")
        fail = 1
    elif n < b:
        print(f"::notice::{f} shrank to {n} raw [kagi] call(s) (baseline {b}) — regenerate: ci/check-klog.sh --write-baseline")
if fail:
    sys.exit(1)
print("OK: no new raw [kagi] lines outside klog! (ratchet).")
PY
