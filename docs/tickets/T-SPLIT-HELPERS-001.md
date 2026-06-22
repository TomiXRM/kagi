# T-SPLIT-HELPERS-001: render_helpers.rs / modal_renderers.rs の分割と共通カード抽出

- Status: todo
- Group: god-file split (CLAUDE.md ≤800 LOC/file, ≤80 LOC/fn)
- 仕様の正: ADR-0116 Wave 3

## スコープ

### A. render_helpers.rs（3153行）— 「render.rs の溢れ」を解体
- `render_commit_panel`（`render_helpers.rs:2321`、832行・11引数フリー関数）と
  コミットパネル系 render を、その状態が住む `src/ui/commit_panel.rs` 近傍へ移す。
- ファイル履歴系 render（`render_fh_*`）を `src/ui/file_history.rs` 近傍へ移す。
- バッジ列・ファイルメニュー描画は責務単位の sibling へ。
- 目標: render_helpers.rs を ≤800 LOC まで縮小（あるいは責務別に解消）。

### B. modal_renderers.rs（3395行）— 共通カード骨格を RenderOnce 化
- 各モーダル renderer（238〜293行級が多数）が共有するカード骨格（ヘッダ/本文/
  フッタのボタン行）を `RenderOnce` コンポーネントに抽出し、各 renderer を
  80 LOC 目標へ寄せる。1モーダル=1関数の機能境界自体は保つ。

方針:
- **振る舞い・出力 DOM 不変の移動/抽出**。`[kagi]` 契約行・i18n（Msg）参照は不変。
- 公開パスは再エクスポートで維持。`render_commit_panel` を CommitPanel の
  inherent method（将来の Entity 化の足場）にしてもよいが、本チケットでは
  Entity 化は**やらない**（Phase 5.1 の領分）。引数羅列の解消に留める。

A と B は別ファイルなので**並行実施可**。ただし render.rs split（T-SPLIT-RENDER-001）
とは概念的に近いので、render.rs split の後に着手して取り合いを避ける。

## 完了条件

- [ ] render_helpers.rs / modal_renderers.rs がそれぞれ ≤800 LOC へ縮小
- [ ] `render_commit_panel` の 11 引数が構造体集約 or inherent method 化で解消
- [ ] modal の共通カードが RenderOnce で 1 箇所化
- [ ] 振る舞い不変（`cargo test --workspace` green）、`cargo fmt --check` clean
- [ ] **UI 目視検証 pending** を明記
- [ ] 実装メモを末尾に追記

## 規約

- 移動・抽出のみ。Entity 化（Phase 5.1）には踏み込まない

## 実装メモ（2026-06-22 実施 / 振る舞い不変・コード移動と共通カード抽出のみ）

### A. render_helpers.rs（3157 → 631 LOC）の解体

責務単位で sibling モジュールへ逐語移動し、`render_helpers.rs` に
`pub(crate) use super::<mod>::*;` の再エクスポートを置いて、既存の
`use super::render_helpers::*;` 経由の呼び出し元（`render_body.rs` /
`render_overlay.rs` / `render_wip.rs`）を **一切触らずに** 公開パスを維持した。

| 新モジュール | LOC | 移動した関数 |
|---|---|---|
| `commit_panel_render.rs` | 1324 | `cp_active_wip` / `render_unstaged_flat_row` / `render_unstaged_tree_row` / `render_staged_flat_row` / `render_staged_tree_row` / `render_commit_panel`（コミットパネル系。状態の住む `commit_panel.rs` 近傍）|
| `file_history_render.rs` | 889 | `fh_header_button` / `render_file_history_view` / `render_fh_message` / `render_fh_error` / `render_fh_list_and_diff` / `render_fh_commit_list` / `render_fh_row` / `render_fh_detail_pane` / `render_fh_row_menu`（ファイル履歴系。`file_history.rs` 近傍）|
| `badges.rs` | 283 | `badge_priority` / `WipRowClick` / `render_wip_diffstat` / `render_badges_column`（バッジ列・WIP diffstat）|
| `file_menu.rs` | 84 | `render_file_menu_overlay`（ファイルメニュー描画）|

`render_helpers.rs`（631 LOC）に残したのはグラフ行/メイン diff の共通ビルダ
（`graph_lane_pad_l` / `connector_line` / `render_rows` / `render_load_more_row` /
`render_loading_placeholder` / `render_main_diff_view` / `with_vertical_scrollbar`）。
`connector_line` は `badges.rs` から使うため `pub(crate)` に昇格（可視性は実質不変）。
別モジュールに分けた理由: コミットパネル/ファイル履歴の render はそれぞれ ~1300 /
~900 LOC と大きく、状態を持つ `commit_panel.rs` / `file_history.rs`（200 行台）に
直接混ぜると逆に肥大化する。`*_render.rs` を新設して「状態 vs 描画」を分離した方が
責務が明確で、状態ファイルは小さく保てる。

