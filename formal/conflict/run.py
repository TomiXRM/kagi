"""One entry point: expected baselines plus load-bearing red mutations.

Exit 0: all expectations and mutation checks match; 1: model mismatch;
2: prerequisite, launch, timeout, or malformed evidence failure.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent
RELEASE = "v1.8.0"
URL = f"https://github.com/tlaplus/tlaplus/releases/download/{RELEASE}/tla2tools.jar"


def tlc_jar():
    expected = (ROOT / "tla/TLA2TOOLS.sha256").read_text().split()[0]
    cache = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
    cache = cache / "kagi-formal-conflict" / RELEASE
    cache.mkdir(parents=True, exist_ok=True)
    jar = cache / "tla2tools.jar"
    if not jar.exists():
        # A unique temporary file keeps interrupted/concurrent downloads out of
        # the executable cache. Never execute bytes before verifying their hash.
        with tempfile.NamedTemporaryFile(dir=cache, suffix=".jar") as download:
            subprocess.run(["curl", "--fail", "--location", "--silent", "--show-error",
                            "--max-time", "30", URL, "--output", download.name],
                           check=True, timeout=35)
            if hashlib.file_digest(download, "sha256").hexdigest() != expected:
                raise RuntimeError("downloaded TLC SHA-256 mismatch")
            # Keep NamedTemporaryFile cleanup portable: copy verified bytes, then
            # atomically publish the complete file without renaming its handle.
            download.seek(0)
            with tempfile.NamedTemporaryFile(dir=cache, delete=False) as ready:
                pending = Path(ready.name)
                ready.write(download.read())
            try:
                pending.replace(jar)
            finally:
                pending.unlink(missing_ok=True)
    with jar.open("rb") as cached:
        if hashlib.file_digest(cached, "sha256").hexdigest() != expected:
            raise RuntimeError(f"cached TLC SHA-256 mismatch: remove {jar} and rerun")
    java = subprocess.run(["java", "-version"], text=True, capture_output=True,
                          check=True, timeout=10)
    if not re.search(r'version "21(?:[.\"])', java.stderr + java.stdout):
        raise RuntimeError("Java 21 required on PATH (CI: Temurin 21)")
    return jar


def run(script, args=(), *, env=None, timeout=180):
    return subprocess.run([sys.executable, str(ROOT / script), *args],
                          cwd=ROOT, env=env, timeout=timeout).returncode


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite", choices=("all", "pure", "tla"), default="all")
    parser.add_argument("--broken", action="store_true",
                        help="TLA only: retain HOLD expectations so broken guards exit 1")
    parser.add_argument("checks", nargs="*", help="optional TLA check names")
    args = parser.parse_args()
    if (args.broken or args.checks) and args.suite != "tla":
        parser.error("--broken and check names require --suite tla")
    started = time.monotonic()
    summary = {"suite": args.suite, "checks": [], "ok": False}
    try:
        if args.suite in ("all", "pure"):
            code = run("pure/run_all.py")
            summary["checks"].append({"name": "pure", "exit": code})
            if code not in (0, 1):
                raise RuntimeError(f"pure harness tooling failure: exit {code}")
        if args.suite in ("all", "tla"):
            env = dict(os.environ, TLA2TOOLS_JAR=str(tlc_jar()))
            code = run("tla/run.py", [*args.checks, *(["--broken"] if args.broken else [])], env=env)
            summary["checks"].append({"name": "tla", "exit": code})
            if code not in (0, 1):
                raise RuntimeError(f"TLC harness tooling failure: exit {code}")
            if not args.broken and not args.checks:
                # The normal table expects counterexamples from broken guards.
                # Independently retain their original HOLD assertions and demand
                # every individual negative control produces a real red witness.
                red = run("tla/run.py", ["--broken"], env=env, timeout=45)
                evidence = json.loads((ROOT / "tla/broken-results/summary.json").read_text())
                rows = evidence["checks"]
                detected = red == 1 and bool(rows) and all(
                    row["expectation"] == "HOLD" and row["result"] == "REFUTE"
                    and row["tlc_exit"] == (
                        13 if row["invariant"] == "ContinueEventuallyRecordedOrReported" else 12
                    ) and bool(row["trace"]) for row in rows)
                summary["checks"].append({"name": "tla-broken", "exit": red,
                                          "detected": detected, "mutations": len(rows)})
                code = run("tla/trace/klog2events.py", env=env)
                summary["checks"].append({"name": "runtime-traces", "exit": code})
                if code not in (0, 1):
                    raise RuntimeError(f"trace harness tooling failure: exit {code}")
        summary["ok"] = all(row.get("detected", row["exit"] == 0) for row in summary["checks"])
        code = 0 if summary["ok"] else 1
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as exc:
        summary["error"] = str(exc)
        code = 2
    summary["seconds"] = round(time.monotonic() - started, 3)
    # Subset debugging never overwrites the full-suite receipt.
    name = "summary.json" if args.suite == "all" else "summary-selected.json"
    (ROOT / name).write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, sort_keys=True), flush=True)
    return code


if __name__ == "__main__":
    raise SystemExit(main())
