# ADR-0005: conflict 取り扱いポリシー

- Status: Accepted
- Date: 2026-06-12

## Context

merge / cherry-pick / stash apply は conflict を起こしうる。MVP の範囲で conflict をどう扱うかを決める。

## Decision

### MVP: 「conflict を起こさせない」を基本戦略とする

1. **事前予測**: cherry-pick は libgit2 の in-memory merge で conflict を予測し、conflict が出る場合は plan に **blocker** として表示して実行しない(「実行したら conflict になります」を見せるのが MVP の価値)。
2. **検出と可視化**: 万一 repo が conflict 状態(他ツール起因含む)なら、status に conflicted files を明示し、書き込み操作を全てブロックする。
3. **解決手段の案内**: MVP では GUI 内解決を提供しない。「外部エディタで解決して `git add` → continue / abort」の手順と、`abort` 相当(`cherry-pick --abort` 等、元の状態に戻る安全側操作)のみボタン提供する。abort は preflight で「abort で失われるものがない」ことを確認してから実行。

### v0.2 以降

- merge 導入時: conflict 発生を許容し、conflicted file の一覧 + ours/theirs の選択式解決を追加。
- v1.0: 3-way 表示 + 手動編集の conflict 解決 UI。

## Rationale

conflict 解決 UI は工数が大きく品質リスクも高い。MVP の差別化は「解決」ではなく「**事前に知らせて回避させる**」こと。abort 経路さえ安全なら、解決は外部エディタに委ねても repo は壊れない。

## Consequences

- MVP では conflict する cherry-pick を実行できない(意図的制約)。ユーザーには「先に merge/rebase 元を更新する」等の回避策を plan 内で提示する。
- conflict 状態の repo を開いても安全(読み取り専用化 + 案内)であることをテストする。