### A3. `render_commit_panel` の 11 引数 → 4 引数（`&self` inherent method 化）

11 引数のうち 6 つ（`commit_input` / `template_mode` / `template_inputs` /
`smart` / `unstaged_scroll_handle` / `staged_scroll_handle`）は呼び出し元
（`render_body`）で `self.<field>.clone()` をそのまま渡していた **KagiApp フィールド
そのもの**、1 つ（`_active_wip`）は PERF 対応で既に未使用（行ごとに再計算）だった。
そこで `impl KagiApp { fn render_commit_panel(&self, panel, panel_width, preview, cx) }`
の inherent method に変換し、本体冒頭で当該 6 フィールドを `self` から `let` 束縛して
**従来の呼び出し側 clone を 1:1 で再現**。ローカル変数名・clone 回数・要素ツリーは不変。
`render_body` 自体が `&self` メソッドで、その中で `cx.listener`/`cx.processor` を呼ぶ
既存パターンと同型なので、`&self` + `&mut Context` 同時借用は安全（render 中に
`cx` でエンティティを読み直す禁止事項にも該当しない）。Entity 化（Phase 5.1）には
踏み込んでいない。

これに伴い `render_body` の 3 引数（`commit_input` / `commit_template_mode` /
`commit_template_inputs`）と局所 `active_wip` が dead になったため、呼び出しパスの
最小修正として `render.rs` の対応する hoist（3 ローカル）と `render_body` の引数を削除。
ロジック変更なし（`render.rs:190/196` の `self.` 直接参照は従来どおり）。

### B. modal_renderers.rs（3395 → 3117 LOC）の共通カード抽出

12 個のモーダル renderer が逐語コピーしていた **フルスクリーンオーバーレイ筐体**
（半透明・`occlude` 背景レイヤ + 中央寄せ flex カラムにカードを載せる二層構造）を
`modal_overlay(card: impl IntoElement) -> gpui::Div` 1 関数に抽出し、全 12 箇所を
`modal_overlay(card)` 呼び出しに置換。`render_plan_modal_card` 内の同筐体も置換した
（プラン系カードの内容ビルダ自体は既に共有済みだったため、今回はオーバーレイ筐体を
1 箇所化）。

- **同一 DOM を担保**: 抽出した DOM は元の inline と要素・属性・順序が完全一致
  （`size_full().absolute().top_0().left_0()` ルート → `occlude().bg(modal_overlay).opacity(0.65)`
  背景 → 中央寄せカラム → card）。
- **戻り値を `gpui::Div`** にしたのは、discard モーダルだけがルートに
  `.on_key_down(esc_cancel)`、card に `.occlude()` を追加していたため。
  `modal_overlay(card.occlude()).on_key_down(esc_cancel)` で再現できる
  （イベントハンドラは子要素リストとは独立に保持されるので、`.child()` との
  チェイン順序は描画ツリーに影響しない）。`focusable_card` を渡す create-branch /
  create-worktree / stash-push 系も card スロットに渡すだけで不変。
- 1 モーダル = 1 関数の機能境界は維持。各 renderer は約 25〜30 行のオーバーレイ
  ボイラープレートが消え、カード組み立て本体のみへ縮んだ（80 LOC ソフト目標へ前進。
  プラン未使用の大型フォーム系はまだ 80 行超だが、本チケットの責務外の入力ロジック）。

modal_renderers.rs は 3117 LOC でまだ 800 LOC 超。共通カード抽出は完了したが
800 LOC 達成にはモーダル群の sibling 分割（checkout/pull-push/stash/conflict/update 系）が
別途必要で、これは後続作業に委ねる（本コミットはオーバーレイ抽出のみで純粋に
振る舞い不変に留めた）。

### 検証

- `cargo build --workspace` green（**新規警告 0**）。
- `cargo test --workspace` 全パス（44 グループ ok / 0 failed）。
- `cargo fmt --check` clean。
- `cargo clippy --workspace` 新規警告なし（`render.rs:650` の `nonminimal_bool` と
  `render_helpers.rs:459` の doc-continuation は移動前から存在する既存 debt で、
  当方の差分は当該行に触れていない）。
- `[kagi]` 契約行・i18n（Msg）参照は無変更（diff 上 klog 行 0）。
- **UI 目視検証 pending**（サブエージェントは GUI を起動できないため、
  コミットパネル / ファイル履歴 / 全モーダルの見た目は人手確認が必要）。
