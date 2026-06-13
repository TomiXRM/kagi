<div align="center">

<img src="assets/icon/icon_256x256.png" width="128" alt="Kagi icon" />

# Kagi 🔑

**A safety-first, commit-graph-centric Git GUI client**

Built with Rust + [GPUI](https://www.gpui.rs/) — the UI framework behind [Zed](https://zed.dev/)

![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-blue)
![Rust](https://img.shields.io/badge/built%20with-Rust-orange?logo=rust)
![GPUI](https://img.shields.io/badge/UI-GPUI-8A2BE2)
[![Release](https://img.shields.io/github/v/release/TomiXRM/kagi?include_prereleases)](https://github.com/TomiXRM/kagi/releases)
[![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)

<img src="docs/images/hero.png" width="850" alt="Kagi main window — commit graph, branch tree, inspector" />

[日本語の説明はこちら](./README.ja.md)

</div>

---

Kagi shows you **exactly what will happen before any Git operation runs** — and is engineered so that it *cannot* destroy your repository. Every write operation goes through a `plan → confirm → preflight → execute → verify` pipeline, and the dangerous commands simply don't exist in the codebase.

## ✨ Features

- 🌳 **Commit graph first** — GitKraken-style lanes, ref badges, HEAD ring, merge nodes, WIP row, virtualized for 10k+ commits
- 🔍 **Rich commit inspector** — author avatars (GitHub), co-authors, changed-file tree, syntax-highlighted diffs
- 📊 **Per-file diffstat** — `+N −M` counts with green/red mini bars in every file list
- ✅ **Commit suite** — pre-commit checklist (conflict markers / secrets / large binaries), draft autosave per branch, structured message templates (`type(scope): summary` + Test/Risk), amend with SHA-change preview
- 🤖 **Smart commit messages** — rule-based generation always available; **local Ollama LLM strictly opt-in** (staged diff only, localhost only, explicit consent dialog)
- 🧪 **Dry-run before danger** — cherry-pick / revert / checkout conflicts are predicted with libgit2 *in-memory* merges, without touching your working tree
- ⚔️ **Conflict resolution** — when merge / rebase / cherry-pick / revert conflict, Kagi enters **Conflict Mode**: a GitKraken-style 3-pane editor with file / chunk / **line-level** accept checkboxes, live Result preview/edit, a conflict dashboard, and Save→stage / Continue flow — or abort to restore the pre-operation state
- 🗑️ **Backup-then-discard** — discarding unstaged changes first snapshots the file content into the object database and records it in the operation log, so it is always recoverable
- 🪄 **Async everything** — checkout, commit, stash, pull/push, cherry-pick… all run off the UI thread with busy indicators; the window never freezes
- 🖥️ **Integrated terminal** — with selection, ⌘C/⌘V, and theme-matched colors
- 🎨 **6 color themes** — Catppuccin, Xcode dark/light, One dark/light, Monokai vivid
- 🌐 **English / Japanese UI** — explanatory prose is localized while Git domain words (Commit, Push, Pull, merge, conflict, branch…) stay English in both languages
- 🔍 **Uniform UI zoom** — header, panels, modals, inspector, menus, and the commit graph all scale together
- 🗂️ **Repo tabs**, branch-prefix tree sidebar (`feat/…`, grouped remotes), operation log, and a native menu bar

<div align="center">
<img src="docs/images/commit-panel.png" width="850" alt="Commit panel — staging, diffstat bars, commit preview, message template" />
</div>

<div align="center">
<img src="docs/images/conflict.png" width="850" alt="Conflict Mode — GitKraken-style 3-pane editor with file/chunk/line-level accept, live Result preview, and conflict dashboard" />
</div>

## 🔒 Safety design

This is the core of Kagi, not an afterthought:

| Guarantee | How |
|-----------|-----|
| You always see the outcome first | Every operation shows a plan modal: current state → predicted state, warnings, blockers, and a recovery recipe. With blockers present, the execute button doesn't even render |
| No destructive commands exist | `git push --force`, `reset --hard`, and `git clean` are **not implemented anywhere** in the codebase |
| Conflicts predicted, not discovered | In-memory merge dry-runs — your working tree is untouched when a conflict is foreseen |
| Conflicts are recoverable, not a trap | Entering Conflict Mode is always reversible — abort restores the exact pre-operation state, and in-progress resolutions are autosaved |
| Nothing is silently lost | Stash before checkout, ODB blob backups before discard, and an append-only operation log (`~/.kagi/operations.jsonl`) with before/after states |
| Ref moves are last | Operations write the working tree first and move refs last, so a mid-operation failure leaves HEAD where it was |

## 📦 Install

Grab the latest build from [**GitHub Releases**](https://github.com/TomiXRM/kagi/releases). Each release ships with `SHA256SUMS-*.txt` — please verify your download.

| OS | Asset |
|----|-------|
| macOS (Apple Silicon) | `Kagi-<version>-arm64.dmg` |
| Linux (x86_64 / arm64) | `kagi-<version>-<arch>.tar.gz` (binary + `.desktop` + icon), or the AppImage zip `kagi_Linux-AppImage_<arch>.zip` |

For the AppImage: `unzip kagi_Linux-AppImage_<arch>.zip && bash scripts/install_linux_desktop.sh` registers it under `~/.local` (icon + `.desktop` entry, fully offline).

### ⚠️ macOS: first launch on an unsigned build

Kagi is **not yet notarized by Apple** (ad-hoc signature only — no Apple Developer ID yet), so Gatekeeper will warn that the developer cannot be verified. Choose either:

1. **Right-click `Kagi.app` → Open → Open** (needed once; afterwards it opens normally), or
2. Remove the quarantine attribute:

   ```sh
   xattr -dr com.apple.quarantine /Applications/Kagi.app
   ```

Signing + notarization (ADR-0038 Phase 2) is planned once an Apple Developer Program membership is in place.

## 🛠️ Build from source

Requirements: Rust stable (rustup) and macOS with **Xcode Command Line Tools only** (no full Xcode needed — Kagi uses GPUI's `runtime_shaders`).

```sh
git clone https://github.com/TomiXRM/kagi.git
cd kagi
cargo run --release -- /path/to/your/repo
```

- First build takes a few minutes (gpui / libgit2); afterwards it's seconds
- Bare repositories are not supported (point it at a normal repo with a working tree)

### Try it without touching your repos

```sh
REPO=$(bash scripts/make_fixture.sh)   # generates a playground repo
cargo run -- "$REPO"
```

The fixture includes branches, a merge, a remote (ahead/behind), tags, a stash, and a dirty working tree.

### Package it yourself

`xtask` builds the distributables with stock macOS tools only (no Homebrew, no cargo-bundle):

```sh
bash scripts/make_icon.sh                 # rounded icon → assets/icon/ (icns + PNGs)
cargo run -p xtask -- bundle-macos        # target/dist/Kagi.app (ad-hoc signed)
cargo run -p xtask -- dmg-macos           # target/dist/Kagi-<version>-<arch>.dmg
cargo run -p xtask -- bundle-linux        # target/dist/kagi-<version>-<arch>.tar.gz
cargo run -p xtask -- bundle-appimage --bin target/release/kagi  # AppImage zip (needs appimagetool on Linux)
```

Tagging `v*` runs the [release workflow](.github/workflows/release.yml): macOS arm64 + Linux x86_64/arm64 builds (tar.gz + AppImage zip), checksums, and a draft GitHub release.

## 🧑‍💻 Development

```sh
cargo test --workspace    # 36 integration suites + unit tests
```

- Design docs: [docs/requirements.md](docs/requirements.md) · [docs/architecture.md](docs/architecture.md) · [ADRs](docs/adr/)
- Ticket board: [docs/tickets/INDEX.md](docs/tickets/INDEX.md)
- **Never test against a real repository** — use `scripts/make_fixture.sh` / tempdirs. `KAGI_AUTO_CONFIRM` and the other `KAGI_*` env vars are headless-testing tools only

### 🏗️ v1.0 re-architecture (in progress, `re-architecture` branch)

Kagi is being re-architected into a layered Cargo workspace so the safety-first
design is enforced by the type system, not by convention. The plan, research, and
decisions live in [docs/rearch/](docs/rearch/) (inventory, 10 domain research docs,
`architecture.md`, migration log) and [ADRs 0072–0078](docs/adr/).

Target layering (dependencies point down only):

```
kagi (bin) → kagi-ui → kagi-app → kagi-git → kagi-domain
                                   (git2 here   (pure: no git2,
                                    only)        no gpui)
```

The **core invariant**: the UI never opens a `git2::Repository` or calls `git2::`
directly — all Git work flows through the `plan → confirm → preflight → execute →
verify → log` pipeline. Progress so far: the pure **`kagi-domain`** crate is
extracted (14 modules — commit/graph/diff/conflict model/rules/plan types, zero
git2/gpui), and **`src/ui` is now completely git2-free** (every access routes
through `kagi::git::Backend`), enforced by a CI grep gate. `cargo test --workspace`
stays green at every step. Remaining: the per-session app-state model
(`AppState`/`RepoSession`/`OperationController`), the view decomposition, and the
async worker-thread backend.

## 🗺️ Status

Actively developed. Implemented: full commit-graph UX, branch/tag/stash/worktree management, staging + commit suite, cherry-pick / revert / amend / discard with dry-run safety, **conflict resolution (Conflict Mode + line-level 3-pane editor + merge-into-conflict)**, repo tabs, themes, **English/Japanese UI**, uniform UI zoom, integrated terminal, GitHub avatars, distribution pipeline. Roadmap lives in [docs/requirements.md](docs/requirements.md) and the ticket index.

## 📄 License

[MIT](LICENSE). The vendored terminal component (`vendor/gpui-terminal`) is upstream-licensed MIT OR Apache-2.0 and is used here under MIT.
