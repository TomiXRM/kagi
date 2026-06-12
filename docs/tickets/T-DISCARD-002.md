# T-DISCARD-002: Discard UI — per-file ボタン + danger 確認 modal + async 実行

- Status: done
- 依存: T-DISCARD-001
- 関連: ADR-0046、lane W17-DISCARD

## スコープ

- Commit Panel unstaged 行に hover Discard アイコン(untracked/conflicted 行は出さない or disabled+tooltip)
- danger 確認 modal(赤系・destructive 表示、ADR-0046 の文言)+ ESC cancel + backdrop occlude
- 実行は W15 パターン: `start_discard` + `discard_blocking` free fn、busy_op="discard"、toast、reload

## 完了条件

- [x] modal 確認 → 実行 → 対象が unstaged から消える(GUI は PM 検証 / headless で挙動確認済み)
- [x] `cargo test` 全パス、own-code warning 0

## 実装メモ

- `src/ui/mod.rs`:
  - `DiscardModal{plan, paths, skipped, is_all, error}` を追加(KagiApp に `discard_modal` フィールド、
    init 2 箇所 + reload で None クリア、Enter ignore ガードに追加)。
  - per-file Discard ボタン: Commit Panel unstaged 行(flat / tree 両方)に追加。
    **conflicted 行と untracked 行(panel では `Added` として surface)には出さない**。click で
    `open_discard_modal_for_index(fi)` → `plan_discard` → `discard_modal` セット。色は
    `theme().color_blocker`(赤、ハードコードなし)。
  - danger 確認 modal `render_discard_modal`: 赤枠カード + タイトル(`color_blocker`)、対象一覧
    (`overflow_y_scroll` + `max_h(180px)`)、skipped セクション、warnings/blockers、recovery 文言
    (ADR-0046)、Cancel + 赤 Discard。**backdrop と card 両方に `.occlude()`**(click-through 対策)。
    ESC で `cancel_discard_modal`(`on_key_down` で escape)。blockers / 0 件で Discard ボタン非表示。
  - 実行は W15 パターン: `start_discard(cx)` が busy_op="discard" をセットし
    `cx.background_spawn(discard_blocking(...))` → 完了で record_op("discard", Success/Failed) +
    Busy footer + toast + reload。`discard_blocking` free fn が execute_discard → verify を実行し
    `(human_summary, StateSummary{dirty=oplog_summary})` を返す(復元ハンドルを oplog に載せる)。
- `chars()` ベース truncate / `theme()` 色のみ / own-code warning 0。
