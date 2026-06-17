# Releasing Kagi

The operational runbook for cutting a release. Design rationale lives in
**ADR-0047** (cross-platform release) and **ADR-0038** (app bundling); this file
is the step-by-step procedure.

## TL;DR

```sh
# on a feature/release branch with green CI:
# 1. bump version + changelog, commit
vim Cargo.toml CHANGELOG.md          # version = "X.Y.Z" ; add a "## [X.Y.Z]" section
cargo build                          # refresh Cargo.lock
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "chore: release X.Y.Z (<one-line summary>)"
git push

# 2. tag → this triggers the build + a DRAFT GitHub release
git tag -a vX.Y.Z -m "Kagi X.Y.Z — <summary>"
git push origin vX.Y.Z

# 3. wait for the build (~14 min), then publish + merge
gh run watch "$(gh run list --workflow=release.yml -L1 --json databaseId -q '.[0].databaseId')" --exit-status
gh release edit vX.Y.Z --draft=false --latest
gh pr merge <PR#> --merge --delete-branch
git checkout main && git pull --ff-only
```

## How it works

- **Trigger:** pushing a `v*` tag runs `.github/workflows/release.yml`
  (also runnable manually via *workflow_dispatch* for a dry run — it still
  uploads to a draft).
- **Build matrix (4 targets):** `macos-arm64`, `linux-x86_64`,
  `linux-arm64` (`ubuntu-24.04-arm`), `windows-x86_64`. Each builds via the
  `xtask` helper and emits a `SHA256SUMS-<target>.txt`.
- **Draft job:** downloads every artifact and creates a **draft** GitHub release
  (`softprops/action-gh-release`, `draft: true`) with all files attached.
  Publishing is a deliberate manual step (below) — the workflow never auto-publishes.
- **Artifacts (10):** `Kagi-X.Y.Z-arm64.dmg`, `kagi-X.Y.Z-x86_64.tar.gz`,
  `kagi-X.Y.Z-aarch64.tar.gz`, `kagi_Linux-AppImage_{x86_64,aarch64}.zip`,
  `kagi-X.Y.Z-x86_64-windows.zip`, plus four `SHA256SUMS-*.txt`.

## Step by step

### 0. Pre-flight (must be green)
```sh
cargo build
cargo fmt --all --check        # blocking-ish: the advisory CI lint job runs this
cargo clippy --workspace       # advisory (pre-existing v0.2.0 debt; warnings ok)
cargo test --workspace         # BLOCKING in CI (macOS) — must be 0 failed
grep -rnE 'git2::|Repository::open' src/ui/   # must be empty (CI gate, ADR-0078)
```
Do the release on a branch (not `main`) so it lands via a PR.

### 1. Version + changelog
- Bump `version` in the **root `Cargo.toml`** only. `crates/kagi-domain` carries
  its **own independent version** — leave it unless that crate changed in a way
  worth versioning.
- Run `cargo build` to update `Cargo.lock`.
- Add a `## [X.Y.Z] — YYYY-MM-DD` section to `CHANGELOG.md` (Added / Fixed /
  Changed). Versioning convention so far: **`0.3.x` minor bumps for features and
  fixes** (e.g. 0.3.15 → 0.3.16); internal-only refactors still get a bump and a
  `### Changed (internal)` note.
- Commit `chore: release X.Y.Z (...)` and push.

### 2. Tag (kicks off the build)
```sh
git tag -a vX.Y.Z -m "Kagi X.Y.Z — <summary>"
git push origin vX.Y.Z
```
The tag may sit on the **release branch tip** (before the PR merges); it ends up
in `main`'s history once the PR is merged. The tag must match `v*`.

### 3. Watch the build (~13–14 min)
```sh
gh run list --workflow=release.yml -L1
gh run watch <run-id> --exit-status     # blocks until success/failure
```
`gh run watch` can outlast a 10-min shell timeout — run it in the background and
wait for the completion notification, or re-poll with `gh run view <id>`.

### 4. Publish
```sh
gh release view vX.Y.Z --json isDraft,assets -q '.isDraft, (.assets|length)'  # draft=true, 10 assets
gh release edit vX.Y.Z --draft=false --latest
gh release list -L1     # confirm: vX.Y.Z  Latest
```

### 5. Merge the PR + sync
```sh
gh pr merge <PR#> --merge --delete-branch
git checkout main && git pull --ff-only
git merge-base --is-ancestor vX.Y.Z HEAD && echo "tag is in main"
```

## Notes & gotchas
- **macOS Intel is out of scope** (ADR-0047) — only `macos-arm64` is built.
- **Code signing:** the macOS app is **ad-hoc signed** (no Developer ID); users
  may need to right-click → Open or clear the quarantine attribute on first run.
- **`linux-arm64` runs on `ubuntu-24.04-arm`** — if that runner is flaky, the leg
  can fail without the others; re-run just that job.
- **Don't tag twice.** If a build fails, fix on the branch, delete the bad tag
  locally + remotely (`git tag -d vX.Y.Z; git push origin :refs/tags/vX.Y.Z`),
  delete the draft release, then re-tag.
- **The lint CI job is advisory** (`continue-on-error`) — fmt must be clean to
  keep it green, but clippy warnings won't block. The **macOS test job is
  blocking**.
- A failed/duplicate artifact upload silently overwrites on same-name collisions;
  the per-target arch in artifact names prevents that (fixed historically).
