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
| **history-rewriting** | reset / amend / rebase / force push | 必要 + `destructive: true` | **2段階** | Advanced/Dangerous 配下、赤 + ⚠ |

- **2段階確認**: plan モーダルの Confirm(赤)→ 追確認(操作名の再表示 +
  「失われるもの」の列挙 + 明示クリック)。タイプ入力確認は過剰なので採らない
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
