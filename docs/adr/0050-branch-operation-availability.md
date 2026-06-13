# ADR-0050: Contextual Branch Operation Availability

- Status: Accepted(2026-06-13)

## Decision

- 入力を凝集した `BranchMenuContext`(branch kind / is_current / has_upstream / ahead / behind /
  dirty / conflict_mode / protected / checked_out_in_other_worktree / merged_into_current /
  is_pushed / detached_head / busy(operation in progress))を snapshot から**事前計算**して渡す。
  UI からの直接 git 判定禁止
- 方針: **「ありえない」項目は非表示、「今はできない」項目は disabled + 理由文字列**
  (commit context menu の disabled tooltip と同型)。busy 中は実行系すべて disabled
- ahead/behind の count は item label に埋め込む(`Pull ↓3`)。0 の場合 Pull/Push は
  no-op であることを label で示す(例 `Pull (up to date)` disabled)
- protected branch 判定は ADR-0040 案C と同じ集合(main / master / develop / release 系)
- 純粋関数 + `tests`(T-BCM-070〜073)で表を固定する
