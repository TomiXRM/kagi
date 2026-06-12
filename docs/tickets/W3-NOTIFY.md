# W3-NOTIFY: スナックバー通知 + pull/push の非同期実行(ユーザー要望)

- Status: done (2026-06-12)
- 担当: PM(main 直接)

## 背景

ユーザー要望: 「refresh/pull/push などの時間がかかるタスクが、実行されたのか/完了したのか/失敗したのかが
わからない。スナックバー的なもので通知されると嬉しい。」

現状は Status Bar の footer 文字列のみで気づきにくい。また pull/push は同期実行のため
実行中は UI が固まり「開始した」ことすら表示できない。

## スコープ

1. **トーストスタック(スナックバー)**: KagiApp 自前のオーバーレイ(右下、最大4枚、
   自動消滅 success/info=4s, error=8s、× で手動 dismiss)。
   gpui-component の Root 通知層は「KagiApp render 中に Root entity を update すると再入 panic の
   恐れ」があるため使わず、既存 modal と同じ自前オーバーレイ方式にする(理由をメモ)
2. **発火点**:
   - `record_op`(全 confirm_* が通る中央フック)で Success/Failed/Refused → toast
   - pull/push 開始時に info toast「pull: 実行中…」
   - Refresh ボタンで success toast「Refreshed」
   - watcher の自動 reload は toast を出さない(スパム防止)
3. **pull/push の非同期化**(「開始」を見えるようにする本丸):
   - blocking 部分(repo open → preflight → execute → verify)を free fn
     `pull_blocking(repo_path, plan) -> Result<(summary, StateSummary), String>` に抽出(push も同様)
   - UI 経路: `start_pull(cx)` = busy ガード → busy_op=Some("pull") → Busy footer + info toast →
     `cx.background_spawn` で blocking 実行 → `cx.spawn` で完了を受けて `finish_pull`
     (record_op + footer + reload + busy_op=None)
   - headless 経路(KAGI_PULL/KAGI_PUSH)は従来どおり同期 `confirm_pull/confirm_push`
     (同じ blocking core を呼ぶ。ログ互換維持)
4. **busy ガード**: busy_op 中は toolbar の git 操作ボタン(Pull/Push/Stash/Pop/Undo/Branch)を
   disabled、open_pull_modal/open_push_modal も refuse。FooterStatus::Busy(⟳)を実構築
   (dead_code allow を外す)

## 完了条件

- [x] `cargo test` 全パス + own-code warning 0
- [x] 既存 headless(KAGI_PULL/KAGI_PUSH + AUTO_CONFIRM)のログに回帰なし
- [x] toast がスクリーンショットで確認できる(success / error)
- [x] 非同期 push を実操作(または CGEvent)で確認: 開始 toast → 完了 toast、UI フリーズなし
- [x] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/mod.rs` / `src/main.rs`(最小限)
- `docs/tickets/W3-NOTIFY.md`

## 触ってはいけないファイル

- `src/git/` / `src/graph/` / `tests/*` / `scripts/*` / `Cargo.toml`

## リスク

- 並行 git 操作: busy 中の操作ボタン無効化 + modal opener ガードで多重実行を防ぐ。
  読み取り(commit クリック等)は並行しても安全(別 Repository インスタンス)
- toast の自動消滅は render 外の Timer task(500ms tick、toasts が空になったら終了)。
  Instant ベースで毎 tick prune

## 実装メモ(2026-06-12)

- `Toast {id, kind, message, born}` + `KagiApp.toasts` / `next_toast_id` / `toast_ticker_alive` / `busy_op`
- `push_toast`(Window 不要)→ render 冒頭で expired prune + `ensure_toast_ticker`(500ms tick、
  空になったら task 終了)。表示は render 末尾の absolute overlay(右下、status bar の上)
- `record_op` に toast フック(Success=緑✓ / Failed・Refused=赤✕)— 全 plan 経由操作が自動で対象
- pull/push: `pull_blocking` / `push_blocking` / `verify_after_snapshot` free fn に抽出。
  UI は `start_pull/start_push`(`cx.background_spawn` → `cx.spawn` → `finish_pull/finish_push`)、
  headless KAGI_PULL/KAGI_PUSH は従来の同期 `confirm_pull/confirm_push`(同 core、ログ互換)
- busy 中: toolbar git ボタン全 off + click 理由「別の操作が実行中です」+ open_pull/push_modal refuse +
  FooterStatus::Busy(⟳)表示
- 検証: 16 suites 全パス / KAGI_PUSH headless ログ互換 / CGEvent 実クリックで
  Push modal → Confirm → 「push: 開始しました」(info)→「push: branch: main → branch: main」(success)
  の2枚スタックをスクリーンショット確認。`[kagi] async: push started/finished` ログ追加
