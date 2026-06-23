# T-DECOMP-002 — Consolidate the changed-files/diff caches into `DiffCaches`

- ADR: 0118 (KagiApp decomposition, Phase 5.2) — Mechanism A (sub-struct consolidation)
- Risk: low-medium (pure data-cluster consolidation; compiler-checked) **+ one deliberate,
  documented behaviour fix** (see §"Behaviour delta")
- Owner: SubAgent (general-purpose) → PM verify + codex cross-review
- Follows: T-DECOMP-001 (`AvatarStore`, same mechanism, landed dcdc7a1)

## Goal

Group the five flat row-keyed diff/changed-files cache fields off `KagiApp` into one cohesive
`DiffCaches` sub-struct with a single `clear()` method, and replace the **four** hand-maintained
invalidation blocks with one `self.diff_caches.clear()` call each. This removes the "forgot to clear
one of the N cache fields" bug class (CLAUDE.md state-update rules; ADR-0118 Consequences).

## Current state (`src/ui/mod.rs:850-867`)

```rust
pub diff_cache: HashMap<usize, Option<Vec<FileStatus>>>,                 // changed-files list per row
pub file_diff_cache: HashMap<(usize, usize), std::sync::Arc<FileDiff>>,  // per (row, file) FileDiff content
pub remote_diff_inflight: std::collections::HashSet<usize>,             // remote changed-files load in flight
pub local_diff_inflight: std::collections::HashSet<usize>,              // local changed-files+diffstat in flight
pub diffstat_cache: HashMap<usize, Vec<FileDiffStat>>,                  // per-row diffstat
```

`FileStatus`, `FileDiff`, `FileDiffStat` are re-exported from `kagi_git` (see `mod.rs:334`).

### Out of scope (do NOT fold in — ADR-0118 explicitly defers these)

`wip_diffstat`, `main_diff`, `compare_view` sit adjacent in the same reset blocks but are **not**
row-keyed caches: `wip_diffstat` is set to different values per site (`Some(wip)` vs `None`),
`main_diff`/`compare_view` are the deferred "view cluster". Leave them exactly as they are —
they stay as direct `self.<field>` assignments next to the new `self.diff_caches.clear()` call.

## The four invalidation sites (today)

| Site | Location | Clears today | After |
|---|---|---|---|
| `reload`            | `mod.rs:1717-1721` | all 5 | `self.diff_caches.clear()` |
| `reload_external`   | `mod.rs:1973-1977` | all 5 | `app.diff_caches.clear()` |
| `reset_per_repo_ui` | `tabs.rs:333-336`  | **4** (missing `diffstat_cache`) | `self.diff_caches.clear()` |
| `show_welcome`      | `tabs.rs:504-507`  | **4** (missing `diffstat_cache`) | `self.diff_caches.clear()` |

### Behaviour delta (intended, must be called out in the PR)

`reset_per_repo_ui` (tab switch / cached swap) and `show_welcome` currently clear only 4 of the 5
caches — `diffstat_cache` is left populated, so a stale per-row diffstat from the previous repo can
survive a tab switch / return-to-welcome. Routing all four sites through `DiffCaches::clear()` makes
them clear `diffstat` too. **This is the bug-class fix the ADR calls for, not a regression** — but it
*is* a behaviour change, so flag it explicitly and confirm no `[kagi]` contract line or test asserts
the old (stale-diffstat-retained) behaviour.

## Tasks

