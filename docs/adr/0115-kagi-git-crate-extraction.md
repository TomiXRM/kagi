# ADR-0115: Extract the `kagi-git` Crate (Phase E, step 1)

- Status: Accepted / Date: 2026-06-21
- Context: Realizes the second layer of the workspace split planned in ADR-0072
  ("Workspace Crate Split and git2 Confinement") and the Phase E line item in
  `docs/architecture-cleanup-roadmap.md`. `kagi-domain` was extracted first
  (ADR-0072 / ADR-0108); this ADR moves the Git backend out of the `kagi` binary.

## Decision

Move the whole `src/git/` tree into a new standalone workspace crate
`crates/kagi-git` (package `kagi-git`, imported as `kagi_git`):

- `src/git/mod.rs` becomes `crates/kagi-git/src/lib.rs` (same submodule tree,
  same public re-exports). Files are moved with `git mv` so history is preserved.
- `kagi-git`'s only dependencies are `kagi-domain` (model types it re-exports),
  `git2`, `ureq` (commit-message generation HTTP), and `tempfile` (request
  staging + tests). No `gpui`, no UI.
- Call sites move from `kagi::git::` / `crate::git::` to `kagi_git::`
  (312 sites in `src/`, 31 test files, plus the lib's `src/remote` module).
- `src/lib.rs` drops `pub mod git;`; the binary keeps `git2` only for the
  bootstrap snapshot in `main.rs` (the allowed shell layer).

## なぜ

ADR-0072 の目標は「`git2` が存在するクレートを一点に閉じ込め、UI からの
git2 直呼びをコンパイルエラーにする」こと。`kagi-git` を独立クレート化すると、
Git バックエンドは単体でビルド・テストでき(90 ユニットテストがGUIリンク無しで
走る)、依存関係が `Cargo.toml` 上で明示される。これは将来 `kagi-ui` を
git2 非依存クレートとして切り出すための前提(roadmap Phase E の残り)。

現時点では UI はまだ `kagi`(bin)内のモジュールなので、git2 排除の強制は
従来どおり CI grep gate(ADR-0078)が担う。クレート境界によるコンパイル時
強制は `kagi-ui` 切り出し時に完成する。

## 代替案

1. **再エクスポートのシム** — `src/lib.rs` に `pub use kagi_git as git;` を置けば
   呼び出し側を一切変更せずに済む。却下: 依存を `kagi::git` 名義で隠してしまい、
   ADR-0072 の「クレート境界を明示する」意図に反する。直接 `kagi_git::` を使う。
2. **現状維持(同一クレート内モジュール)** — ADR-0072 で却下済み。

## Consequences

- `cargo test --workspace`: 784 passed / 0 failed。
- CI grep gate(`grep -rnE 'git2::|Repository::open' src/ui/`)は 0 件のまま。
  gate のヒント文言を `kagi_git::Backend` に更新。
- `tests/push_test.rs` の安全 grep テストが読むソースパスを
  `crates/kagi-git/src/ops/pull_push.rs` に更新。
- 既存の clippy 警告は `src/git/` から持ち越し(挙動変更なし)。advisory のまま。
- 残: `kagi-app` / `kagi-ui` の切り出し(Phase E の続き)。
