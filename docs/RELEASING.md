# Releasing Kagi

Operational procedure for ADR-0047 (distribution) and ADR-0038 (bundling).
Release completed `dev` changes through a reviewed PR into `main`; do not include
unfinished worktrees or tag a candidate before its main-branch CI is green.

## 1. Prepare the candidate

- Use an isolated worktree. Preserve the owner's checkout and unrelated changes.
- Bump only the root `Cargo.toml` package version. Workspace crates have independent
  versions. `cargo build` updates the matching `kagi` entry in `Cargo.lock`.
- Move Unreleased entries beneath a dated `## [X.Y.Z] - YYYY-MM-DD` heading,
  retain a fresh Unreleased section, and write `docs/releases/vX.Y.Z.md`.
- Describe shipped behavior and known limitations, not pending PRs as delivered.
- The tag, package/lock version, executable `--version`, archive names and generated
  macOS bundle versions must agree. The workflow does not enforce all of these.

## 2. Verify, push and review

Run one Cargo command at a time in the worktree:

```sh
cargo build
cargo fmt --all
cargo fmt --all --check
cargo clippy --workspace
KAGI_LOG_DIR=/tmp/kagi-release-check-unique cargo test --workspace
uv run --frozen --project ci check-all
./target/debug/kagi --version
```

Create a unique log directory before testing. Run the affected native scenarios
using an explicit `KAGI_GUI_E2E_ONLY` filter and the canonical
`.claude/skills/verify/SKILL.md` recipe; never launch the unfiltered runner.
Exercise the real application and preserve screenshots/evidence for the PR.

Push only after gates pass and verify the remote head equals the local commit.
For a `dev` → `main` release, place the release metadata on `dev` and open that
PR; do not delete the persistent `dev` branch. Follow `docs/agents/workflow.md`:
review the exact head, answer every review comment, require green blocking CI,
then merge with `--match-head-commit`.

## 3. Wait for the merged SHA's CI, then tag

Record the PR's merge SHA and find its `ci.yml` run by exact SHA, not the latest
run across unrelated branches:

```sh
gh run list --workflow ci.yml --commit <merge-sha> \
  --json databaseId,headSha,status,conclusion
gh run watch <ci-run-id> --exit-status
```

The authoritative gate is `blocking-ci` in the newest `ci.yml` run and its latest
attempt for that SHA. `check-release-ci` checks that identity again before release.
It requires the invariant matrix, gate lint/selftests, macOS workspace tests,
Windows build/stop proof and, when selected, Linux installer proof. Only an
unselected installer job may be skipped. Advisory Linux tests or lint do not
replace the aggregate. A successful older attempt is not sufficient.

After recording successful exact-SHA evidence, create the annotated tag on that
merge commit:

```sh
git tag -a vX.Y.Z <merge-sha> -m "Kagi X.Y.Z"
git push origin refs/tags/vX.Y.Z
```

Verify the remote tag peels to the intended commit. Never move a published tag.
A code fix needs a new reviewed commit and release decision; do not rewrite
history to hide a failed candidate.

## 4. Build and inspect the draft

The `v*` push starts `.github/workflows/release.yml`. Select its run by the tagged
SHA and watch that run. `workflow_dispatch` also uploads to a draft; it is not a
side-effect-free dry run. All four platform build legs must pass.

From v0.45.0, expect **13 assets**:

| Platform | Packages |
|---|---|
| macOS arm64 | `Kagi-X.Y.Z-arm64.dmg`, `kagi-X.Y.Z-arm64-macos.tar.gz` |
| Linux x86_64 | `kagi-X.Y.Z-x86_64-linux.tar.gz`, `kagi_Linux-AppImage_x86_64.zip`, `kagi_X.Y.Z-1_amd64.deb` |
| Linux aarch64 | `kagi-X.Y.Z-aarch64-linux.tar.gz`, `kagi_Linux-AppImage_aarch64.zip`, `kagi_X.Y.Z-1_arm64.deb` |
| Windows x86_64 | `kagi-X.Y.Z-x86_64-windows.zip` |

Plus `SHA256SUMS-{macos-arm64,linux-x86_64,linux-arm64,windows-x86_64}.txt`.
The old platform-less Linux tar aliases were transitional assets in v0.44.0.
Do not name the macOS tar `aarch64-macos`: older Linux updaters can misidentify it.

Before publication:

1. Inspect draft/tag/target, package names, nonzero sizes and release notes.
2. Download every asset into a fresh directory. Verify all four checksum files
   and that their entries cover every package exactly once.
3. Inspect archive contents, architecture and versions. macOS tar must contain
   `Kagi.app` plus `bin/kagi` pointing to its executable; Linux tar retains its
   versioned top-level directory, desktop entry and icon.
4. Verify macOS signing with `codesign --verify --strict`, both bundle version
   fields, `--version`, and the DMG's app. Run platform binaries/install checks
   on matching hosts; do not call archive inspection runtime evidence.

## 5. Publish and verify installation

Set the draft's body from the checked-in release notes, then publish deliberately:

```sh
gh release edit vX.Y.Z --notes-file docs/releases/vX.Y.Z.md
gh release edit vX.Y.Z --draft=false --latest
gh release view vX.Y.Z --json tagName,isDraft,isPrerelease,assets,url
```

Confirm the latest release is the intended version. Exercise `install.sh` with
`--version vX.Y.Z --prefix <fresh-temp-directory> --no-modify-path`, or a
version-pinned mise installation, without overwriting the owner's installation.
Record actual downloaded-binary version/signature and platform verification.

## Distribution limits

- macOS is arm64 only, ad-hoc signed; no Developer ID or notarization is claimed.
- Linux AppImage/tar builds still depend on supported host system libraries.
- A failed required platform leg blocks the draft; runner availability is not
  permission to omit an architecture silently.
- Keep `dev` aligned with the released main history without rewriting pushed
  branches. Never delete `dev` as part of PR cleanup.
