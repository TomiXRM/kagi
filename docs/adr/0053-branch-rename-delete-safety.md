# ADR-0053: Branch Rename/Delete Safety

- Status: Accepted(2026-06-13)

## Decision

### Rename
- **local branch のみ**。`git check-ref-format` 相当の validation を実行し、ref-only の `Reference::rename` を使う。
- **current branch の rename は許可**(`git branch -m` 同等の ref-only 操作で安全。
  HEAD の symbolic ref も追従させる)。dirty でも safe(WT 不変)だが R6 に従い warning 表示のみ
- upstream tracking 設定(branch.<name>.*)は新名へ引き継ぐ。**remote branch 名は自動 rename しない**
  (plan に「remote 上の名前は変わらない」を明示)
- #1129 (2026-10-11): repo-local の exact `branch "<old>"` subsection だけを移す。`foo.bar` は `foo` の設定ではない。複数値の順序・quotes／escapes・未知のキーを含む全値を保持するため、Git の `config --file <origin> --rename-section` で header だけを変更する。origin は canonical gitdir／commondir 内だけに限定し、local include でも共有／作業ファイルや外部へ向く symlink は plan blocker とする。global／system は列挙も変更もしない。worktreeConfig が有効なら `config.worktree` も同じ規則で移す。既存 destination 設定は消さない。
- plan は移動キーと source／destination の ordered values・include origin の digest を保持する。preflight と execute が再照合し、確認後の変更や identity 欠落は ref 書込み前に拒否する。完了後は config 値と ref を検証する。ref rename 後の config 書込み失敗は観測した after-state を一件の Partial receipt に残す。
- 対象名に一致する `includeIf onbranch:` は HEAD の rename で参照が消えるため plan blocker とする。各 origin の全 header／values は repository 外の temporary copy で Git の実 rename と parsed read-back を検証してから ref を変更する。case-variant 等で header を安全に移せない場合も typed EN／JA blocker とし、予見可能な問題を Partial にしない。
- linked worktree の local branch rename も同じ経路を使い、`Reference::rename` が全 worktree の HEAD を追従させる。別の worktree-branch rename／remote rename executor は存在しない。

### Delete
- 既存 plan_delete_branch / execute_delete_branch(ADR-0014: merged-only guard、unmerged=blocker、
  ref-only)を**そのまま**呼ぶ。current branch は availability で disabled
- remote へ push 済みかを plan に表示。remote branch delete は MVP 外
  (Advanced/Dangerous、ネットワーク破壊操作なので ADR-0040 案C 系の隔離フローを別 ADR で)
