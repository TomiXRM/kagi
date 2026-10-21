# ADR-0023: Dangerous Operations Policy

- Status: Accepted / Date: 2026-06-12

## Decision

操作を5カテゴリに分類し、UI とパイプラインの扱いを固定する:

| カテゴリ | 例 | plan | 確認 | 表示 |
|----------|----|------|------|------|
| read-only | Copy / Compare / Show details | 不要 | なし | 通常 |
| safe-create | branch / worktree / tag 作成 | 必要 | 1段階 | 通常 |
| history-additive | cherry-pick / revert / merge / commit | 必要 | 1段階 + dirty 警告 | 通常 |
| wt-mutating | checkout(branch/commit)/ stash pop / pull | 必要 | 1段階 + dirty 警告 | 通常 |
| **history-rewriting** | reset / amend / rebase / force-with-lease push | 必要 + `destructive: true` | 下記の回復可能性・影響範囲で判定 | Advanced/Dangerous 配下 |

- **2段階確認**は「実効的な復旧手段がない、他者に影響する、または backup があっても
  local branch 削除で通常の ref から到達不能になる commit が存在する」場合に限定する。
  `OperationPlan.destructive` は変更の性質であり、二段 arm の根拠とは分離する。
  plan の確認→追確認（対象・失われるもの・条件を表示）とし、タイプ入力は要求しない。
- destructive plan は実行前に **現在 HEAD の SHA を必ず oplog に記録**(recovery の起点)
- **force push は実装しない・提案しない**(既存規約の再確認。コードベースに存在禁止)
- **Revert の安全設計**(T-CM-033 の決定): revert は history-additive(新 commit を作るだけ)。
  destructive ではない。merge commit の revert は parent 選択 UI ができるまで disabled。
  dirty WT では warning(in-memory 方式により conflict 時は repo 無傷で Refused)
- 上記カテゴリは MenuItem.dangerous / OperationPlan.destructive に反映し、
  build_commit_menu の unit test でカテゴリ妥当性を固定する

## Consequences

- 既存操作の再分類: undo_commit(ref-only soft)は history-rewriting だが
  「未 push のみ」blocker により実質安全 — 既存どおり 1段階のまま(変更しない)
- 将来 rebase 等を足すときはこの表に追記してから実装する

## Amendment — 2026-10-21: 回復可能な操作の確認 (#1695)

一段確認も plan を省略せず、明示的に選んだ Confirm だけが実行を承認する。
root Return は Cancel、Esc と Cancel は無変更のまま。二段 arm は実行時 guard の
代替ではなく、どちらの段数も plan → preflight → execute → verify → oplog を守る。
identity / tip / digest / lease の再検査と blocker は緩和しない。

| 操作・凍結された計画上の条件 | 確認 | 表示する結果と復旧可能性 |
|---|---|---|
| Reset（ref-only soft） | 一段 | 対象 branch と commit、index・作業ファイルの保持、記録した旧 tip への ref-only 復元 |
| 未公開 Amend | 一段 | 新 commit への置換、旧 commit は reflog から復元可能 |
| backup 付き Discard | 一段 | 選択した作業ファイルを戻す。ref-backed backup を receipt に保持し、バックアップ失敗時は削除しない（ADR-0179） |
| local branch 削除、他の ref から到達不能になる commit が 0 件 | 一段 | local ref のみ削除、commit は他の ref に残り復元用 ref にも保持 |
| local branch 削除、到達不能になる commit が 1 件以上 | 二段を維持 | 失われる通常の参照と commit 数、必須の復元用 ref |
| force-with-lease / remote 破壊 / 公開済み履歴の書き換え | 二段を維持 | 他者への影響。force-with-lease の最終確認は remote branch と承認時の正確な期待 tip を表示 |
| backup なしの削除 | 二段を維持 | 復元手段がないことと削除対象 |

ここで「到達不能になる commit 数」は recovery ref を作る前に計算する通常 ref の
到達可能性であり、0 件を「未 merge」というだけで再確認しない。1 件以上は従来の
二段確認を維持する。Discard は backup から内容を完全復元できる一方、branch 削除は
recovery ref を知らないと履歴を見失うため、この到達可能性も確認基準とする。
文言は条件に一致させ、回復可能な plan に「取り消せません」
「Permanently」を併記しない。force-with-lease の復元も remote が再び動いていない
場合の条件付き操作であり、他者の取得済み履歴までは元に戻せないため二段のままとする。
EN/JA の同一条件で同じ確認段数を使い、JA に英語の追確認説明を残さない。

