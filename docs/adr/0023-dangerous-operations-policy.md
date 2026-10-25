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
root Return は既定アクションなしで sheet と arm を保持し、Esc / Cmd-. と Cancel は無変更で閉じる。二段 arm は実行時 guard の
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

ここで「到達不能になる commit 数」は通常 ref の到達可能性であり、新規作成前か
既存かを問わず backup / snapshot / recovery / oplog 専用 ref を保持先に数えない。
0 件を「未 merge」というだけで再確認しない。1 件以上は従来の二段確認を維持する。
Discard は backup から内容を完全復元できる一方、branch 削除は recovery ref を
知らないと履歴を見失うため、この到達可能性も確認基準とする。実行前にも承認した
到達不能 commit 数を再検査し、通常 ref の削除・移動で変化したら typed refusal と
して新しい plan の確認を要求する。
到達性を支える通常 ref と symbolic chain は最終検査から削除完了まで lock する。
libgit2 の transaction commit は変更しない ref の lock を削除対象より先に解放し得るため、
保持先の lock は別の未 commit guard transaction に置き、削除 transaction の完了後に
解放する。最終計算へ未 lock の新しい保持先を紛れ込ませず、必要な lock を取得できなければ
副作用前に拒否する。
文言は条件に一致させ、回復可能な plan に「取り消せません」
「Permanently」を併記しない。force-with-lease の復元も remote が再び動いていない
場合の条件付き操作であり、他者の取得済み履歴までは元に戻せないため二段のままとする。
EN/JA の同一条件で同じ確認段数を使い、JA に英語の追確認説明を残さない。

「未公開 Amend」は upstream 設定の有無から推定しない。設定済み upstream に加え、
すべての remote-tracking ref の履歴に HEAD が含まれていないことを検査する。
upstream が未設定・解除済みでも remote-tracking ref が HEAD またはその子孫を
指していれば公開済みとして二段確認を維持し、protected branch の blocker も維持する。
未公開の承認では実行入口で既存の remote-tracking / upstream ref と symbolic chain を
transaction lock し、lock 内で公開判定を再検査する。typed refusal は Git の実行前に返し、
lock は既存の Git CLI による hooks / signing / HEAD 更新が終わるまで保持する。

ただし Git / libgit2 に ref namespace 全体の lock API はない。列挙時に存在しなかった
remote ref の新規作成（hook 内で新規 ref を作る拡張ケースを含む）はこの排他の対象外とする。
既存 ref の更新・削除に対する guard を、新規 namespace 追加も防ぐ大域的な保証と表示しない。

