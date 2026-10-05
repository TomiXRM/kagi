"""Exercise root install.sh against locally served, real release artifacts.

Run after xtask bundles a real binary (not a stub), on that binary's host OS:
    uv run --frozen --project ci install-fixture --dist target/dist
"""

from __future__ import annotations

import argparse
import os
import platform
import subprocess
import tempfile
import threading
import tomllib
from collections.abc import Iterator
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]


@contextmanager
def release_server(
    version: str,
    archive: Path,
    checksum: bytes,
    checksum_name: str,
    *,
    latest_available: bool = False,
) -> Iterator[tuple[str, dict[str, int]]]:
    """Serve exactly the assets the install script may request; count API calls."""
    counts = {"api": 0, "archive": 0, "checksum": 0}
    asset_path = f"/{version}/{archive.name}"
    sums_path = f"/{version}/{checksum_name}"

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            if self.path == "/latest":
                counts["api"] += 1
                if latest_available:
                    payload = f'{{"tag_name": "{version}"}}'.encode()
                    self.send_response(200)
                    self.send_header("Content-Length", str(len(payload)))
                    self.end_headers()
                    self.wfile.write(payload)
                else:
                    self.send_error(503, "release API unavailable")
                return
            if self.path == asset_path:
                counts["archive"] += 1
                self.send_response(200)
                self.send_header("Content-Length", str(archive.stat().st_size))
                self.end_headers()
                with archive.open("rb") as data:
                    while chunk := data.read(1024 * 1024):
                        self.wfile.write(chunk)
                return
            if self.path == sums_path:
                counts["checksum"] += 1
                self.send_response(200)
                self.send_header("Content-Length", str(len(checksum)))
                self.end_headers()
                self.wfile.write(checksum)
                return
            self.send_error(404, "unknown fixture path")

        def log_message(self, format: str, *args: object) -> None:
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}", counts
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


