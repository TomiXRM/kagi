# W15-ASYNCOPS: 残りの同期 git 操作の background 化 + checkout dirty 予測修正

- Status: in-progress
- 担当: worktree agent(Opus)
- 関連: docs/research/qa-audit-matrix.md(BUG-1 / BUG-2 が正)/ ADR-0030 §6 / W3(pull/push)・stash(済)の background パターン

## 背景(QA 監査の結論)

- **BUG-1(Critical UX)**: UI の confirm_* のうち pull/push/stash 以外は libgit2 を
  **UI スレッドで同期実行**。tree サイズ比例の操作(checkout / commit / pop / cherry-pick /
  revert / amend)は大 repo でフリーズ体感になる
- **BUG-2(Minor)**: `plan_checkout_commit` が dirty tree で blockers=0 を返すが、
  safe execute は重なりファイルで失敗する(予測と実行の不一致。データ損失なし)

## スコープ

1. **共通 background ヘルパ**: stash で確立したパターン
   (busy_op ガード → modal close → Busy footer + info toast → `cx.background_spawn(blocking)`
   → `cx.spawn` で finish(record_op + footer + reload + busy 解除))を関数/マクロに整理し、
   以下の UI confirm_* を順次移行:
   - **checkout(branch / commit)** / **commit(plan modal 確定)** / **stash pop** /
     **cherry-pick** / **revert** / **amend** / delete branch / create worktree
   - create branch(ref 作成のみ・軽量)は同期のままで可(理由をメモ)
2. **blocking core の抽出**: 各操作の preflight→execute→verify を free fn 化
   (pull_blocking / stash_push_blocking が実例)。**ref-order 規則・in-memory 主義は不変**
3. **headless 互換**: KAGI_* 経路は従来どおり同期(confirm_* の sync 版を温存 or 分岐。
   既存ログの文言・順序を変えない — qa_audit_test.rs / 既存 24 suites が回帰の網)
4. **BUG-2 修正**: `plan_checkout_commit`(ops.rs)に in-memory dry-run
   (`predict_stash_pop_conflict` と同型)を追加し、WT と重なる変更があれば **blocker** に。
   監査の再現手順(qa-audit-matrix.md)で blocker 化を確認、回帰テスト追加
5. busy 中の二重実行防止は既存 busy_op に乗せる(toolbar/menu の disabled も自動連動)

## 完了条件

- [ ] 上記操作すべてが Busy footer + toast 付きで background 実行(UI 無応答なし)
- [ ] 50MB 級 fixture で checkout / pop / commit が UI を塞がないことを実測
- [ ] BUG-2: dirty 重なりで plan が blocker を返す(テスト付き)
- [ ] 既存 24 suites + qa_audit_test 全パス、headless ログ回帰なし、own-code warning 0
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/mod.rs` / `src/git/ops.rs`(BUG-2)/ `tests/`(回帰テスト追加のみ)
- `docs/tickets/W15-ASYNCOPS.md`

## 触ってはいけないファイル

- `vendor/` / `scripts/*` / `Cargo.toml` / 他 docs

## リスク

- background 中の状態変化(ユーザーが別操作)→ busy_op ガード + finish 時の generation/preflight が防衛
- reload() は finish 側(main thread)で実行 — 二重 reload(watcher 併発)は冪等なので許容
- 文字列 chars() ベース / force 系コード追加禁止(全体規約)
