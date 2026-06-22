# T-DECOMP-001 — Consolidate the avatar cache into `AvatarStore`

- ADR: 0118 (KagiApp decomposition, Phase 5.2) — Mechanism A, proving run
- Risk: low (pure data-cluster consolidation; compiler-checked; no behaviour change)
- Owner: SubAgent (general-purpose) → PM verify + cross-review

## Goal

Move the two flat avatar fields off `KagiApp` into one cohesive `AvatarStore` sub-struct.
**Behaviour-preserving** — no change to avatar fetching, rendering, timing, `[kagi]` lines, or i18n.
This is a field-grouping refactor only; the compiler must catch every missed access site.

## Current state (`src/ui/mod.rs`)

```rust
pub avatar_images: HashMap<String, std::sync::Arc<gpui::Image>>,   // ~:1138
pub avatar_fetch_for: Option<PathBuf>,                              // ~:1142
```

Owning access sites (the only ones that change — helper fns in `inspector.rs` /
`render_helpers.rs` take the map *by reference as a parameter*, so only their **call sites**
change, not their signatures):
- `src/ui/mod.rs`: inits ~1551/1552 and ~1661/1662; `ensure_avatars` ~2366/2369/2400.
- `src/ui/render_body.rs`: ~52 (`self.avatar_images.clone()`), ~330 (`&this.avatar_images`), ~480.

## Tasks

1. In **`src/ui/avatar.rs`** (the existing avatar module) add:
   ```rust
   #[derive(Default)]
   pub struct AvatarStore {
       /// Resolved avatar images keyed by author email (memory cache; disk cache
       /// lives under ~/.kagi/avatars/).
       pub images: std::collections::HashMap<String, std::sync::Arc<gpui::Image>>,
       /// Guard so avatar resolution runs at most once per repo path.
       pub fetch_for: Option<std::path::PathBuf>,
   }
   ```
   Move the existing doc comments from the two `KagiApp` fields onto these.
2. Replace the two `KagiApp` fields with a single `pub avatars: avatar::AvatarStore,` (keep the
   `// ── W11-AVATAR …` section comment). Update **both** struct initialisers to
   `avatars: avatar::AvatarStore::default(),` (drop the two old init lines).
3. Update every owning access site: `self.avatar_images` → `self.avatars.images`,
   `self.avatar_fetch_for` → `self.avatars.fetch_for` (and the `this.`/`app.` variants). Helper
   call sites pass `&self.avatars.images` where they passed `&self.avatar_images`.
4. Do **not** change `inspector.rs` / `render_helpers.rs` function *signatures* (they keep taking
   `avatar_images: &HashMap<…>`); only their callers change.

## Constraints (from CLAUDE.md)

- No `git2::` in `src/ui/` (unaffected here). No `.unwrap()` added.
- Do not touch any `[kagi]` / `klog!` line. No new clippy warnings; run `cargo fmt --all`.
- Keep the diff minimal — this is a mechanical move, not a redesign.

## Done = all green

- `cargo build`
- `cargo test --workspace` (expect 791 passed, 0 failed)
- `cargo fmt --check`
- `cargo clippy --bin kagi` adds no new warning in `avatar.rs` / changed sites
- Report exactly which files/sites changed.
