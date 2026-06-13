# T-CONFLICT-001: ConflictSession 検出 backend(state + index conflicts)

- Status: backend-done(W26-CONFLICT-CORE。UI レーンは後続)
- Phase: MVP
- 仕様の正: requirements-conflict-ux.md + ADR-0056〜0061 + research/conflict-ux-*.md

## スコープ

`src/git/conflicts.rs`(新規): Repository::state() + Index::conflicts() から ConflictSession を構築(op 種別、files、kind 分類)。unit test(fixture で merge/cherry-pick conflict 再現)

## 規約

- plan→confirm→preflight→execute→verify→oplog。in-memory 主義(repo を汚さない)
- chars() ベース・バイトスライス禁止。theme() 経由。i18n は Msg 経由。fixture のみで検証

## 実装メモ(backend-done)

- `src/git/conflicts.rs::detect_conflict_session(repo) -> Option<ConflictSession>`。
  `Repository::state()` で op 種別を判定し、`Index::conflicts()` で衝突 path を列挙。
- `ConflictOp = Merge | Rebase{step,total,commit,..} | CherryPick{source,..} | Revert{source,..}`。
  rebase の step/total は `.git/rebase-merge/{msgnum,end}`、source sha+summary は
  `MERGE_HEAD`/`CHERRY_PICK_HEAD`/`REVERT_HEAD`/`rebase-merge/stopped-sha` から best-effort 読取。
- file kind 分類 `ConflictKind = Content | RenameDelete | ModifyDelete | Binary`:
  stage 1/2/3 の presence パターン + blob binary プローブ(`is_binary` + NUL ヒューリスティック)。
- pure データ・UI-free。`src/git/mod.rs` で `pub use` 再エクスポート(`#[allow(dead_code)]` 不使用)。
- 検証: `tests/conflicts_test.rs`(merge/cherry-pick/modify-delete/binary fixture)+ lib unit。
  `cargo test --workspace` green。