def check(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def run_install(url: str, prefix: Path, *arguments: str) -> subprocess.CompletedProcess[str]:
    env = {
        **os.environ,
        "KAGI_RELEASE_BASE_URL": url,
        "KAGI_RELEASE_API_URL": f"{url}/latest",
    }
    return subprocess.run(
        ["sh", str(ROOT / "install.sh"), *arguments, "--prefix", str(prefix), "--no-modify-path"],
        cwd=ROOT,
        env=env,
        capture_output=True,
        text=True,
        timeout=90,
        check=False,
    )


def verify(dist: Path) -> None:
    with (ROOT / "Cargo.toml").open("rb") as manifest:
        version = f"v{tomllib.load(manifest)['package']['version']}"
    macos = platform.system() == "Darwin"
    arch = "aarch64" if platform.machine() in ("arm64", "aarch64") else "x86_64"
    os_name = "macos" if macos else "linux"
    checksum_arch = "arm64" if arch == "aarch64" else arch
    if macos and arch == "aarch64":
        arch = "arm64"
    archive = dist / f"kagi-{version[1:]}-{arch}-{os_name}.tar.gz"
    sums = dist / f"SHA256SUMS-{os_name}-{checksum_arch}.txt"
    check(archive.is_file() and sums.is_file(), f"release tar/SHA missing from {dist}")
    checksum = sums.read_bytes()

    with tempfile.TemporaryDirectory(prefix="kagi-installer-fixture-") as directory:
        root = Path(directory)
        with release_server(version, archive, checksum, sums.name) as (url, requests):
            prefix = root / "successful-install"
            result = run_install(url, prefix, "--version", version)
            check(result.returncode == 0, f"real archive install failed: {result.stderr}")
            check(
                requests == {"api": 0, "archive": 1, "checksum": 1},
                f"explicit tag requests: {requests}",
            )
            binary = prefix / "bin/kagi"
            check(
                binary.is_file() and os.access(binary, os.X_OK),
                "release CLI missing or not executable",
            )
            if macos:
                app = prefix / "Kagi.app"
                check(binary.is_symlink(), "macOS CLI must be a symlink")
                check(
                    binary.resolve() == (app / "Contents/MacOS/kagi").resolve(),
                    "CLI resolves outside app",
                )
                signature = subprocess.run(
                    ["codesign", "--verify", "--deep", "--strict", str(app)],
                    capture_output=True,
                    text=True,
                    check=False,
                )
                check(signature.returncode == 0, f"installed signature invalid: {signature.stderr}")
                attributes = subprocess.run(
                    ["xattr", "-lr", str(app)],
                    capture_output=True,
                    text=True,
                    check=False,
                )
                check(
                    attributes.returncode == 0 and "com.apple.quarantine" not in attributes.stdout,
                    "installed application still has quarantine",
                )
                print(
                    "installed macOS app signature verified; quarantine absent; "
                    "CLI symlink resolves"
                )
            else:
                for relative in (
                    "share/applications/com.tomixrm.kagi.desktop",
                    "share/icons/hicolor/512x512/apps/kagi.png",
                ):
                    check((prefix / relative).is_file(), f"release resource missing: {relative}")
            actual = subprocess.run(
                [str(binary), "--version"],
                env={
                    **os.environ,
                    "KAGI_LOG_DIR": str(root / "cli-state"),
                    "KAGI_NO_ACTIVATE": "1",
                },
                capture_output=True,
                text=True,
                timeout=30,
                check=False,
            )
            check(
                actual.returncode == 0
                and actual.stdout.strip() == f"kagi {version[1:]}"
                and not actual.stderr
                and not (root / "cli-state").exists(),
                f"installed CLI --version failed: {actual.stdout} {actual.stderr}",
            )
            print(f"real {os_name} tar installed; {binary} --version: {actual.stdout.strip()}")

            before = requests.copy()
            dry_prefix = root / "dry-run"
            dry = run_install(url, dry_prefix, "--version", version, "--dry-run")
            check(
                dry.returncode == 0 and not dry_prefix.exists(),
                f"dry-run mutated prefix: {dry.stderr}",
            )
            check(requests == before, f"dry-run issued HTTP requests: {requests}")

        with release_server(version, archive, checksum, sums.name, latest_available=True) as (
            url,
            requests,
        ):
            prefix = root / "successful-install"
            result = run_install(url, prefix)
            check(result.returncode == 0, f"latest release install failed: {result.stderr}")
            check(
                requests == {"api": 1, "archive": 1, "checksum": 1},
                f"latest success requests: {requests}",
            )
            upgraded = subprocess.run(
                [str(prefix / "bin/kagi"), "--version"],
                env={
                    **os.environ,
                    "KAGI_LOG_DIR": str(root / "cli-state"),
                    "KAGI_NO_ACTIVATE": "1",
                },
                capture_output=True,
                text=True,
                timeout=30,
                check=False,
            )
            check(
                upgraded.returncode == 0
                and upgraded.stdout.strip() == f"kagi {version[1:]}"
                and not upgraded.stderr
                and not (root / "cli-state").exists(),
                f"latest installation replaced a working CLI with a broken one: {upgraded}",
            )
            print("latest API resolved and replaced the existing installation; CLI still works")

        invalid = b"".join(
            (b"1" if line[:1] == b"0" else b"0") + line[1:]
            if line.rstrip().endswith(b" " + archive.name.encode())
            else line
            for line in checksum.splitlines(keepends=True)
        )
        with release_server(version, archive, invalid, sums.name) as (url, requests):
            prefix = root / "checksum-rejected"
            result = run_install(url, prefix, "--version", version)
            check(result.returncode != 0, "corrupt checksum was accepted")
            check(not prefix.exists(), "checksum rejection created installation files")
            check(
                requests == {"api": 0, "archive": 1, "checksum": 1},
                f"checksum requests: {requests}",
            )
            print("tampered SHA rejected without installing files")

        with release_server(version, archive, checksum, sums.name) as (url, requests):
            prefix = root / "api-failed"
            result = run_install(url, prefix)
            lines = result.stderr.strip().splitlines()
            check(
                result.returncode != 0 and len(lines) == 1 and "--version" in lines[0],
                f"API failure guidance should be one line: {result.stderr!r}",
            )
            check(not prefix.exists(), "API failure created installation files")
            check(
                requests == {"api": 1, "archive": 0, "checksum": 0},
                f"latest failure requests: {requests}",
            )
            print("latest API failure provides single-line --version guidance")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dist", type=Path, required=True, help="downloaded release tar and SHA")
    arguments = parser.parse_args()
    verify(arguments.dist)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
