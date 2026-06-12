# W2-INSPECTOR: Right Panel を Commit Inspector に再構成(worktree レーン)

- Status: in-progress / 依存: ADR-0015
- 原文要件: requirements-gk-parity.md(要件4)

## 手順(競合対策のため厳守)

1. **最初に** `render_detail_panel` 一式を `src/ui/inspector.rs` に抽出(mod.rs は呼び出しのみ。挙動不変でビルド確認)
2. Inspector 構成へ並び替え・拡張(上から):
   - **Summary**: summary(タイトル)・short SHA・**Copy ボタン**(`cx.write_to_clipboard(ClipboardItem::new_string(full_sha))` — gpui API は registry で確認)・その commit の ref バッジ(badge map は commit_list の build_badge_map 流用 or 必要データを引数追加)
   - **Metadata**: author + authored date / committer + committed date(CommitDetail に committed date がなければ detail_panel.rs に追加 — Commit モデルには committer がある)/ parents / full SHA(折返しなし truncate)/ message 本文
   - **Contextual Actions**: Create branch here / Cherry-pick onto HEAD / Copy SHA(Summary と同じ動作)— **metadata の下**に移動
   - **Changed Files**: 既存 tree 表示 + **Path⇄Tree トグル**(Commit Panel の List|Tree セグメントと同じ部品感。flat 表示は path 全表示)。count・status バッジ・active ハイライトは既存維持
3. copy 実行時 footer に `SHA copied` を表示。**コピーは raw 値(ZWSP 禁止)**
4. ログ: copy 時 `[kagi] copy-sha: <short>`

## 完了条件
- cargo test 全パス + 警告 0 / KAGI_SELECT_FIRST・KAGI_OPEN_FIRST_FILE 回帰なし
- 並びが Summary→Metadata→Actions→Files(見た目は PM 確認)
- worktree ブランチにコミット。push はしない

## 触ってよい: src/ui/(inspector.rs 新規・detail_panel.rs(committed date 追加)・mod.rs 最小限)/ docs/tickets/W2-INSPECTOR.md
## 触ってはいけない: src/git/ src/graph/ src/lib.rs src/main.rs tests/ Cargo.toml scripts docs他
