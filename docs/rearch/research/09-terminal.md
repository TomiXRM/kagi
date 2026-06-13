# 09 — Integrated Terminal (re-architecture research)

Sub-agent #9 / DOMAIN: integrated terminal (selection, copy/paste, theme-matched
colors, per-repo PTY sessions). RESEARCH ONLY.

Target layering: **app (terminal sessions owned per `RepoSession`) → ui (terminal
view in bottom panel)**. Invariant: UI never calls git2 directly; the terminal
backend is isolated behind a port so the vendored fork can be swapped for upstream
later.

Sources read: `src/ui/terminal.rs`, `src/ui/mod.rs` (`terminal_sessions` field +
`ensure_terminal` + `render_terminal_body`), `vendor/gpui-terminal/` (Cargo.toml,
Cargo.toml.orig, README, lib.rs, grep of view.rs/mouse.rs/colors.rs),
`Cargo.toml`, ADR-0007/0008/0017/0035. Web: zortax/gpui-terminal GitHub +
DeepWiki (indexed 2026-01-20).

---

## 1. Kagi 現状

### KagiTerminalSession (`src/ui/terminal.rs`)
- A thin wrapper around **gpui-terminal `TerminalView` + portable-pty 0.9**,
  introduced as T-BP-007 (ADR-0008).
- Fields: `view: Option<Entity<TerminalView>>` (lazy — `None` until the Terminal
  tab is first shown, and re-`None`'d when the shell exits), `start_error`,
  `repo_path` (used as shell cwd), `paste_writer: Option<SharedWriter>`.
- **Lifecycle**: PTY spawned lazily on first tab show via `ensure_terminal`;
  preserved across tab switches; on shell exit the `with_exit_callback` clears
  `session.view = None` so the next show restarts it.
- **PTY plumbing** (`build_terminal_view`): `NativePtySystem::openpty` (24×80) →
  `CommandBuilder` (`$SHELL` or `/bin/zsh`, cwd=repo root, `TERM=xterm-256color`,
  `COLORTERM=truecolor`) → spawn on slave → take writer/reader of master. The
  master is wrapped in `Arc<Mutex<…>>` and shared with `with_resize_callback`
  (resizes the real PTY when the grid reflows).
- **SharedWriter**: `Arc<Mutex<Box<dyn Write + Send>>>`. `portable_pty::take_writer`
  is once-only, but both the `TerminalView` (keystrokes) and the cmd-v paste path
  need to write, so all writes funnel through one mutex handle. **cmd-v paste is
  implemented app-side** (an ancestor key listener in `render_terminal_body` reads
  the gpui clipboard and writes straight to the PTY) because gpui-terminal 0.1.0
  has no built-in paste.
- **Theme integration (W9-THEME / ADR-0036)**: `build_color_palette()` maps the
  active Kagi theme (`theme().term_*`) onto `ColorPalette::builder()` — bg/fg/
  cursor, 8 normal + 8 bright ANSI colors, and a translucent `selection` color
  (W8-TERMSEL). `build_terminal_config()` bundles font + palette and is reused
  for live theme switching via `TerminalView::update_config`.
- **Font**: `pick_font_family()` probes macOS font dirs for Nerd Fonts
  (RobotoMono → JetBrainsMono → Hack), falling back to Menlo.

### Per-repo PTY ownership (`src/ui/mod.rs`)
- `KagiApp.terminal_sessions: HashMap<PathBuf, KagiTerminalSession>` (W4-TABS) —
  **one PTY session per repo, keyed by repo root**. `ensure_terminal` does
  `entry(repo_path).or_insert_with(KagiTerminalSession::new)` then delegates to
  `terminal::ensure_terminal`. `render_terminal_body` looks up the **active**
  repo's session and renders its `view` (or a start-error / "starting…"
  placeholder). Start failures are recorded in the Operation Log
  (`op="terminal-start"`, `OpOutcome::Failed`).
- This HashMap is **the de-facto per-`RepoSession` ownership today**, just held on
  the monolithic `KagiApp` instead of on a real session struct. It is the natural
  seam for the re-architecture: move it onto `RepoSession`.

### Bottom-panel hosting (ADR-0007/0017)
- Terminal is one tab of the Bottom Panel (alongside Operation Log). Default panel
  height = **18% of viewport** (min 80px / max 60%), session-resize remembered.
- Per ADR-0017, terminal output is **not parsed**; repo state stays fresh via the
  `.git` file watcher (T029) — no focus-out / command-exit hooks. Session survives
  panel close; killed only on explicit kill or app exit.

