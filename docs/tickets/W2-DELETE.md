# W2-DELETE: branch delete(plan 経由、merged のみ)+ sidebar からの起動

- Status: in-progress
- 担当: worktree agent
- 関連 ADR: 0014

## 背景

Repository Navigator の context menu 要件のうち delete は backend がないため未実装だった。
安全方針: **merged 済み local branch のみ削除可**。unmerged は blocker(force delete は提供しない)。

## スコープ

1. **backend** `src/git/ops.rs`:
   - `plan_delete_branch(repo, name) -> Result<OperationPlan, GitError>`
     - blockers: 対象が存在しない / 現在 checkout 中の branch / HEAD detached で対象が HEAD /
       **unmerged**(`repo.graph_descendant_of(head_or_upstream, branch_tip)` で HEAD から到達不能なら
       unmerged 扱い。判定は「branch tip が HEAD の祖先か」= `graph_descendant_of(head, tip) || head==tip`)
     - warnings: upstream が設定されている場合「remote branch は削除されない」
     - predicted: `delete branch '<name>' (tip <short_sha>)`、recovery: `git branch <name> <sha>` を明記
   - `execute_delete_branch(repo, plan, name)`: preflight(HEAD/stash 数が plan 時と同一)→
     `Branch::delete()`(git2、working tree に触らない)→ verify(branch が消えたこと)
   - oplog 記録は UI 側(既存 confirm_* と同様)
2. **tests** `tests/delete_branch_test.rs`(新規):
   - merged branch 削除成功 / unmerged は blocker / current branch は blocker /
     存在しない branch は Err or blocker / 削除後の recovery 文字列に sha が入る / preflight 不一致で Refused
   - テスト repo は tempdir + `git init -b main`(isolated env は default master のため)
3. **UI** `src/ui/sidebar.rs` + `src/ui/mod.rs`(最小限):
   - local branch 行に hover 時のみ表示される小さな `✕`(右端、current branch には出さない)
   - クリック → `open_delete_branch_modal(name)` → 既存 plan modal カード(render_plan_modal_card)で
     blockers/warnings/recovery 表示 → Confirm で execute → reload + oplog + footer
   - headless: `KAGI_DELETE_BRANCH=<name>` で plan ログ `[kagi] plan: delete-branch <name> blockers=N`、
     `KAGI_AUTO_CONFIRM=1`(test-only)で実行ログ `[kagi] executed: delete-branch <name>`
4. 本物の context menu(右クリック)は次フェーズ。今回は ✕ ボタンで起動経路を作る

## 完了条件

- [ ] `cargo test` 全パス(新 suite 含む)+ own-code warning 0
- [ ] fixture headless: merged branch の delete plan→実行、unmerged で Refused
- [ ] 既存 headless 検証に回帰なし
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/git/ops.rs` / `src/git/mod.rs`(re-export のみ)/ `tests/delete_branch_test.rs`(新規)
- `src/ui/sidebar.rs` / `src/ui/mod.rs`(最小限)/ `src/main.rs`(KAGI_DELETE_BRANCH のみ)
- `docs/tickets/W2-DELETE.md`

## 触ってはいけないファイル

- `src/graph/` / `scripts/*` / `Cargo.toml` / 他の docs / 他の tests

## テスト方法

1. `cargo test`(パイプで握りつぶさず exit code 確認)
2. fixture(feature/two は main に merge 済み=削除可、feature/one は unmerged)で headless 検証
3. 検証は fixture / tempdir のみ。ユーザー repo 禁止

## リスク

- **force delete / reset --hard / clean は絶対に追加しない**(コードベース全体の安全規約)
- `Branch::delete` は ref 削除のみで working tree に触れない(安全)が、plan 時と HEAD が変わって
  いたら preflight で Refused にする
- mod.rs の変更は最小限にし、変更点を完了報告で全列挙(PM が merge する)