1. **New module `src/ui/diff_cache.rs`** (mirror `src/ui/avatar.rs`'s `AvatarStore` layout):
   ```rust
   use kagi_git::{FileDiff, FileDiffStat, FileStatus};
   use std::collections::{HashMap, HashSet};
   use std::sync::Arc;

   /// Cohesive per-row diff / changed-files cache cluster (ADR-0118 Phase 5.2).
   /// Read inside `KagiApp::render`; deliberately NOT an `Entity` (no notify-scope
   /// to isolate — see ADR-0118 Mechanism A). Invalidated as a unit via `clear()`.
   #[derive(Default)]
   pub struct DiffCaches {
       /// Changed-files list per commit row (`None` = load attempted but failed). (was `diff_cache`)
       pub changed_files: HashMap<usize, Option<Vec<FileStatus>>>,
       /// Per-(row, file-index) `FileDiff` content cache (T-REARCH-031). (was `file_diff_cache`)
       pub file_content: HashMap<(usize, usize), Arc<FileDiff>>,
       /// Rows whose REMOTE changed-files load is in flight (dedupe). (was `remote_diff_inflight`)
       pub remote_inflight: HashSet<usize>,
       /// Rows whose LOCAL changed-files+diffstat load is in flight (dedupe). (was `local_diff_inflight`)
       pub local_inflight: HashSet<usize>,
       /// Per-row diffstat for the Inspector changed-files list (W16-DIFFSTAT). (was `diffstat_cache`)
       pub diffstat: HashMap<usize, Vec<FileDiffStat>>,
   }

   impl DiffCaches {
       /// Drop every cached diff/changed-files entry as one unit. Single
       /// invalidation point for `reload` / `reload_external` /
       /// `reset_per_repo_ui` / `show_welcome` so no field can be forgotten.
       pub fn clear(&mut self) {
           self.changed_files.clear();
           self.file_content.clear();
           self.remote_inflight.clear();
           self.local_inflight.clear();
           self.diffstat.clear();
       }
   }
   ```
   Move the existing rich doc comments from the five `KagiApp` fields onto the new fields
   (don't lose the T-REARCH-031 / ADR-0089 / W16-DIFFSTAT context — condense if needed).

2. **Declare the module** in `src/ui/mod.rs` next to `pub mod avatar;` (`mod diff_cache;` —
   private is fine; expose `pub use diff_cache::DiffCaches;` only if a sibling needs it, else keep
   it module-private and reference as `diff_cache::DiffCaches`).

3. **Replace the five `KagiApp` fields** (`mod.rs:850-867`) with one
   `pub diff_caches: diff_cache::DiffCaches,` (keep the surrounding section comment). Update **both**
   struct initialisers (`mod.rs:1465-1469` and `1572-1576`) to
   `diff_caches: diff_cache::DiffCaches::default(),` (drop the ten old init lines).

4. **Rewrite the four invalidation blocks** to a single `self.diff_caches.clear()` /
   `app.diff_caches.clear()` (see table above), leaving the adjacent `wip_diffstat` / `main_diff` /
   `compare_view` lines untouched.

5. **Update every remaining access site** (compiler-checked, ~40 sites in `mod.rs` + `render.rs`):
   - `self.diff_cache` → `self.diff_caches.changed_files` (and `app.`/`this.` variants)
   - `self.file_diff_cache` → `self.diff_caches.file_content`
   - `self.remote_diff_inflight` → `self.diff_caches.remote_inflight`
   - `self.local_diff_inflight` → `self.diff_caches.local_inflight`
   - `self.diffstat_cache` → `self.diff_caches.diffstat`
   Cover `.contains_key` / `.get` / `.insert` / `.remove` / `.contains` call sites in `render.rs`
   (lines ~277/286/298/301) and `mod.rs` (3171–4061).

## Constraints (from CLAUDE.md)

- No `git2::` in `src/ui/` (unaffected). No new `.unwrap()`.
- Do **not** touch any `[kagi]` / `klog!` line. No new clippy warnings; run `cargo fmt --all`.
- `kagi-domain` purity / layering unaffected (this is all `src/ui/`).
- Keep the diff focused: a field-move + one centralised `clear()`, not a redesign of the diff flow.

## Done = all green

- `cargo build`
- `cargo test --workspace` (was 791 passed pre-001; confirm the count, 0 failed)
- `cargo fmt --check`
- `cargo clippy --bin kagi` — no new warning in `diff_cache.rs` / changed sites
- Report exactly which files/sites changed, and **call out the diffstat-clear behaviour delta**.
