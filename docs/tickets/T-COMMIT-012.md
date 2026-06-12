# T-COMMIT-012: Undo Last Commit — UI 配線(Header / Commit Panel + oplog 元 sha 表示)

- Status: todo(backend は既存 T-HT-009 で充足。UI 配線のみ、主に PM)
- 依存: T-HT-009(done)/ ADR-0011 / 0041
- 関連: requirements-commit-suite.md

## 背景

Undo Last Commit の backend(`plan_undo_commit` / `execute_undo_commit`)は実装済み。要件を満たすには
UI から呼べること + oplog の undo エントリに元 sha が見えることが残差分。

## スコープ

- Header もしくは Commit Panel に「Undo last commit」を出す(blocker 時は理由表示で disabled)。
- 1段階確認(ADR-0023: undo は ref-only soft で blocker 付きのため 1段階据え置き)。
- 実行後 reload + toast。oplog の undo エントリに **元 sha**(`git reset --soft <元sha>` で復元可)を表示(Redo 代替)。

## 完了条件

- [ ] UI から undo が実行でき、変更が staged のまま残る
- [ ] blocker(pushed / merge / detached / root)時は disabled + 理由
- [ ] oplog エントリに元 sha が表示される
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/commit_panel.rs` / `src/ui/mod.rs` / `src/ui/commands.rs`
- `docs/tickets/T-COMMIT-012.md`

## 触ってはいけないファイル

- `src/git/ops.rs` の undo backend(既存、変更不要)/ `Cargo.toml`

## テスト方法

1. `cargo test`
2. UI は PM がスクリーンショット確認

## リスク・規約

- backend は変更しない(ref 付け替えのみの不変条件を壊さない)
