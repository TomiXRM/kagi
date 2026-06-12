# T-DISCARD-003: Discard all changes — 一覧付き modal + skipped 明示

- Status: todo
- 依存: T-DISCARD-002
- 関連: ADR-0046

## スコープ

- unstaged セクションヘッダに「Discard all」ボタン
- modal に対象ファイル一覧(件数 + list、スクロール可)。untracked/conflicted は「skipped」表示
- 対象 0 件なら disabled

## 完了条件

- [ ] 複数ファイル discard が 1 オペで実行され oplog 1 entry にまとまる
- [ ] `cargo test` 全パス、own-code warning 0
