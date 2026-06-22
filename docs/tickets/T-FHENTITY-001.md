# T-FHENTITY-001: FileHistory を `Entity<FileHistoryView>` へ抽出

- Status: impl-done（UI 目視検証 pending）
- Group: Phase 5.1（KagiApp god-object の `Entity<T>` 分解 / ADR-0110 の続き）
- 仕様の正: ADR-0117

## スコープ

`KagiApp.file_history: Option<file_history::FileHistoryState>` を
`Option<Entity<file_history::FileHistoryView>>` へ昇格し、描画と FH 内部状態を子
エンティティに移す。**Backend を駆動する4パネルのうち最小リスクの FileHistory で、
子→親経路（`WeakEntity<KagiApp>`）の前例を確立する**のが目的。

対象:
- `src/ui/file_history.rs` … `FileHistoryView`（`FileHistoryState` データ +
  `WeakEntity<KagiApp>` + `file_history_menu`）を定義。
- `src/ui/file_history_render.rs` … 自由関数 `render_file_history_view(&FileHistoryState, …,
  cx: &mut Context<KagiApp>)` を `impl Render for FileHistoryView` に移植。ハンドラは
  `cx.listener(move |this, …| { this.app.update(cx, |app, cx| app.METHOD(…)).ok(); })` 形へ。
  ヘルパ（`fh_header_button` 等）は `Context<FileHistoryView>` へ retype。
- `src/ui/mod.rs` … FH 関連メソッド（`open_file_history` / `open_file_history_with_follow` /
  `file_history_select` / `step_file_history_selection` / `load_file_history_diff` /
  `refresh_file_history` / `toggle_file_history_follow` / `close_file_history`）を、データを
  エンティティ経由で読み書きする形に改修。生成は `cx.entity().downgrade()` を渡して `cx.new`。
- `src/ui/render_body.rs:412` … 自由関数呼び出しを `body_row.child(fh_entity.clone())` に。
- `src/ui/render_divider.rs:179` … 分割ドラッグを `fh.update` で `split` 更新＋子 notify に。
- `src/ui/render.rs:514/528` … キーボード上下＝`step_file_history_selection` を子経由に。

方針（ADR-0117 確定事項）:
- **`WeakEntity<KagiApp>` 子→親**。**リスナ/イベント/タイマ閉包の中だけで使用**、`Render`
  読み取り経路では `upgrade()` しない（再入panic回避）。
- **D1 薄いエンティティ**（Backend ロジックは KagiApp に残す）。
- **非同期の世代ガードはエンティティ更新と原子的**（同一 `fh.update` 内で `generation` 判定→
  変更→`cx.notify()`、成立時のみ diff ロード）。
- **通知スコープ規律**: 行選択/メニュー/分割→子のみ。close/jump-to-commit→KagiApp。
- **`file_history_menu` はエンティティへ移動**。`file_history_geom`(`Rc<Cell>`) は KagiApp 残置。
- **`mod.rs:3376` の生 `eprintln!("[kagi] file-history: loaded …")` を `klog!` 化**（出力バイト同一）。

## 完了条件

- [ ] `file_history` が `Option<Entity<FileHistoryView>>` になり、FH 内部操作が子スコープ再描画
- [ ] `WeakEntity<KagiApp>` 子→親経路を Render 外の閉包でのみ使用
- [ ] 非同期世代ガードが原子的（close/refresh/follow 競合で誤更新/復活なし）
- [ ] `[kagi] file-history: open …` / `… loaded N entries` を文字列・順序不変で維持（loaded は klog 化）
- [ ] 振る舞い不変（`cargo test --workspace` green、ヘッドレス契約行不変）
- [ ] `cargo fmt --check` clean、自分の diff に新規 clippy 警告を足さない
- [ ] **UI 目視検証 pending** を明記（競合系: ロード中 Back / Refresh 連打 / ロード中 Follow 切替 /
      閉じて別ファイルを即開く）
- [ ] 実装メモを末尾に追記

## 規約

- ロジック変更を最小化（D1）。`[kagi]` 行と i18n(Msg) は不変。git2 を `src/ui/` に持ち込まない。

## 実装メモ（2026-06-22）

