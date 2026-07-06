# T-WS-EDITOR-006: エディタタブ(複数バッファ・dirty 表示・close 確認)

- Status: done (PM accepted 2026-07-07 — tests+headless+render verified; interaction GUI checklist pending user)
- Group: workspace framework / エディタモード
- 発端: ユーザー要望(2026-07-07)「エディタ編集 pane を複数持てるタブ機能が欲しい。
  ファイルが編集中か未編集かはわかりやすく表示されて欲しい。編集中のものを x で
  消すときは unsave でいいのか表示」。依存: T-WS-EDITOR-002(spec change 込み)。

## スコープ(実装済み)

1. **複数バッファ**: `EditorWorkspaceView` に `open_tabs: Vec<PathBuf>`(タブ順)+
   `tab_cache: HashMap<PathBuf, EditorBufferState>`(非アクティブバッファ)。
   アクティブバッファのフィールドは従来どおり view にフラット
   (`KagiApp.active_view`/`tab_cache` と同じ active+cache スワップ、ADR-0075 準拠)
   なので render/loader は無改修。タブごとに専用 `InputState`
   (共有して `set_value` 差し替えにすると undo 履歴がファイル間でリークするため)。
2. **タブストリップ**: センターペイン最上部。チップ = ファイル名 + dirty `●`
   (warning 色)+ `×` ボタン。アクティブタブは `bg_base` でエディタ面と地続き。
   `×` は `cx.stop_propagation()` でチップの activate と分離。
3. **dirty 可視化**: タブチップ / ヘッダパス / tree 行(開いているタブが dirty なら
   選択外の行にも)の3箇所に `●`。
4. **close 確認**: dirty タブの `×` → 既存の unsaved-changes モーダル
   (`EditorPendingIntent::CloseTab(PathBuf)`)。Discard で `close_tab_now`
   (隣接タブへ: 次を優先、なければ前 — `next_active_tab` 純関数)。
   ワークスペース close(← Graph / toolbar / Cmd-Shift-E)のガードは
   `any_dirty()`(バックグラウンドタブ含む)に拡大。
5. **ファイル切替のガード廃止**: tree クリック / ↑↓ は「タブを開く」操作になり
   破棄が発生しないため、T-WS-EDITOR-002 の SelectFile ガードは削除
   (ユーザーのメンタルモデルどおり)。ガードが残るのは Reload / CloseTab /
   ワークスペース Close の3破棄経路のみ。

## 設計メモ

- ponytail: クリーンなアクティブタブは、新規ファイルを開くとき**置き換え**
  (VSCode の preview tab 相当)— ↑↓ でツリーを流し読みするだけでタブが
  1 ステップ 1 枚増えるのを防ぐ。dirty タブは常にタブとして生存。
  クリーンなタブを2枚並べたい場合は pinned/preview 区別の導入が upgrade path。
- ponytail: タブストリップは横スクロールなし(多数タブは縮小 + truncate)。
  実運用で溢れたら overflow scroll を足す。
- watcher: バックグラウンドの dirty タブにも `external_changed` を立て、
  再アクティブ時にバナー表示。クリーンなタブはアクティブ化時に必ず再読込
  (`open_tab` の clean-refresh)するため watcher 側の処理は不要。
- `load_selected` の marshal-back に `open_path` 一致ガードを追加 — タブ切替を
  跨いだロード結果が別タブに着地しない。切替先はアクティブ化時に自前で
  ロードし直すため取りこぼしなし。
- deferred な `InputEvent::Change` がスタッシュ後に届くケース: 購読ハンドラが
  発火元 entity をアクティブ editor と比較し、不一致なら `tab_cache` 側の
  該当バッファに dirty を記帳。

## 完了条件

- [x] 複数ファイルをタブで並行編集できる(dirty バッファはスタッシュされ生存)
- [x] dirty 表示: タブ / ヘッダ / tree 行で `●`
- [x] dirty タブの `×` → unsaved 確認モーダル(Discard / Cancel、Enter/Esc 対応)
- [x] `cargo build` / `cargo test --workspace` 全パス、`cargo fmt --check` clean、
      clippy 警告 baseline(39)から増加なし、`git2::` gate 0
- [x] headless: 既存 `editor-ws:` 契約行の変更なし(fixture で確認)
- [x] 純関数テスト: `next_active_tab_prefers_next_then_previous`
- [ ] GUI 目視は PM(タブの開閉・切替・dirty dot・× 確認・undo のタブ独立性)

## テスト方法

`cargo test --workspace` + headless(T-WS-EDITOR-002 と同じ fixture 手順)。
タブ操作そのもの(クリック・×・undo 独立性)は GUI 目視。