### Why gpui-terminal is vendored (ADR-0035)
- gpui-terminal **0.1.0** (zortax, sole crates.io release) ships mouse selection
  as a **TODO stub**, and clipboard copy (cmd-c) is impossible without selection.
  User wanted selection + copy. → in-tree fork at `vendor/gpui-terminal/`,
  Cargo path dep (`Cargo.toml:14`). The fork adds selection + Cmd/Ctrl-C copy;
  changes are marked `// kagi:` in `view.rs` (selection state, copy shortcut,
  mouse-down anchor), `mouse.rs` (`Selection`, `pixel_to_cell`, word/line expand),
  and `colors.rs` (selection color). **In-tree, not submodule**, so parallel
  worktree agents and offline (codex) sandboxes build with no submodule fetch.
- License is **MIT OR Apache-2.0** (permissive) — both LICENSE files + README kept
  in vendor. cmd-v paste deliberately lives app-side (SharedWriter), not in the
  fork, to keep the fork generic/upstream-able.

---

## 2. 参考プロジェクトの実装方針

### gpui-terminal upstream (zortax) — the fork's source of truth
- crates.io 0.1.0, **6 commits, no published releases**, single author. Backend is
  `alacritty_terminal 0.25.1` for VTE parsing/grid; gpui 0.2.2 (exact match to
  Kagi). I/O is **generic `Read`/`Write`** — PTY-library-agnostic (portable-pty is
  only used in its example bin).
- Architecture (from lib.rs): `TerminalView` (gpui `Entity`, `Render`) →
  `TerminalState` (`alacritty Term` + VTE parser), `TerminalRenderer` (font
  metrics, palette, cell-batched canvas paint), and a background reader thread →
  flume channel → async task → notify-repaint pipeline. Rich callback surface:
  resize, exit, bell, title (OSC 0/2), clipboard-store (OSC 52), key-handler.
- **Still missing upstream as of 2026-01**: README + GitHub + DeepWiki all confirm
  "Mouse text selection not yet implemented", "No scrollback navigation", and
  manual copy/paste "future feature, not yet fully implemented". 1 open PR, 0
  issues — low velocity. → Kagi's fork delta is exactly these gaps.

### Zed terminal (`crates/terminal` + `crates/terminal_view`)
- The conceptual upstream of gpui-terminal's design. Same `alacritty_terminal`
  backend; `Terminal` model owns the alacritty `Term`, an `event_loop`/PTY, and
  exposes selection, scrollback, hyperlink detection, search, and task/REPL
  integration. `terminal_view` is the gpui element + input/mouse/selection UI.
- **Licensing: GPL-3.0, publish=false, depends on in-tree gpui + Zed-internal
  crates + an alacritty fork.** Per ADR-0008 this is **reference-for-design-only —
  code transcription is forbidden**. Useful only as a blueprint for how
  selection/scrollback/search/copy-on-select are structured.

### alacritty_terminal (the backend)
- Apache-2.0, UI-independent VTE engine: grid, `Term`, `Selection`,
  `SelectionType` (word/line/semantic), scrollback, and its own tty + event_loop
  (so portable-pty is unnecessary if used directly). This is what both Zed and
  gpui-terminal wrap. Direct use is the "method 3" fallback in ADR-0008 (proven by
  Zed and cosmic-term/Apache) but requires writing the gpui renderer, input
  translation, and clipboard ourselves — M–L effort.

---

## 3. 採用すべき設計

### 3.1 Session ownership: move `terminal_sessions` onto `RepoSession`
- The terminal session is **repo-scoped state**: cwd, PTY, scrollback all belong
  to one repository. Today it lives in `KagiApp.terminal_sessions: HashMap<PathBuf,
  …>` — a workaround for the monolith. In the new model **each `RepoSession` owns
  exactly one `TerminalSession`** (`Option<TerminalSession>`, lazy). The HashMap
  disappears: lookup becomes `self.active_repo().terminal` instead of
  `terminal_sessions.get(repo_path)`.
- The app layer owns the PTY + session; the ui layer (bottom-panel terminal tab)
  only **renders the session's view entity and forwards focus/paste**. This matches
  the target layering and keeps UI free of PTY/git concerns.
- `ensure_terminal`, `build_terminal_view`, `SharedWriter`, `pick_font_family`,
  `resolve_shell` move into an **app-side terminal module** (e.g. `app::terminal`),
  not `ui`. `build_color_palette`/`build_terminal_config` straddle the boundary
  (need theme) — keep palette mapping in app, fed by a theme snapshot the app
  already holds.

