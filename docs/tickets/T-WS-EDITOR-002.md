# T-WS-EDITOR-002: エディタワークスペース編集可能化(保存・再読込)

- Status: review
- Group: workspace framework / エディタモード
- 仕様の正: ADR-0120 §Decision 4。依存: T-WS-EDITOR-001。

## 背景

001 で read-only ビューアまで通した。本チケットで編集→保存→watcher 反映の一巡を
成立させる。ファイル書き込みは Git write ではないため plan pipeline(invariant 4)の
対象外だが、失敗時のユーザー通知(error handling 規約)は守る。

## スコープ

1. `code_editor` を編集可能にし、dirty 状態を entity が管理(タイトル/tree 行に ● 表示)。
2. Cmd-S(`secondary-s`、Input コンテキストでも効くよう binding 設計に注意)で
   `std::fs::write`。書き込み失敗はユーザーに通知。
3. 保存 → watcher(`WatchEvent::WorkTree`)→ status refresh の一巡で tree と右 hunk が
   更新されることを確認(既存 `refresh_working_tree_external` 経路をそのまま使う)。
4. クリーン(未編集)バッファのみ、外部変更時に watcher 駆動で再読込。dirty バッファは
   上書きせずバナーで「外部変更あり」を提示(reload ボタン)。
5. 未保存 dirty があるままモード切替/タブ切替/ファイル切替する場合は確認モーダル
   (`ActiveModal` 規約: variant + accessors + confirm/cancel 配線)。

## 触ってよいファイル

`src/ui/editor_workspace*.rs`, `src/ui/modals.rs`, `src/ui/operations/modal_state.rs`,
`src/ui/tabs.rs`(watcher 配線), `src/ui/watcher.rs`(per-path 通知が要る場合のみ最小拡張),
`src/ui/i18n.rs`, `src/ui/commands.rs`。

実装時に追加で触った(モーダルの render 配線に必須、CLAUDE.md のモーダル追加手順どおり):
`src/ui/mod.rs`(action 登録・キーバインド・confirm/cancel_active_modal 配線・watcher nudge)、
`src/ui/render.rs` / `src/ui/render_overlay.rs`(Cmd-S の on_action、新モーダルの render 配線)、
`src/ui/modal_renderers_misc.rs`(新モーダルのレンダラ)。

## 触ってはいけないファイル

`crates/kagi-git/src/ops/*`(Git write なし)、既存 `[kagi]` コントラクト行 — 変更なし
(新規 `[kagi] editor-ws: saved <path>` / `editor-ws: dirty-guard` はこのチケットの新規
契約行として追加、既存行の書式・文言は不変)。

## 完了条件

- [x] 編集 → Cmd-S 保存 → 右 hunk と WIP 行が自動更新される(`save()` が保存テキストを
      そのままバッファのスナップショットに採用し、`start_load`(tree バッジ)+
      `load_selected`(右 hunk)を明示的に再実行。sig 一致により editor への再 push は
      発生せず、保存でカーソル/スクロールが飛ばない)
- [x] dirty 表示(ヘッダのパス欄 + 選択中 tree 行に `●`)、未保存での離脱に確認モーダル
      (**ファイル切替 / close / 外部変更バナーの Reload** — ソース切替は対象外、下記
      仕様変更参照)、外部変更はクリーン時のみ自動再読込(dirty 時はバナー + Reload)
- [x] **仕様変更(ユーザー指示、2026-07-07)**: Changes⇄All のソース切替は「表示
      フィルタ」であり破棄操作ではない — dirty ガードを出さず、開いているバッファに
      一切触れない。実装: バッファを tree から分離(`open_path` が開いているファイルの
      正; header/loader/save はすべて `open_path` を参照)。ソース切替/watcher/保存後の
      tree 再構築はハイライトの再マップのみ(`restore_selection` 純関数)で、`select`
      (content 再読込 + dirty リセット)は初回ロード時のみ走る。新リストに開いている
      ファイルが無い場合(All→Changes 等)はハイライト無しでバッファ継続(ヘッダパス +
      ● が識別子)。`EditorPendingIntent::SwitchSource` は削除、代わりに `Reload`
      (外部変更バナーの Reload も破棄操作なのでガード経由)を追加。
