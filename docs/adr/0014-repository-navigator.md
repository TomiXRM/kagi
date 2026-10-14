# ADR-0014: Repository Navigator Sidebar

- Status: Accepted / Date: 2026-06-12
- Graph の section 配置と scroll ownership は ADR-0217 で更新。

## Decision
- セクション: LOCAL BRANCHES / REMOTE BRANCHES / TAGS / STASHES(順固定)。各セクションは折りたたみ可 + 件数表示。WORKTREES は v0.2、PR/ISSUES は later
- **filter 入力**(先頭固定): 全セクションを部分一致で絞り込み(gpui-component Input を流用)
- local branch 行: current は ✓ + 強調。upstream があれば `↑a ↓b` を行内に併記
- click = jump(既存)/ double-click = checkout(既存)/ **context menu(右クリック)**: Checkout / Create branch here / Delete branch(merged のみ、plan 経由 — backend W2-DELETE)
- 実装はモジュール分離(`src/ui/sidebar.rs` へ抽出)し、mod.rs の肥大を止める

## Consequences
- 状態追加: collapsed sections / filter 文字列(KagiApp 保持、reload で維持)

## Amendment — 2026-10-14: search cancellation and Clear accessibility (#1547/#1548)

Nonempty search consumes Escape by clearing only its query and retaining the
Input's focus. Empty search Escape retains the existing exit to the local list.
IME marked-text cancellation is consumed by Input before the containing
workspace's Escape handler runs. The Input's optional Clear button receives its
own localized EN/JA action name, distinct from “Filter sidebar”; native AXPress
uses the existing clear-and-refocus callback and empty inputs expose no Clear.
