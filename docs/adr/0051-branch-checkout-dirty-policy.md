# ADR-0051: Branch Checkout and Dirty Working Tree Policy

- Status: Accepted(2026-06-13)

## Decision

- Context Menu の Checkout は**既存の checkout フローをそのまま使う**: safe checkout のみ、
  dirty 時は plan に warning + **auto-stash 提案**(Enter-checkout で実装済みの
  stash_before_checkout 機構)。conflict 予測(W15 の predict_checkout_commit_conflict 同型)で
  重なりがあれば blocker
- remote branch の checkout は「local tracking branch 作成 → checkout」を 1 plan に統合
  (ADR-0055)。detached checkout は Advanced(later)
- 別 worktree で checkout 済みの branch は checkout を blocker(git 自体が拒否する。理由表示)
