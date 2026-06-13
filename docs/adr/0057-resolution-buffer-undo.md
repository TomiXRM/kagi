# ADR-0057: Resolution Buffer と Undo(repo を汚さない解決)

- Status: Accepted(2026-06-13)

## Decision

- choose(current/incoming/both-ordered)・手編集は **WT/index に触れない解決バッファ**上の操作。
  ファイルごとに Result 草稿 + 操作履歴(undo/redo)。draft(ADR-0042)と同じ
  `~/.kagi/conflicts/<repo-hash>/` へ 250ms debounce 自動保存 → 中断・再開可能
- continue 時にのみ WT へ書き出し+stage。**abort してもバッファは oplog 参照付きで退避**
  (jj の「部分解決を失わせない」の git 互換実装)
- 行ごとの採用元(current/incoming/manual)を保持し UI で出所可視化(BC/KDiff3 流)
