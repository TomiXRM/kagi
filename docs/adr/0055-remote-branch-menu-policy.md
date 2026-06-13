# ADR-0055: Remote Branch Context Menu Policy

- Status: Accepted(2026-06-13)

## Decision

- remote branch への「Checkout as local branch」=「**local tracking branch を作って checkout**」を
  1 つの plan に統合(branch 名既定 = remote 名から remote prefix を除いたもの。既存 local と
  衝突する場合は blocker + 名前入力)
- detached HEAD にする checkout は Advanced(later)
- Merge remote into current / Rebase current onto remote は local と同じ ADR-0052 の意味論
  (merge は remote-tracking ref を target にする)
- Delete remote branch は **MVP 外**: `push --delete` はネットワーク破壊操作のため、
  ADR-0040 案C(force-with-lease)と同じ隔離・段階確認ファミリーとして将来 ADR で設計
- Copy 系は remote 名そのまま(`origin/feat/x`)