ブランチ `refactor/fh-entity`（main から、worktree 作業）。クロスモデル運用
（cross-model-orchestration skill）：プランは Codex でクロスプランニング、実装は
Claude、検証は Codex（別ファミリ、自己検証回避）。GLM は未設定のため pool から外し
codex+claude で続行（クロスファミリは codex のみ＝多様性やや低下）。

### 確定設計（ADR-0117 / cross-plan で D1→D2 に変更）

クロスプランの推奨は「薄い D1」だったが、実装で **GPUI 再入制約**を発見：子（FileHistoryView）
のリスナから親メソッドを呼び→親が同じ子を `read/update` すると「既にリース中」で panic。
行クリック選択・refresh・follow など**子発火アクション全滅**。よって **D2「太いエンティティ」**へ。

- `KagiApp.file_history`: `Option<FileHistoryState>` → `Option<Entity<file_history::FileHistoryView>>`。
- `FileHistoryView`（`file_history.rs`）= `data: FileHistoryState` + `app: WeakEntity<KagiApp>` +
  `menu`（旧 `file_history_menu` を取り込み）+ `geom`（KagiApp と共有 Rc<Cell>）+ `panel_width` + `repo_path`。
- ロード/選択/diff ロジックはエンティティ自身（`start_load`/`reload`/`select`/`step`/`load_diff`/`set_split`、
  `file_history_render.rs` の `impl FileHistoryView`）。非同期は自分のコンテキストで spawn し
  自分の弱参照へマーシャル、`generation` で stale 破棄。
- 子→親は `WeakEntity<KagiApp>` の `app.update(...)` を**リスナ閉包内のみ**で使用（`close_file_history`/
  `jump_to_commit` のみ＝どちらも子のリースに触れない）。Render 読み取り経路では未使用。
- `render_main_diff_view` から汎用 `render_diff_list<V>` を抽出（`render_helpers.rs`）。standalone の
  Back/History ボタンは KagiApp 版だけが持ち、FH 埋め込みは持たない。
- `render_body.rs`：`render_file_history_view(...)` 自由関数呼び出し → `body_row.child(fh_entity)`。
- `render_divider.rs`：split ドラッグ→`fh.update(set_split)`、Panel ドラッグで `panel_width` をエンティティへ同期。
- `mod.rs`：`open_file_history` はエンティティ生成＋`start_load`、`step_file_history_selection` は
  `fh.update(step)`、`close_file_history` は `file_history=None`。`open_file_history_with_follow`/
  `file_history_select`/`load_file_history_diff`/`refresh_file_history`/`toggle_file_history_follow`/
  `pick_initial_file_history_index` を削除（エンティティへ移動）。
- **klog**：`mod.rs` の生 `eprintln!("[kagi] file-history: loaded …")` を `klog!` 化（出力バイト同一）。
  contract 非対称を厳守：open/refresh は open+loaded、follow-toggle は open のみ（`emit_loaded` フラグ）。

### Codex クロスファミリ検証

- R1 で **P2 を1件**指摘：FH を開いたままタブ/リポ切替すると、エンティティが捕捉した `repo_path` で
  旧リポを読み続ける（`reset_per_repo_ui` が `file_history` を未クリア）。
  → **修正**：`tabs.rs::reset_per_repo_ui` に `self.file_history = None;` 追加（FH はリポ単位 UI）。
- R2：新規リグレッションなし＝pass。

### 検証結果

`cargo build`（クリーン）/ `cargo test --workspace`（791 passed / 0 failed、`tests/file_history_test.rs`
ヘッドレス契約含む通過）/ `cargo fmt --all --check`（clean）/ `cargo clippy --workspace`（自分の差分に新規警告ゼロ）。

### 残（UI 目視検証 pending — サブエージェント不可）

実機 `cargo run` で：FH を開く／行選択（マウス+↑↓）／Refresh／Follow 切替／jump-to-commit／
行コンテキストメニュー／list-diff 分割ドラッグ／詳細ペイン幅ドラッグ／Back。**競合系**：ロード中に
Back／Refresh 連打／ロード中 Follow 切替／閉じて別ファイル即開く／FH 中にタブ切替（panic/古い diff/
復活/クロスリポ読みが無いこと）。
