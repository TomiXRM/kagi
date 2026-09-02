# docs/research — 調査ドキュメント索引

Kagi の設計判断の前段にある調査記録。ADR が「決めたこと」なら、ここは「決める前に何を見たか」。

## 2026Q3 サーベイ（AI native 化 / 人間に優しい化）

2026-09-03 実施。5 スライスの外部サーベイと、その統合。

| ファイル | スライス | 規模 |
|---|---|---|
| [ai-native-roadmap-2026q3.md](./ai-native-roadmap-2026q3.md) | **統合・ギャップ分析・ロードマップ・Issue 索引** | 起点はここ |
| [survey-2026q3-git-gh-features.md](./survey-2026q3-git-gh-features.md) | git 2.40〜2.55 / GitHub・gh CLI 2.99 の新機能 | 643 行 |
| [survey-2026q3-git-clients.md](./survey-2026q3-git-clients.md) | git クライアント 47 製品（jj / Sapling / GitButler / branchless 深掘り） | 479 行 |
| [survey-2026q3-ai-native-dev.md](./survey-2026q3-ai-native-dev.md) | AI native 開発の実運用と git の使い方、MCP | 461 行 |
| [survey-2026q3-worktree-managers.md](./survey-2026q3-worktree-managers.md) | worktree manager 24 ツールの機能比較 | 710 行 |
| [survey-2026q3-human-ux.md](./survey-2026q3-human-ux.md) | undo / diff 理解 / 学習コスト / a11y / 性能 | 489 行 |

各サーベイは同一の 5 節構成（1. サマリ / 2. 詳細 / 3. Kagi 取り込み候補表 / 4. 取り込まないと判断したもの / 5. 未解決の疑問）。
候補は合計 **136 件**。統合ドキュメントが 10 テーマに整理し、GitHub Issue に落としている。

## それ以前の調査

| ファイル | 内容 |
|---|---|
| conflict-ux-models.md / conflict-ux-gui-clients.md / conflict-ux-editors.md | コンフリクト UX の三部作（ADR-0056〜0071 の前段） |
| github-pr-integration.md | PR 連携の設計調査（ADR-0136） |
| gitbutler-reuse-research.md | GitButler 流用可否（ADR-0033） |
| jj-reuse-research.md | jj 流用可否（ADR-0032） |
| zed-gpui-reuse-research.md | Zed / GPUI 流用可否（ADR-0034） |
| gitcomet-comparison.md / rgitui-learnings.md / openlogi-learnings.md | 他クライアントからの学習 |
| gpui-component-audit.md | gpui-component の棚卸し（ADR-0006） |
| qa-audit-matrix.md | QA 監査マトリクス |
| reference/ | 一次資料のスナップショット |
