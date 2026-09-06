# ADR-0180: modal の plan/replan 失敗を明示状態にする

状態: 採用（#510）
日付: 2026-09-07
関連: #510、#559（confirm dispatch 統一）、[ADR-0176](0176-app-stash-local-boundary.md)、
[ADR-0175](0175-app-remove-boundary.md)、[DESIGN](../rearch/app-layer/DESIGN.md) §3

## 問題

`RepoSession` に同期 plan する modal（create-branch / create-tag / create-worktree /
set-upstream / rename-branch）は `plan: Option<Arc<OperationPlan>>` と
`error: Option<SharedString>` を独立に持っていた。replan が Err のとき klog だけを出し、
**旧 plan がそのまま残る**。confirm は plan の有無しか見ないので、入力に対応しない plan を
Enter でもボタンでも実行できる状態だった。checkout / cherry-pick / revert の plan 失敗は
modal を開かずに klog で終わり、利用者には無反応に見えた。

## 決定

1. `src/app/flow.rs` に純粋な `PlanSlot<P>`（`Pending` / `Ready(P)` / `Failed(String)`）を置く。
   `replan(Result<P, _>)` は結果を**丸ごと入れ替える**。失敗は再計算対象だった plan を捨てる。
   `plan()` は `Ready` でだけ `Some` を返し、これが confirm 経路の唯一の入口になる。
   `PlanState` と同じ語彙だが token を持たない段で、Sessions に移行した family は使わない。
   単体テストは `src/app/flow.rs` 内（失敗が旧 plan を無効化 / 再試行で復帰 / Pending は実行不可）。
2. 上記 5 modal の `plan` を `ModalPlan = PlanSlot<Arc<OperationPlan>>`（`src/ui/modal_plan.rs`）に
   置き換える。`error` は execute/preflight 失敗専用として残し、plan 失敗とは同居しない。
   描画は `plan_or_exec_error` で 1 行に集約する。plan が無ければ `has_blockers` が真になり、
   confirm ボタンは描画されない。#559 で Enter とボタンは同じ `confirm_*` に入るので、
   `plan()` が `None` を返す時点で両方が拒否される。再 plan 成功で `Ready` に戻り、再試行できる。
3. plan 失敗文言は `i18n::op_plan_failed`（EN/JA 既存）に統一する。
   `Set upstream plan error:` の英語直書きは撤去。
4. modal を開く前に失敗する checkout / checkout-commit / cherry-pick / revert は
   `report_plan_failure` で footer + 共通 app-notice modal に出す。

## 保証しないもの

`[kagi]` 契約行は文言・順序とも不変。plan 失敗の oplog 記録は stash family
（`record_stash_plan_error`）のみで、checkout/cherry-pick/revert 側は未記録のまま
（`open_*` は `cx` を持たず、family ごとの recording 入口も無い）。
`sync_modal_inputs` は入力変更で plan を `Pending` に戻さない。debounce 中の表示ちらつきを
避けるためで、confirm は必ず `run_modal_replans` を先に通すので実行はできない。

## 検証

`cargo build` / `cargo test --workspace` / `cargo clippy --workspace`（新規警告なし）/
`uv run --project ci check-all` / `cargo build -p kagi --features gui-e2e --tests`。
GUI E2E は既存 `stash_replan_error` の兄弟として `create_branch_replan_error` を追加。
Ready な plan の confirm bounds を取ってから repo を移動し、Enter と旧ボタン座標の双方で
branch が作られないこと、slot が `Failed` になること、repo を戻した再試行が成功することを見る。