### 3.2 Thin port/trait so the vendored fork is swappable (key invariant)
- Define a minimal **`TerminalBackend` port** in the app layer that the rest of
  Kagi depends on — never `gpui_terminal::TerminalView` directly outside one
  adapter module. Surface area (driven by what `terminal.rs` actually uses):
  - construct from `(writer, reader, config)` → a renderable handle
  - `with_resize_callback`, `with_exit_callback` (and optionally bell/title/
    clipboard-store)
  - `update_config(TerminalConfig)` for live theme switch
  - `focus_handle()`
  - copy-selection / has-selection (the fork's `// kagi:` additions)
- Provide one adapter `GpuiTerminalBackend` that wraps the vendored
  `TerminalView`. Because gpui-terminal already accepts **generic Read/Write**, the
  port stays PTY-agnostic too: portable-pty sits behind the same boundary and can
  be replaced (e.g. alacritty's own tty) without touching ui.
- Kagi-neutral config types: define Kagi's own `TerminalConfig`/`ColorPalette`-
  shaped structs (or re-export) so a future swap to alacritty_terminal-direct
  (ADR-0008 method 3) or upstream gpui-terminal needs only a new adapter, not a
  ui rewrite. This is the explicit ADR-0035 exit path.

### 3.3 Theme integration
- Keep the W9-THEME pattern: a single `build_terminal_config()` is the source of
  truth, reused both at session start and on theme switch (`update_config`). On a
  theme change the app iterates live `RepoSession` terminals and pushes the new
  config. Selection color stays translucent (W8-TERMSEL) so glyphs stay readable.
- Theme values (`term_*`) stay in the theme module; the app maps them into the
  port's palette type — ui never builds palettes.

### 3.4 Upstreaming path (ADR-0035 exit condition)
- Keep fork changes **isolated and `// kagi:`-tagged** (already done) so the
  selection+copy delta can be submitted as an upstream PR. When upstream publishes
  selection/copy, drop `vendor/` and return to a crates.io version dep — the port
  in §3.2 makes this a one-adapter change. Track the upstream repo's PR/releases.

---

## 4. 採用しない設計

- **Zed `crates/terminal` code reuse** — GPL-3.0 + publish=false + in-tree gpui /
  Zed-internal crates / alacritty fork. Design reference only; do not transcribe
  (ADR-0008/0035).
- **alacritty_terminal-direct (method 3) now** — viable long-term fallback but
  M–L effort (must hand-write gpui renderer, input translation, clipboard). Not
  worth it while the small fork delta covers the requirement. Keep it as the
  documented Plan B behind the port.
- **git submodule for the vendor** — rejected by ADR-0035 (breaks parallel
  worktree lanes and offline sandboxes). Keep in-tree.
- **Parsing terminal output to refresh repo state** — rejected by ADR-0017; the
  `.git` watcher (T029) is the source of truth. No focus-out/command-exit hooks.
- **Putting cmd-v paste inside the fork** — keep it app-side (SharedWriter) so the
  fork stays generic and upstream-able.
- **Multiple terminals / split panes per repo** — out of scope; one PTY per
  `RepoSession` (matches current HashMap-of-one semantics).

---

## 5. リスク (maintaining a fork)

- **Drift from upstream**: upstream is low-velocity (6 commits, 1 PR, no
  releases), so drift risk is currently low, but any future upstream refactor
  forces a manual re-merge of the `// kagi:` selection/copy delta. Mitigation:
  keep the delta minimal and tagged; pin to a known upstream commit
  (`.cargo_vcs_info.json` is preserved).
- **Single-author dependency**: if upstream is abandoned, Kagi inherits full
  maintenance of an alacritty_terminal wrapper. Mitigation: the port (§3.2) lets us
  fall back to alacritty-direct without a ui rewrite.
- **gpui version coupling**: fork pins `gpui = "0.2.2"`. A Kagi gpui bump (Kagi
  also uses the runtime_shaders requirement, per memory) must be matched in the
  vendor's Cargo.toml — the in-tree fork makes this a local edit but it is a
  coupling point.
- **alacritty_terminal 0.25.1 pin** in the vendor — security/bug fixes require
  bumping inside the fork.
- **Cargo.toml "no-change" convention exception**: the path-dep switch is a logged
  exception (ADR-0035); re-architecture must preserve that provenance note.
- **Edition 2024** in the vendor crate — toolchain MSRV coupling.

## 6. 未解決事項

- **Can we drop the fork?** Only once upstream ships mouse selection + manual copy
  (still unimplemented as of 2026-01). Needs an upstream watch; revisit at v1.0
  cut. The port should be built now so the drop is cheap whenever it lands.
- **Will upstream accept our selection/copy PR?** Unknown (1 existing open PR,
  unresponsive cadence). If not, the fork is effectively permanent → the
  alacritty-direct fallback gains weight.
- **Scrollback navigation** — still missing both upstream and (apparently) in the
  fork; is it a v1.0 requirement? If yes, that is a second fork delta or a push
  toward method 3.
- **Should the PTY backend itself sit behind the port?** Leaning yes (gpui-terminal
  is already Read/Write-generic), so portable-pty vs alacritty-tty is a swap — but
  confirm the resize callback / `MasterPty` sharing model survives the abstraction.
- **Per-`RepoSession` terminal lifecycle on repo close** — today sessions live in a
  HashMap until app exit; once owned by `RepoSession`, closing a repo should kill
  its PTY. Confirm the desired semantics (kill vs detach) with PM.
- **Theme snapshot ownership** — where the app gets the theme to build the palette
  once `KagiApp` is decomposed (theme is global today). Needs to be a value the app
  layer can read without reaching into ui.
