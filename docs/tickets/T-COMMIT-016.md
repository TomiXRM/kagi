# T-COMMIT-016: Smart Commit Message — UI(Generate ボタン / 日英トグル / 静かな fallback)

- Status: todo(ADR-0044 決定済み: rule-based 既定 / Ollama 検出のみ / LLM は明示 opt-in + 初回同意)
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

## 実装メモ (W14-SMART UI, completed)

- 新規 `src/ui/smart_commit.rs`(state / settings / detection glue)。描画は `mod.rs`(既存 modal・theme 流儀を再利用)。
- Commit Panel footer に Smart toolbar 追加: **Suggest**(rule-based、常時 = staged>0 で活性)/ **Lang: EN⇄日本語** / **Style: CC⇄Plain** トグル / **Generate with Local LLM**(`llm_offered()` = ollama 検出 + ユーザー有効化 + not offline の時だけ表示)。検出時は「● Local LLM available」、操作結果は status 行(toast 程度、止めない)。
- 既定 = rule-based のみ。LLM は明示 opt-in: 未有効化で Generate 押下 →「Enable Local LLM…」/ 初回**同意ダイアログ**(`CONSENT_LINES` = ADR-0044 の4文言を verbatim 表示、unit test で検証)→ 有効化を settings.json に永続化。
- model selection: 検出した `/api/tags` を model picker で列挙。1つでも初回確認、複数なら必ず選択(ADR-0044)。選択後 settings.json に保存。
- 検出は `open_commit_panel` から `cx.background_spawn`(repo ごと1回、到達確認のみ = diff 送らない)。生成は `run_smart_generation` で `collect_staged_diff`+`generate_message` を background 実行、失敗/timeout/offline は**静かに rule_based / 手動編集**へ。生成結果は `pending_smart_msg` 経由で次 render(Window 有り)で `set_value` 挿入。**既存 message が非空なら上書きしない**(ticket 指定)。
- lang/style は draft 同様に settings.json に記憶(`smart_commit_lang`/`_style`)。`theme.rs` の settings 機構を **key 保存型に一般化**(`read_setting`/`write_setting`/`parse_string_value`、theme key を clobber しない)。
- headless ログ: `KAGI_SMART_SUGGEST=1` で rule-based 結果を stderr(`[kagi] smart-suggest: ...`)。検出は `[kagi] smart-commit: ollama_available=.. models=..`。`KAGI_OFFLINE=1` で検出・生成停止(決定的)。`KAGI_OLLAMA_HOST` で host override 可(既定 localhost:11434)。
- `cargo test` 全パス(smart_commit 3 + theme settings 1 含む)、own-code warning 0、clippy 0(自ファイル)。
