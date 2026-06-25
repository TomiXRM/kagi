# ADR-0120: Package-manager distribution (Homebrew, cargo-binstall, mise)

- Status: Accepted
- Date: 2026-06-25
- Follows: ADR-0047 (W20-RELEASE cross-platform distribution), ADR-0082 (in-app auto-update)

## Context

Kagi ships per-tag GitHub Releases (ADR-0047): a macOS `.dmg`, Linux `tar.gz`
(`bin/ + .desktop + icon`) and AppImage `.zip`, and a Windows `.zip`. Those are
hand-download / drag-install artifacts, and the in-app updater (ADR-0082,
`crates/kagi-domain/src/update.rs::pick_asset`) is keyed to their exact names.

Users asked to install Kagi the way they install everything else — through a
package manager: **Homebrew**, **`cargo binstall`**, and **mise**. None of the
existing artifacts fit those tools cleanly:

- `cargo binstall` and mise's `ubi` backend resolve a release asset *from its
  name* and extract a single binary from it. They expect an archive named with
  the Rust **target triple** containing the bare binary — not a `.dmg`, not a
  `bin/kagi`-nested tarball, and not an AppImage.
- macOS shipped **only** a `.dmg`, which `binstall`/`ubi` cannot extract.
- Homebrew needs a formula with per-platform URLs + SHA-256s.
- Kagi is **not on crates.io** (path + vendored deps — `gpui-terminal`), so
  `cargo binstall kagi` / `cargo install kagi` cannot resolve it from the
  registry.

## Decision

Add **triple-named bare-binary archives** to every release and drive all three
package managers from them. The existing `.dmg` / AppImage / `bin/`-tarball /
Windows-zip artifacts and the updater's `pick_asset` logic are **left
untouched** — the new archives are additive.

### New release artifacts

`xtask bundle-binarchive --target <triple> --bin <path>` (stdlib-only, mirrors
the other `bundle-*` commands) stages the bare binary (+ `LICENSE`) at the
archive root and writes:

| Target triple | Asset |
|---|---|
| `aarch64-apple-darwin` | `kagi-<v>-aarch64-apple-darwin.tar.gz` |
| `x86_64-unknown-linux-gnu` | `kagi-<v>-x86_64-unknown-linux-gnu.tar.gz` |
| `aarch64-unknown-linux-gnu` | `kagi-<v>-aarch64-unknown-linux-gnu.tar.gz` |
| `x86_64-pc-windows-msvc` | `kagi-<v>-x86_64-pc-windows-msvc.zip` |

`release.yml` runs this on each existing build leg (no new legs), so the archives
are covered by the existing `SHA256SUMS-*.txt` and upload globs. On macOS the
staged binary is re-`codesign`ed ad-hoc (arm64 refuses unsigned binaries).

### cargo-binstall

`[package.metadata.binstall]` in the root `Cargo.toml` points at those archives
(`{ bin }{ binary-ext }` at the root; `.zip`/`zip` override for the Windows
target). Because Kagi is not on crates.io, the documented entry point is:

```sh
cargo binstall --git https://github.com/TomiXRM/kagi kagi
```

### mise

The `ubi` backend consumes the triple archives directly — their names carry the
full triple, which `ubi` scores far above the AppImage/`.dmg` for the host
target, removing the asset-selection ambiguity that the legacy names had:

```sh
mise use -g "ubi:TomiXRM/kagi[exe=kagi]"
```

### Homebrew

`scripts/gen_homebrew_formula.sh` generates `kagi.rb` from the release's
`SHA256SUMS-*.txt`. The `release` job runs it and uploads `kagi.rb` as a release
asset, so

```sh
brew install https://github.com/TomiXRM/kagi/releases/latest/download/kagi.rb
```

always resolves to the newest version. The same file is drop-in ready for a
future `TomiXRM/homebrew-kagi` tap (which would also enable `brew upgrade`). The
formula installs the CLI (`bin.install "kagi"`); macOS-Intel and (when a leg is
skipped) arm64-Linux degrade to an `odie` "build from source" message.

### `kagi --version` / `--help`

`main()` short-circuits these before any theme / single-instance / GUI setup, so
all three install paths have a GUI-free smoke test (the Homebrew `test do` block
asserts `kagi --version`). No `[kagi]` contract line is involved.

## Consequences

- **Updater is unaffected functionally.** `pick_asset` still matches the legacy
  `.dmg` (macOS) and the first `.tar.gz`/`windows .zip` it finds (Linux/Windows).
  The new triple `.tar.gz`/`.zip` *also* match those substring rules, but every
  candidate contains a discoverable `kagi` binary, so whichever `find()` returns
  first still self-updates correctly. macOS ignores the new tarball (it only
  looks for `.dmg`).
- Each release gains four small archives that duplicate binaries already inside
  the platform bundles. Accepted as the price of clean tool resolution.
- `brew install <url>` is a one-shot install, not a tap, so `brew upgrade` won't
  see new versions until a `homebrew-kagi` tap exists — documented as the
  follow-up. Kagi's own in-app updater covers upgrades meanwhile.
- Not on crates.io: `cargo binstall`/mise-`cargo` need `--git`; revisit if the
  vendored `gpui-terminal` dependency is ever published.
