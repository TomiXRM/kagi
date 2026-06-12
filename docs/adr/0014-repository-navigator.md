# ADR-0014: Repository Navigator Sidebar

- Status: Accepted / Date: 2026-06-12

## Decision
- セクション: LOCAL BRANCHES / REMOTE BRANCHES / TAGS / STASHES(順固定)。各セクションは折りたたみ可 + 件数表示。WORKTREES は v0.2、PR/ISSUES は later
- **filter 入力**(先頭固定): 全セクションを部分一致で絞り込み(gpui-component Input を流用)
- local branch 行: current は ✓ + 強調。upstream があれば `↑a ↓b` を行内に併記
- click = jump(既存)/ double-click = checkout(既存)/ **context menu(右クリック)**: Checkout / Create branch here / Delete branch(merged のみ、plan 経由 — backend W2-DELETE)
- 実装はモジュール分離(`src/ui/sidebar.rs` へ抽出)し、mod.rs の肥大を止める

## Consequences
- 状態追加: collapsed sections / filter 文字列(KagiApp 保持、reload で維持)