- [x] 書き込み失敗はトースト + フッターで通知(握りつぶさない)— oplog/plan modal は
      使わない(下記スコープ逸脱の項を参照)
- [x] `cargo test --workspace` 全パス / 既存 `[kagi]` 行の変更なし(新規行のみ追加)
- [ ] GUI 目視は PM(下記チェックリスト参照)

## スコープ逸脱(記録)

- **保存失敗の通知経路**: CLAUDE.md は「oplog + modal」を原則とするが、`record_op`/oplog は
  Git の `StateSummary`(before/after head+dirty)前提の機構で、ファイル書き込み単体に
  被せると ADR-0120 §4 の「保存は Git write ではない」という区別を曖昧にする。ticket 本文が
  明示的に許容する代替(「既存の toast/footer error path」)を採用し、`repo.fetch` 失敗
  (`commands.rs`)と同じ形(`push_toast(ToastKind::Error, …)` + `status_footer =
  FooterStatus::Failed(…)`)で通知する。oplog 併用は次チケットで要望があれば追加。
- **タブ切替時の dirty ガード**: ticket 指示どおり範囲外。`reset_per_repo_ui` が
  `editor_workspace` エンティティを破棄する既存動作のまま(タブ切替で未保存の編集は
  黙って失われる — 既知の制限、モーダルは作らない)。
- **`InputEvent::Change` の判定方式**: 当初は「push 中だけ true にする `syncing: bool`」
  で実装したが、`gpui::Context::emit` は `pending_effects` に積むだけの deferred effect
  であることが実測で判明(`sync_editor` が同期的に `syncing` を false に戻した後で
  push の Change イベントが届く — headless 検証で `editor-ws: dirty-guard` が起動直後に
  誤発火して発覚)。`content_sig`/`syncing` どちらの案も ticket が許容していたため、
  タイミング非依存な「editor の現在値と `content`(最後にロード/保存した値)を比較」方式に
  切り替えた。

## テスト方法

`cargo test --workspace`(`ui::editor_workspace::tests` に純関数の新規テスト:
`should_guard_navigation_only_when_dirty_and_actually_switching` /
`open_buffer_survives_source_switch`(仕様変更のインバリアント)/
`initial_load_selects_and_loads_first_file` /
`editor_save_path_joins_repo_root_and_relative_path`)+ headless
(`KAGI_EDITOR_WS=1 KAGI_NO_RESTORE=1 KAGI_OPEN_REPO=<dirty fixture> ./target/debug/kagi`,
バックグラウンド起動 + sleep + kill 方式、macOS に `timeout` なし)で
`editor-ws: open` / `files 1` / `file <path>` が起動直後に出て以降 `dirty-guard` が
誤発火しないことを確認済み。実際のキー入力/保存は subagent からは打鍵できないため
未検証 — 上記の純関数テストと GUI チェックリストでカバー。

## リスク(実装後の知見を反映)

- InputState(gpui-component)からの全文取り出し/set_value の毎 frame クロバーは
  既存の `content_sig`/`pushed_sig` パターンをそのまま再利用して回避。
- `InputEvent::Change` は gpui-component の `set_value` からも(disabled を一時解除して
  `replace_text_in_range` を通るため)発火する。`cx.emit` は deferred effect なので、
  同期的なフラグでは区別できない(上記スコープ逸脱参照)— 実装は編集内容そのものの
  比較に切り替え済み。
- キーバインドの Input コンテキスト競合: gpui-component 0.5.1 の `src/input/state.rs`
  を確認し、`secondary-s`/`cmd-s`/`ctrl-s` の既存バインドが無いことを確認済み
  (`SaveEditorFile` はコンテキスト述語なしでグローバル登録)。
