# ADR-0063: Conflict Dashboard UX(Right Panel)

- Status: Accepted(2026-06-13)
- 関連: requirements-conflict-ux.md §2.3 / ADR-0058(用語)/ 0066(marker safety)

## Decision

Conflict Mode 中、Right Panel は通常の Commit Inspector から **Conflict Dashboard** に切り替える:

- ヘッダ: `Merge conflicts detected`(op 別文言)+ operation summary(ADR-0058 の方向文言)
- **Current / Incoming の badge**(役割名 + 実名。ours/theirs は出さない、tooltip で補足)
- conflicted count / resolved count
- **Path / Tree toggle**(MVP は Path のみ実装、Tree は v0.2)
- **Conflicted Files** セクション(type badge: both modified / rename-delete 等、ADR-0065)
- **Resolved Files** セクション(解決候補に移ったファイル)
- selected file preview(クリックで Conflict Editor、ADR-0064)
- アクション: Abort(常時)/ Continue(ADR-0067 のゲート)/ Skip(sequencer のみ)/ external tool(ADR-0060)

### Mark resolved 系(GitKraken の Mark All Resolved は不採用)
- `Mark selected file resolved`: marker 無し & index で解決可能なファイルのみ
- `Mark all clean files resolved`: marker 無し & unmerged index でない clean なものだけ一括
- `Mark all resolved`: **Advanced 扱い**。marker 検出 + unmerged index 確認を通過した時のみ許可
- いずれも ConflictOperationPlan 経由(ADR-0067)。直接 index を書かない

W30 はファイル単位 choose までを実装済み。本 ADR は Dashboard 化 + Resolved セクション +
Mark resolved 系 + Path/Tree の足場を定義する。
