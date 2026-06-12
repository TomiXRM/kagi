# W14-TEMPLATE: Message Template(type/scope/summary/body/test/risk + plain⇄template)

- Status: done
- 担当: worktree agent(Opus)
- チケット: T-COMMIT-009(完了条件・ファイル制約はチケットが正)
- 関連: ADR-0042(draft の mode フィールド)

## 補足

- 組み立て/parse は純関数で `src/git/message_template.rs`(新規)+ `tests/message_template_test.rs`
- draft autosave(wave1 実装済み・mod.rs の draft_save_gen debounce)と整合: mode 切替も draft に保存
- template 入力欄は gpui-component InputState(手書き入力 widget 禁止)

## 共通規約(全 lane 同一)

- 破壊的 git 操作の実装禁止(`--force` / `reset --hard` / `git clean`)。確認なし実行禁止
- 検証は `scripts/make_fixture.sh` の fixture / tempdir のみ。**ユーザー repo 禁止**
- 文字列切り詰めは `chars()` ベース。色は theme() 経由(ハードコード禁止)
- `cargo test` は exit code を確認(パイプで握りつぶさない)。own-code warning 0
- macOS に `timeout` コマンドはない。`cargo build` 後の GUI 起動確認は PM が行う
- 完了時: 担当チケット末尾に実装メモ追記 + Status 更新、worktree branch に commit

## 実装メモ (done)

- 純関数 `src/git/message_template.rs`(新規)+ `tests/message_template_test.rs`(新規・20 ケース)。
  `assemble` / `parse_message` / `TemplateFields` / `TYPE_CHOICES`。詳細は T-COMMIT-009 末尾参照。
- UI 配線は `src/ui/mod.rs`(`commit_panel.rs` は無改変)。6 欄すべて gpui-component InputState
  (body は multi_line + auto_grow)。plain⇄template トグル + type quick-pick chips。色は theme() 経由。
- draft autosave(既存 250ms debounce)に統合: 保存値は展開済み plain text、`mode` に現モードを渡す。
  `drafts.rs` は無改変(API/`mode` のみ使用)。復元時 `mode=="template"` は再 parse して template で開く。
- `cargo build` green / own-code warning 0、`cargo test` 全 suite pass(exit 0 確認)。Cargo.toml 無改変。
