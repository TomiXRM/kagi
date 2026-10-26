- Deliverable / evidence: [examples/lab_i04_s1.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i04_s1.rs). Commands run: `rustfmt --edition 2021 examples/lab_i04_s1.rs`; `cargo build --example lab_i04_s1`; `target/debug/examples/lab_i04_s1 --list`; `rustfmt --check --edition 2021 examples/lab_i04_s1.rs`; `git diff --check -- examples/lab_i04_s1.rs`. Intended PNG directory: `/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i04/s1/`; no PNG was written because the gallery host rejected the window as occluded on display `ad1ab626`.

- Requirement self-check:
  1. × Apple Dark/Light host capture: build succeeds, but the Apple Dark default capture stopped with `window … is not visible (occlusionState 8192)`; Light was not attempted to avoid repeating the same stale-frame-safe failure.
  2. ○ Kit reuse: all standard settings controls use `kagi::ui::settings_kit`.
  3. ○ The specified sections, rows, subtitles, navigation states, and final off switch are present.
  4. ○ `position_picker(selected, forced)` is a reusable page-local function; states include `rest`, `selected`, and `focus`, using only `pt`, `ink`, and `ring` for its geometry and colour.
  5. Partial: rows/groups use kit geometry and trailing alignment by construction; visual capture confirmation is blocked.
  6. Partial: named forced states are implemented (`focus`, `hover-switch`, `focus-nav`), but visibly different PNGs could not be captured.
  7. Partial: commands and the capture blocker are reported, but there are no PNG paths to provide.

- Unclear points:
  - [Execution] Gallery capture refused the target window / macOS reports `occlusionState 8192` despite the specified display / Make the target display’s Space visible without activating the gallery, then rerun captures for both themes and all states.

- Discretionary fills: included Rex’s visible category set including Shortcuts; used the required Accent tile for selected Notifications; `selected` changes the picker to bottom-left and updates its subtitle consistently.

- Kit gaps: `position_picker`, a 3×2 visual notification-position selector with an in-cell checkmark and forced focus ring.

- Retries: 1 — the first build found that picker radio cells needed stable IDs before GPUI could assign their accessibility roles; corrected and rebuilt successfully.

- Not observed: real native-window hover, keyboard focus traversal, VoiceOver, and light/dark screenshot appearance due to the occlusion-safe capture failure.