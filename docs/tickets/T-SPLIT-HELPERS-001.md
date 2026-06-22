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
