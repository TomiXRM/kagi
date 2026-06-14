# ADR-0082: In-app auto-update (Zed-style, signing-aware)

- Status: Proposed / Date: 2026-06-14
- Context: Kagi is distributed via GitHub Releases (ADR-0047): per-platform assets
  (`.dmg`, `.tar.gz`, AppImage zip, `windows.zip`) plus `SHA256SUMS-*.txt`. There
  is no in-app update path — users must notice a release and re-download manually.
  We want "notice a new version → fetch → update" like other GUI apps, **without**
  betraying Kagi's safety thesis (predict → confirm → execute; nothing destructive
  or silent). Decision (with the user): follow **Zed's own auto_update pattern**
  (Kagi is GPUI, same lineage) rather than swapping the whole pipeline for
  cargo-dist/Sparkle, and **design for code signing from the start** (notarization
  on macOS, Authenticode on Windows) even though signing lands in a later phase.

## Decision

Add a small, self-contained **updater** that checks GitHub Releases, and — only on
explicit user confirmation — downloads the platform asset, **verifies it**, swaps
it in, and relaunches. It reuses what Kagi already has: `ureq` (blocking HTTPS,
rustls) like `avatar_fetch`, `cx.background_spawn` for off-thread work, the
`~/.kagi/settings.json` hand-written-JSON store, and the header slot for UI. No new
heavyweight dependency (no `self_update`, no cargo-dist); version compare is a tiny
hand-rolled semver (the same no-extra-deps ethos as `xtask`'s version parsing).

The update itself is modeled as an **operation through the existing pipeline**:
`plan (what version → what version, size, source, verification) → confirm (modal) →
preflight (writable install dir, checksum/signature ok) → execute (swap) → verify
(relaunched binary reports the new version) → log (oplog)`. Auto-update is **opt-in
checking, always-confirmed installing** — Kagi never replaces itself silently.

### Layering (no git2/network in the view)
1. **domain** (`kagi-domain`, pure): `Version` parse + compare, `ReleaseInfo`
   (tag, notes, assets), `UpdatePlan` (current → latest, asset url, sha256, size),
   `pick_asset(platform, arch, assets)`. Unit-testable with no I/O.
2. **app/io** (`src/update/`, `ureq` + fs): `check_latest()` (GitHub API),
   `download(asset) -> tempfile`, `verify(file, sha256[, signature])`,
   `install(file)` (platform-specific swap), `relaunch()`.
3. **ui** (`src/ui/`): a header **"vX.Y.Z available"** chip → opens the update
   modal (plan preview) → Confirm runs the download/verify/install on a background
   spawn with progress, then relaunches. The view never calls `ureq`/fs directly —
   it goes through `src/update`.

### Check
- `GET https://api.github.com/repos/TomiXRM/kagi/releases/latest`, `User-Agent`
  header (GitHub requires one — already done for avatars). Parse `tag_name`,
  `body` (release notes), `assets[]` (name + `browser_download_url` + size).
- Compare `tag_name` (`vX.Y.Z`) against `env!("CARGO_PKG_VERSION")` via hand-rolled
  semver. Pre-releases ignored unless on a pre-release channel.
- Runs on startup (background, best-effort, failures silent) and on demand from a
  "Check for updates" menu item. Throttled (≤ once / N hours) via a timestamp in
  settings so we don't hammer the API.

### Verify (defence in depth — Kagi's "predict before danger")
1. **HTTPS** to GitHub (rustls) — transport integrity.
2. **SHA-256** of the downloaded asset against the release's `SHA256SUMS-*.txt`.
   Refuse to install on mismatch (loud error, keep current install untouched).
3. **OS code signature** (post-signing): the swapped binary is Developer-ID-signed
   + notarized (macOS) / Authenticode-signed (Windows), so the OS validates it on
   relaunch.
4. **(Hardening, optional later)** a detached **minisign/EdDSA** signature over the
   checksums file, verified with a public key baked into the binary — authenticates
   the *update channel itself* (Sparkle-style), not just the artifact. Tracked but
   not required for the first signed release.

### Install (platform swap, then relaunch)
- **Linux** (tar.gz / AppImage): the simplest case (no OS signing gate). Replace
  the running binary/AppImage (rename-in-place: the running inode stays open),
  `exec` the new one. Works unsigned today.
- **macOS** (`.dmg` → `Kagi.app`): mount (`hdiutil attach`), copy the new
  `Kagi.app` next to the current one, atomic swap (move old aside → move new in),
  `hdiutil detach`, relaunch via `open`. Clean (no Gatekeeper re-prompt) **only
  once Developer-ID-signed + notarized**; until then it works but re-warns.
- **Windows** (`kagi.exe`): a running `.exe` can't be overwritten, so rename the
  running exe (`MoveFileEx`), write the new one alongside, relaunch, and let the
  old be cleaned next start — the standard self-replace trick. Clean once
  Authenticode-signed; until then SmartScreen warns.
- Each path is isolated behind `#[cfg(target_os = ...)]`; the swap writes to a temp
  first and only commits on success, so a failed/interrupted update never leaves a
  half-written install.

### Signing (release pipeline; this is the real gate for a good UX)
- **macOS**: add Developer ID signing + `notarytool` notarization + stapling to the
  `bundle-macos`/`dmg-macos` steps in `release.yml` (ADR-0038 Phase 2). Needs an
  Apple Developer Program membership + secrets (cert, app-specific password).
- **Windows**: Authenticode-sign `kagi.exe` with `signtool` in the Windows leg.
  Needs a code-signing certificate (OV/EV) in secrets.
- The updater design above is signing-agnostic: turning signing on improves the
  install UX (no OS warnings) but does not change the updater code.

### Settings (`~/.kagi/settings.json`, hand-written JSON, no serde)
- `update.auto_check` (bool, default true) — startup background check on/off.
- `update.channel` (`"stable"`, future `"beta"`).
- `update.skipped_version` (string) — "Skip this version" hides the banner for it.
- `update.last_checked` (unix ts) — throttle.
- Surfaced in the Settings window (ADR-0080).

## Phased rollout
- **Phase 0 — notify only (no signing, ship now):** background check + a header
  "vX.Y.Z available" chip that opens the GitHub release page. Manual download.
  Zero risk, no pipeline change, no signing. Validates the check/compare/UI plumbing.
- **Phase 1 — in-app update, unsigned:** download + SHA-256 verify + swap +
  relaunch behind the confirm modal. Clean on Linux; macOS/Windows work but show OS
  warnings (documented as experimental).
- **Phase 2 — signing:** Developer-ID/notarize (macOS) + Authenticode (Windows) in
  `release.yml` → warning-free updates. Optional minisign channel signature.

## Alternatives considered
- **cargo-dist + axoupdater** — the "famous existing method"; would regenerate the
  whole release pipeline and ship an updater for free, but means abandoning the
  hand-rolled `xtask` (dmg/AppImage/icon) and adopting its conventions. Rejected for
  now to keep the existing, working pipeline and stay idiomatic to GPUI.
- **Sparkle** (macOS) — the de-facto Mac standard, but macOS-only, ObjC, and needs
  an appcast feed + Developer ID anyway. Too heavy / not cross-platform.
- **`self_update` crate** — fine for the GitHub-release swap, but pulls a dep tree
  and we already have `ureq`; the swap logic we need is small.

## Consequences
- New network egress: a GitHub API call on startup (best-effort, throttled,
  toggleable). No telemetry — only the public releases endpoint.
- New `src/update/` module + a small `kagi-domain` version/plan type; the view stays
  network/fs-free.
- A real auto-update UX is gated on **code signing** (external cost: Apple Developer
  membership + a Windows cert). Phase 0/1 ship value before that lands.
- Consistent with the safety thesis: checking is opt-in and silent-failing;
  installing is always previewed + confirmed + checksum-verified, writes atomically,
  and never runs a destructive command.
