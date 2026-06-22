# T-PERF-RENDER-002: sidebar.rows の毎フレーム再構築を止め、render の clone を削減

- Status: todo
- Group: anti-pattern / perf (refactor-plan Step 3.4)
- 仕様の正: ADR-0116 Wave 2

## スコープ

render が派生状態を毎フレーム再計算・clone しており、render 純粋性違反かつ
O(全ref) アロケーション/フレームになっている。

1. **sidebar.rows の毎フレーム再構築**
   `src/ui/render.rs:496-511` が毎 render で `build_sidebar_rows(...)`
   （`src/ui/sidebar.rs:496` 付近）を呼び、全 branch/tag/stash/worktree を
   clone+collect して `self.sidebar.rows` に代入（コメント "Rebuilt every render"）。
   → 入力（branches / collapsed 集合 / filter 文字列）が変化した時だけ再構築する。
   ダーティフラグか入力ハッシュ比較を `SidebarState` に持たせ、render は
   キャッシュ済み rows を参照するだけにする。filter 入力変化で dirty を立てる。

2. **theme()/zoom() の hoist と set_rem_size のゲート**
   `src/ui/render.rs:402` 付近の `theme::theme()` / `theme::zoom()` を `render()`
   冒頭で1回だけ取得しヘルパへ渡す。`set_rem_size` は zoom 変化時のみ呼ぶ。

3. **render_rows の行 clone 削減**
   `src/ui/render_helpers.rs:51` の `row.clone()`（全体コピー）を避け、ハンドラは
   `ix` を使う形にする（refactor-plan Step 3.4 の `render.rs:3176` と同趣旨）。
   表示フィールドは `SharedString`（Arc bump）で済むようにする。

## 完了条件

- [ ] sidebar rows が入力変化時のみ再構築（無変化フレームで再 collect しない）
- [ ] theme()/zoom() が render あたり1回、set_rem_size は変化時のみ
- [ ] render_rows の全体 `row.clone()` 除去
- [ ] サイドバー表示・折りたたみ・フィルタ・選択が従来通り（振る舞い不変）
- [ ] `cargo build` + `cargo test --workspace` green、`cargo fmt --check` clean
- [ ] **UI 目視検証 pending** を明記
- [ ] 実装メモを末尾に追記

## 規約

- 派生状態の単一情報源を保つ。dirty 化の取りこぼし（折りたたみ/フィルタ/外部 reload）
  に注意。T-PERF-RENDER-001 と同じ render.rs を触るため**後続**で実施

## 実装メモ（2026-06-22, ADR-0116 Wave 2 / 振る舞い不変）

### 1. sidebar.rows のダーティ化方式 — 「安価なフィンガープリント比較」を採用
取りこぼしの起きにくい方を選択（mutation 経路ごとに dirty フラグを散らすのではなく、
render で毎フレーム安価なフィンガープリントを計算して前回と比較）。

- `SidebarState` に `rows_fingerprint: u64` を追加（`SidebarState::new` で `u64::MAX`
  初期化 → 初回 render は必ず再構築）。
- `KagiApp` に `view_epoch: u64` を追加。`active_view` を書き換える**全経路の
  チョークポイント**で `wrapping_add(1)`：
  - `apply_tab_view`（reload / load_more / 非同期ロード / `switch_repo` のキャッシュ
    即時スワップ / リモートタブ等、`active_view = view` の全スワップが通る単一経路）
  - `show_welcome`（`apply_tab_view` を経由せず branches/remote_branches/tags/stashes
    を個別代入する唯一の例外経路 → ここでも明示 bump）
- 新規 free fn `sidebar::sidebar_rows_fingerprint(...)`：`view_epoch` + 各コレクション長
  + collapsed 集合（`SECTION_*`）+ branch_groups_collapsed + 小文字フィルタ文字列 を
  ハッシュ（アロケーションなし）。集合は反復順非決定なので per-element ハッシュの XOR
  畳み込みで順序非依存に。コレクション**内容**の変化は `view_epoch` がカバーするので
  全 ref リストをハッシュしない（毎フレーム O(全ref) を避ける）。長さはO(1)の保険。
- render（`src/ui/render.rs`）：フィンガープリントが前回と異なる時だけ
  `build_sidebar_rows` を呼んで `rows` と `rows_fingerprint` を更新。無変化フレームは
  キャッシュ済み `rows` を再利用。

### 取りこぼし対策（dirty 化の経路カバレッジ）
- 折りたたみトグル：`sidebar.collapsed` / `branch_groups_collapsed` をフィンガープリント
  に直接含めるため、トグルした次フレームで自動的に差分検知（クリックハンドラ側の改修不要）。
- フィルタ入力：filter `InputState` は `KagiApp` への通知経路を持たない（subscription なし、
  render で都度 read）。よって epoch では追えないため、小文字フィルタ文字列を毎フレーム
  read してフィンガープリントに畳み込む（従来どおり都度 read のコストのみ）。
- 外部 reload / タブ切替 / ブランチ更新：すべて `apply_tab_view`（または `show_welcome`）
  を通るので `view_epoch` bump で検知。

### 2. set_rem_size のゲート
`KagiApp.last_rem_size: f32`（`f32::NAN` 初期化）を追加。render 冒頭で
`theme::rem_size_px()` を1回取得し、前回値と異なる時だけ `window.set_rem_size` を呼ぶ。
ウィンドウ再生成（macOS Dock reopen のみ）は必ず KagiApp を新規構築するため
`last_rem_size` も NAN にリセットされ、初回フレームで再アサートされる（self-heal 維持）。
ズーム変更（menu / settings stepper）はウィンドウを作り直さず `cx.notify()` のみなので
同一エンティティが生き残り、次 render で値差分により1回だけ再アサートされる。
※ `theme()` は static 参照を返す安価なアトミックロードでアロケーションがないため、
ヘルパ群への引数スレッド化（大改修）は行わず局所のゲートに留めた（ADR の "局所で可"）。

### 3. render_rows の行 clone 削減
`src/ui/render_helpers.rs` の `let row = row.clone();`（行全体コピー）を除去。`row` は
`rows: &[CommitRow]` から借用した `&CommitRow` のまま使用。クリック/コンテキストハンドラ
は `ix` のみキャプチャし行は捕捉しない。各フィールド参照は Copy 値か `SharedString`
（Arc bump）か小さな Vec の clone のみで、`&mut *cx`（`cx.listener` / `render_badges_column`）
とも借用が両立する（ビルド確認済み）。

### 検証
- `cargo build --workspace` green / `cargo test --workspace` green（44 test result すべて
  ok、失敗0）/ `cargo fmt --check` clean / `cargo clippy --workspace` 新規警告なし
  （新 fn には既存 `build_sidebar_rows` と同様 `#[allow(clippy::too_many_arguments)]`）。
- `[kagi]` / `klog!` 契約行は不変（diff に出現なし）。
- **UI 目視検証 pending**：サブエージェントは GUI を起動できない（CLAUDE.md）。
  サイドバー表示・folder 折りたたみ・フィルタ・選択ハイライト・stash/worktree 行・
  ズーム変更時の rem サイズ反映が従来と同一に見えることは、人間（または主セッション）
  によるアプリ起動での目視確認が必要。
