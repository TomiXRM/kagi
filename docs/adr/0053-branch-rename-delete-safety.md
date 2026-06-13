# ADR-0053: Branch Rename/Delete Safety

- Status: Accepted(2026-06-13)

## Decision

### Rename
- **local branch のみ**。`git check-ref-format` 相当の validation(git2 Branch::rename が検証)
- **current branch の rename は許可**(`git branch -m` 同等の ref-only 操作で安全。
  HEAD の symbolic ref も追従させる)。dirty でも safe(WT 不変)だが R6 に従い warning 表示のみ
- upstream tracking 設定(branch.<name>.*)は新名へ引き継ぐ。**remote branch 名は自動 rename しない**
  (plan に「remote 上の名前は変わらない」を明示)
- gh 重複キー問題(2026-06-13 の delete バグ)と同型の config 移行に注意: 寛容な
  read→rewrite で移すこと

### Delete
- 既存 plan_delete_branch / execute_delete_branch(ADR-0014: merged-only guard、unmerged=blocker、
  ref-only)を**そのまま**呼ぶ。current branch は availability で disabled
- remote へ push 済みかを plan に表示。remote branch delete は MVP 外
  (Advanced/Dangerous、ネットワーク破壊操作なので ADR-0040 案C 系の隔離フローを別 ADR で)
