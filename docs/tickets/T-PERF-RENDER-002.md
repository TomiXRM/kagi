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
