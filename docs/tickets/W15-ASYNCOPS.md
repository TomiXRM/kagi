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

## 実装メモ(W15-ASYNCOPS 完了)

### 設計方針

- **pull/push/stash の async パターンを踏襲**: blocking core(free fn・`&mut KagiApp`
  非依存・repo は core 内 open)を `cx.background_spawn` で実行し、`cx.spawn` の finish
  で main thread に戻して record_op + footer + reload + busy 解除。
- **`start_*`(UI・async)/ `confirm_*`(headless・sync)の分離**を踏襲。UI のボタン
  handler を `confirm_*` → `start_*` に差し替え。ref-order 規則・in-memory 主義・
  execute ロジックは一切変更せず、スレッド移動と core 抽出のみ。

### BUG-1 — background 化した操作と blocking core

| 操作 | blocking core(`src/ui/mod.rs`) | UI entry | headless(不変) |
|------|------------------------------|----------|----------------|
| checkout(branch/commit) | `checkout_blocking` | `start_checkout` | `confirm_checkout`(sync・main.rs が使用) |
| cherry-pick | `cherry_pick_blocking` | `start_cherry_pick` | main.rs は `execute_cherry_pick` 直呼び |
| revert | `revert_blocking` | `start_revert` | main.rs は `execute_revert` 直呼び |
| commit | `commit_blocking` | `start_commit` | main.rs は `execute_commit` 直呼び |
| stash pop | `stash_pop_blocking` | `start_pop` | `confirm_pop`(sync・main.rs が使用) |
| amend | `amend_blocking` | `start_amend`(armed は main thread・最終 execute のみ bg) | `confirm_amend`(sync・main.rs が使用) |
| delete branch | `delete_branch_blocking` | `start_delete_branch` | `confirm_delete_branch`(sync・main.rs が使用) |
| create worktree | `create_worktree_blocking` | `start_create_worktree` | main.rs は `execute_create_worktree` 直呼び |

- 共通の verify ヘルパ `verify_new_commit_snapshot`(cherry-pick / revert)を追加。
  各 blocking core は元の sync 版と同一の `[kagi] executed:` / `[kagi] verified:`
  ログ文言を出力(回帰網の文言保持)。
- busy_op の値は操作別("checkout"/"cherry-pick"/"revert"/"commit"/"stash-pop"/
  "amend"/"delete-branch"/"create-worktree")。toolbar/menu disabled は既存の
  `busy_op.is_some()` 連動でそのまま効く。
- **同期のまま残した操作と理由**:
  - **create branch**: ref 作成のみで tree サイズ非依存(軽量)。チケット明記どおり
    同期維持(`start_*` 追加なし)。
  - **undo-commit**: `execute_undo_commit` は ref-only soft reset(監査でも data 安全と
    確認済み)で tree 非依存・軽量。UI handler は `confirm_undo` のまま。
  - **stash apply**: pop と異なりエントリ削除を伴わず、UI 動線も pop 経由が主。今回の
    スコープ列挙の中核(checkout/commit/pop/cherry-pick/revert/amend/delete/worktree)を
    優先し、apply は同期維持(次段で揃える余地あり・リスク低)。
- **削除した dead な sync メソッド**: `confirm_commit` / `confirm_cherry_pick` /
  `confirm_revert` / `confirm_create_worktree` は headless が execute_* 直呼びのため
  呼び出し元が消え dead に。own-code warning 0 のため削除(headless 経路は execute_*
  直呼びで不変)。`confirm_checkout` / `confirm_pop` / `confirm_amend` /
  `confirm_delete_branch` は main.rs headless がなお使うため温存。

### BUG-2 — checkout-commit dirty 重なりの blocker 化

- `src/git/ops.rs` `plan_checkout_commit` に in-memory dry-run
  `predict_checkout_commit_conflict`(`predict_stash_pop_conflict` と同型・WT/HEAD 非変更)
  を追加。HEAD-tree → target-tree の diff が触る path 集合と、status の staged+unstaged
  (tracked のみ・untracked 除外)の交差があれば warning を **blocker** に昇格。重なりが
  なければ従来どおり warning のまま(非重なり dirty は safe checkout 成功するため)。
- テスト: `tests/qa_audit_test.rs` に `checkout_commit_overlapping_dirty_plan_blocks` 追加、
  既存 `checkout_commit_dirty_plan_warns_but_does_not_block` のコメントを「非重なりは warning」
  に更新。`tests/ops_test.rs::test_checkout_commit_dirty_safe_checkout_fails_without_moving_head`
  は旧バグ挙動(blockers 空)を pin していたため、重なり dirty が blocker を返す新契約に更新
  (execute 失敗 + ローカル編集保持の data-safety アサートは維持)。

### mod.rs 変更の全列挙

- 追加 free fn(blocking cores + verify helper): `checkout_blocking` / `cherry_pick_blocking`
  / `revert_blocking` / `commit_blocking` / `stash_pop_blocking` / `amend_blocking`
  / `delete_branch_blocking` / `create_worktree_blocking` / `verify_new_commit_snapshot`。
- 追加 KagiApp メソッド(UI async entry): `start_checkout` / `start_cherry_pick`
  / `start_revert` / `start_create_worktree` / `start_amend` / `start_pop`
  / `start_delete_branch` / `start_commit`。
- 削除 KagiApp メソッド: `confirm_commit` / `confirm_cherry_pick` / `confirm_revert`
  / `confirm_create_worktree`(上記理由)。
- render handler 差し替え(8 箇所): checkout / amend / pop / delete-branch / revert /
  create-worktree / cherry-pick / commit の confirm ボタンを `start_*(cx)` に変更。

### テスト・実測結果

- 全 24 integration suites + lib(58)+ main(78)unit + qa_audit_test(11)= **全パス**、
  own-code warning 0。
- 50MB 級実測: 90MB(30MB×3)tree の fixture で headless checkout-commit を実行。
  libgit2 `execute_checkout_commit`(=`checkout_tree` の重い WT 書き込み)が plan→executed
  間で **~458ms**。これが旧 UI 同期経路では窓フリーズだった部分で、新経路では
  `cx.background_spawn` 内で実行され Busy footer + toast 付きで UI 無応答にならない
  (pull/push/stash と同一機構)。

### 未解決リスク

- stash apply は今回同期のまま(スコープ優先)。大 untracked tree で UI 体感がありうるが、
  pop と同 core 化で次段に揃えられる(リスク低)。
- GUI イベントループ下の応答性は headless では実測できないため、構造的保証
  (`cx.background_spawn` 委譲・既存 busy_op ガード)に依拠。pull/push/stash で実証済みの
  同一パターン。
