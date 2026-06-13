# ADR-0070: Conflict Editor Scroll Synchronization Policy

- Status: Accepted(2026-06-13)
- 関連: ADR-0069 / requirements-conflict-ux.md v2 §4

## Decision

- **A / B pane は縦スクロールを同期**(同じ hunk 行が左右で揃って見えるように)。MVP は
  「行オフセット連動」(A をスクロールしたら B を同オフセットへ)。厳密な hunk アライン(行数差の
  吸収)は v0.2。
- Result pane は独立スクロール(A/B とは行数が違うため)。
- 横スクロールは各 pane 独立。
- 実装: gpui-component InputState のスクロール位置 API を使えれば連動、無ければ共有
  ScrollHandle で近似。MVP で同期が難しければ「同期なし(各独立)」に落としてよい(その旨ログ/ticket 明記)。
- pane resize(A|B 比率、A・B / Result 比率、bottom panel 境界)は W7 の measured-bounds +
  Rc<Cell> ドラッグ方式(inspector split と同型)を流用。
