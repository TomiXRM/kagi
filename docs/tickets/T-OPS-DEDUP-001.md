# T-OPS-DEDUP-001: src/ui/operations/* の execute フロー重複を測って共通化する

- Status: todo
- Group: アーキテクチャ / S5 前準備
- 仕様の正: CLAUDE.md「plan → confirm → preflight → execute → verify → oplog」、migration README S5

## 背景(調査済み)

Analyze coupling(all time)で `src/ui/operations/` の 10 モジュールが degree **0.4〜0.7**
の「群れ」を形成(例: cherry_revert↔worktree 0.70、discard↔worktree 0.70、
pull_push↔stash 0.62)。原因は 2 つ混在:

1. **横断リファクタの履歴ノイズ** — ActiveModal 移行(ADR-0093)や klog 化(ADR-0096)が
   全 operations/* を同時に触った。これは解消済みで対処不要。
2. **実際のボイラープレート重複** — 各 op が `confirm_X` / `start_X` 内で
   「Backend 呼び出しを spawn → 結果で refresh → oplog 追記 → modal クリア」の
   同型スキャフォールドを繰り返している(branch.rs だけで spawn/background 18 箇所)。
   フロー側の変更が全ファイル同時修正になる根因。

## スコープ

1. **測る(先)**: 各 operations/*.rs の confirm/start 系 fn の構造を比較し、
   本当に同型な部分(spawn→apply→refresh→oplog→clear_modal)を特定してチケットに追記。
2. **抜く(後)**: 同型部分だけを 1 つの private helper
   (例: `KagiApp::run_op(plan, exec_closure, on_done)` 相当)に抽出し、
   各 op はプランと結果適用の差分だけを書く形にする。**機械的重複のみ**が対象。

## 重要な制約(S5 との関係)

S5(kagi-app / OperationController)がこのフローの正式な引っ越し先。
本チケットは **UI 側の重複除去に限定** し、新しい抽象層(trait / enum dispatch /
ジェネリックなコントローラ)を発明しない。helper 関数 1〜2 個まで。
S5 の設計を先取りしそうになったら止めて報告する。

## 触ってよいファイル

- `src/ui/operations/*.rs`、helper の置き場(`src/ui/operations/mod.rs` 内で可)。

## 触ってはいけないファイル

- `crates/kagi-git/`(ops 三つ組は対象外)、`src/ui/modals.rs`、headless、CI。

## 完了条件

- [ ] 重複測定結果(どの fn が何行同型か)がこのファイルの Implementation memo に記録されている。
- [ ] 同型スキャフォールドが helper 経由になり、operations/* 合計 LOC が減っている。
- [ ] `plan → confirm → preflight → execute → verify → oplog` の順序・`[kagi]` ログ不変。
- [ ] `cargo test --workspace` 全パス、fmt/clippy clean。

## テスト方法

既存の ops/headless テスト(stash_pop_test, discard_test, push_test 等)がフローを検証済み。

## リスク

- op ごとの微妙な差(auto_confirm 分岐、conflict 検出後の分岐)を「同型」と誤認して
  潰すこと。差があるものは無理に共通化せず残す。

## やってはいけないこと

OperationController の先行実装 / trait 導入 / modal 規約の変更 / ログ文字列変更。

## Implementation memo

### 測定結果(Phase 1)

`start_*` / `confirm_stash_push` 系 21 関数の spawn ブロックを比較。**外殻シェルは
完全に同型** で、各 7 行が機械的に同一:

```rust
cx.spawn(async move |this, acx| {
    let result = task.await;
    let _ = this.update(acx, |app, cx| {
        app.busy_op = None;
        // <per-op BODY>
        cx.notify();
    });
})
.detach();
cx.notify();
```

`<per-op BODY>` の差異はすべて BODY 内に閉じる(シェルの外には漏れない):
- 成功時 `record_op(Success)` → 失敗時 `record_op(Failed)` の op 名・`after` 形状
- 成功後の `reload(cx)` vs `refresh_remote_view(cx)`(remote 系)
- 失敗時のモーダル再表示 `set_X_modal`(op 固有のフィールド)
- `record_history`(commit / merge / cherry-pick / revert のみ)
- `status_footer = Success(...)` の有無(discard / stash / delete-branch / pull / push / amend)
- discard の `confirm_armed` リセット

### 抽出するもの / 残すもの

- **抽出**: 上記シェル(7 行)を `KagiApp::finish_op_on_main(cx, task, on_done)` 1 個に集約。
  `on_done` クロージャに `<per-op BODY>` をそのまま移動(バイト同一・順序維持)。
  `app.busy_op = None;` は helper 側で実行(BODY が常に最初にやっていた行を引き継ぐ)。
- **残す(同型ではない)**:
  - `open_merge_modal`(branch.rs): 計画系 spawn。シェル後の `cx.notify()` が無く、
    BODY も `set_merge_modal` で `record_op/reload` を通らない → 残す。
  - commit.rs の smart-commit 検出 / メッセージ生成 spawn(2 箇所): `busy_op` 未使用、
   BODY が LLM 系 → 残す。
  - `seed_history_from_reflog_async`(history.rs): `busy_op=None` 無し・空チェック → 残す。
  - `confirm_*` 同期系(checkout/pop/stash-apply/pull/push/delete-branch/conflict-continue/
    smart-consent/amend/undo/history): spawn 無し → 対象外。
- **pull_push の `finish_pull`/`finish_push`**: 既存の独自抽出。start_pull/start_push の
  シェルは他と同型なので helper 経由に統一し、`finish_*` 側の `self.busy_op = None;` は
  helper と重複するため削除(各 1 行)。`finish_*` 自体はインデントの読みやすさから残置。

### 移行対象 21 サイト

cherry_revert(start_cherry_pick / start_revert=2), discard(start_discard=1),
worktree(start_create_worktree=1), checkout(start_checkout=1),
stash(confirm_stash_push / start_stash_drop[remote+local] / start_pop=4),
pull_push(start_pull[remote+local] / start_push=3), branch(start_branch_plan /
start_set_upstream / start_rename_branch / start_merge / start_tracking_checkout /
start_switch_to_latest / start_delete_branch=7), commit(start_commit=1),
history(start_amend=1)。

### helper シグネチャ

```rust
fn finish_op_on_main<R, F>(
    &mut self,
    cx: &mut Context<Self>,
    task: impl std::future::Future<Output = R> + 'static,
    on_done: F,
) where
    R: 'static,
    F: FnOnce(&mut Self, R, &mut Context<Self>) + 'static,
```

`gpui::Task<R>`(`background_spawn` の戻り値)は `Future<Output=R>` を実装するので
`impl Future` で受ける(型名を仮定しない)。trait / enum dispatch / macro なし・クロージャ
1 個が上限。

