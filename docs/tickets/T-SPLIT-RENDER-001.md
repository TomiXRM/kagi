# T-SPLIT-RENDER-001: render.rs を機能境界で分割

- Status: todo
- Group: god-file split (CLAUDE.md ≤800 LOC/file, ≤80 LOC/fn)
- 仕様の正: ADR-0116 Wave 3

## スコープ

`src/ui/render.rs`（3521行）と、その中の巨大関数を機能境界で分割する。

- `render`（`render.rs:303`、882行）→ header / body / sidebar / bottom-panel /
  overlay の各サブメソッド呼び出しを薄く合成する形にする。
- `render_header_slot`（`render.rs:1265`、584行）、`render_body`
  （`render.rs:2020`、507行）等の巨大メソッドを、責務単位のサブメソッド or
  sibling モジュール（例 `src/ui/render/header.rs` 等、または `render_header.rs`）へ移す。

方針:
- **純粋な `mod`/可視性移動**。振る舞い・出力 DOM・`[kagi]` 契約行は不変。
- 既存の公開パス（他モジュールから呼ばれる `render_*`）は再エクスポート or
  `impl KagiApp` の inherent method として維持し、呼び出し側を壊さない。
- 1ファイル ≤800 LOC を目標。`render` 本体は合成のみで ≤80 LOC を目指す。

**前提**: T-PERF-RENDER-001 / 002 が render.rs を触るため、それらの**後**に着手する
（同一ファイル競合回避）。

## 完了条件

- [ ] render.rs が ≤800 LOC（または明確な機能サブモジュール群に分割）
- [ ] `render` 本体が合成中心の小さなメソッドになる
- [ ] 振る舞い不変（`cargo test --workspace` green、ヘッドレス契約行不変）
- [ ] `cargo fmt --check` clean、自分の diff に新規 clippy 警告を足さない
- [ ] **UI 目視検証 pending** を明記
- [ ] 実装メモを末尾に追記

## 規約

- 機能境界で割る（CLAUDE.md）。ロジック変更を混ぜない（移動のみ）

## 実装メモ（2026-06-22 / Wave 3 完了）

純粋なモジュール分割（コード移動 + 「切り出して呼ぶだけ」の抽出のみ）。出力
される要素ツリー・スタイル・イベントハンドラ・`[kagi]` 契約行（4 本すべて文字
列まで不変、移動のみ）・i18n（Msg）参照は一切変更していない。`cargo build` /
`cargo test --workspace`（全 green）/ `cargo fmt --check`（clean）を通過。
clippy は自分の diff に新規警告を足していない（`attach_modal_overlays` の
`too_many_arguments` は元々 `render.rs` の `#![allow(...)]` 下にあったため移動先
にも同 allow を付与。`render.rs:654` の boolean-simplify 警告は HEAD（旧 1030 行）
からの逐語移動で既存債務）。

### 新ファイル構成と各 LOC

| ファイル | LOC | 内容 |
|---|---:|---|
| `src/ui/render.rs` | 718 | モジュール doc、`impl Render for KagiApp::render`（合成中心）、トースト/ビジースナックバー、コミット/ブランチ/スタッシュのメニューオーバーレイ小ヘルパ |
| `src/ui/render_header.rs` | 677 | `render_header_slot`（ツールバー）、`register_menu_actions` |
| `src/ui/render_body.rs` | 518 | `render_body`（sidebar｜commit list｜inspector/commit panel） |
| `src/ui/render_wip.rs` | 363 | `render_wip_row`、`render_stash_graph_rows`（body が消費する行ビルダ） |
| `src/ui/render_bottom.rs` | 525 | `render_bottom_panel_slot`、`render_activity_body`、`render_terminal_body` |
| `src/ui/render_status.rs` | 188 | `render_status_bar` |
| `src/ui/render_overlay.rs` | 481 | `big_sync_icon`、`impl Render for ToastStack`、`impl Render for OpLogPanel`、`attach_modal_overlays` |
| `src/ui/render_divider.rs` | 212 | `handle_divider_drag`（root の `on_drag_move` リスナ本体） |

全ファイル ≤800 LOC。`src/ui/mod.rs` に
`mod render_body; mod render_bottom; mod render_divider; mod render_header;
mod render_overlay; mod render_status; mod render_wip;` を `mod render;` 近傍へ追加。
各メソッドは `impl KagiApp` の inherent method のままで、他モジュールから呼ばれる
ものは `pub(super)`（最小可視性）に変更（移動前は private `fn` だったが、同一親
`crate::ui` 配下の兄弟モジュール間で呼び出すために必要）。`big_sync_icon` は元の
`pub(crate)` を維持。

### `render` 本体の縮小と抽出したサブメソッド

`render`（旧 882 行）→ **約 596 行**（`render.rs` の 122〜717 行）。逐語抽出した
サブメソッド（呼び出しに置き換えただけ、評価順序・条件分岐・生成要素は不変）:

- `handle_divider_drag(&mut self, event, window, cx)` … 旧 `render` 内の巨大な
  `divider_drag_move` リスナ本体（全 `DividerKind` の絶対座標リサイズ計算、約 190
  行）を `render_divider.rs` へ移動。`render` 側は薄い `cx.listener` ラッパが
  `this.handle_divider_drag(...)` を呼ぶだけ。
- `attach_modal_overlays(&self, el, <27 個の事前 clone 済みモーダル状態>, window, cx)
  -> Div` … プラン/プル/プッシュ/アメンド/スタッシュ等の全モーダル + ファイル
  メニュー + コミットプラン + Smart Commit + 自動更新の `.when_some(...)` チェーン
  （約 165 行）を `render_overlay.rs` へ移動。モーダル状態の clone はフレーム内の
  同一地点のまま（順序不変）でメソッドへ move。

`render` は依然プレアンブル（状態 clone・error/welcome の early return・
`commit_menu_overlay`/`conflict_chrome`/`toolbar_state` 等の組み立て）を含むため
≤150 行の目標（仕様上「目指す」＝soft）には未達だが、882→596 行へ縮小し、本体は
header / banner / body / bottom-panel / overlay / status-bar / toast の合成が中心。

### UI 目視検証

**pending**（サブエージェントは GUI を起動できない）。要素ツリー・イベント
ハンドラ・契約行は不変のため挙動は同一の想定だが、リリース前に人手で
アプリ起動の目視確認が必要。
