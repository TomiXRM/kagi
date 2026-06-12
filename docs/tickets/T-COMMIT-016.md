# T-COMMIT-016: Smart Commit Message — UI(Generate ボタン / 日英トグル / 静かな fallback)

- Status: blocked(T-COMMIT-015 backend 待ち / ADR-0044 Proposed)/ v0.2
- 依存: T-COMMIT-015 / ADR-0044
- 関連: lane W14-SMART(UI は PM 主体)

## 背景

Smart Commit backend を Commit Panel に繋ぐ。生成ボタン、日英/style トグル、background 実行、失敗時は静かに手動編集へ。

## スコープ

- Commit Panel に「Generate message」(staged が空なら disabled)。日英・Conventional/Plain トグル。
- 押下で `collect_staged_diff` → `generate_message` を `cx.background_spawn`(タイムアウトつき)。
  結果を message Input に流す(ユーザーが上書き編集できる叩き台)。
- 失敗 / タイムアウト / offline は **静かに rule-based 結果 or 手動編集**へ(エラーモーダルで止めない、トースト程度)。
- lang/style 選択は draft(ADR-0042)と同様に記憶。

## 完了条件

- [ ] Generate で message が入る(staged のみから)
- [ ] 失敗時に UI が止まらず手動編集できる
- [ ] 日英 / style トグルが効く
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/commit_panel.rs` / `src/ui/mod.rs` / `src/ui/commands.rs`
- `docs/tickets/T-COMMIT-016.md`

## 触ってはいけないファイル

- `src/git/message_gen.rs`(backend は T-COMMIT-015)/ `Cargo.toml`

## テスト方法

1. `cargo test`
2. UI は PM がスクリーンショット確認。`KAGI_OFFLINE=1` で fallback 経路を確認

## リスク・規約

- 生成中も UI を塞がない(background)。タイムアウトで必ず復帰
- 生成結果はあくまで叩き台。自動 commit はしない(ユーザー確認必須)
