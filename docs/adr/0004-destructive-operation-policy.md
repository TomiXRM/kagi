# ADR-0004: 破壊的操作ポリシー

- Status: Accepted
- Date: 2026-06-12

## Context

本プロダクトの最重要価値は「ローカル repo を壊さない」こと。操作を危険度で分類し、扱いを固定する。

## Decision

### 操作分類

| クラス | 定義 | 例 | 扱い |
|--------|------|----|------|
| Safe | commit も working tree の内容も失わない | branch 作成, stash push, stash apply, fetch | plan 表示 + 実行 |
| Guarded | 条件次第で working tree の変更を失いうる | checkout, merge, cherry-pick, stash pop, pull | plan + **preflight で dirty 状態を検査**。変更を失う可能性があれば blocker として実行拒否(stash を提案) |
| Destructive | commit / 変更を意図的に捨てる | reset(全モード), rebase, branch -D, stash drop | **MVP では実装しない。** 導入時は backup ref 自動作成 + 二重確認 + undo 手順提示を必須とする |
| Forbidden | 当面 GUI から提供しない | reset --hard, git clean, force push | 実装しない(コードベースに該当 API を置かない) |

### 全操作共通パイプライン(architecture.md §5)

1. **plan**: 実行内容・現在状態・予測状態・警告・blocker・復旧手順を生成し表示
2. **confirm**: ユーザーの明示的操作。デフォルトボタンはキャンセル
3. **preflight**: 直前に snapshot 再取得。plan 時と状態が変わっていたら中断・再 plan
4. **execute**: 単一操作のみ。連鎖実行しない
5. **verify**: 実行後 snapshot を予測と照合。乖離があれば警告 + 復旧手順
6. **log**: 前後状態の要約を operation log に永続化

### 開発時ルール

- テストは生成した fixture repo に対してのみ書き込みを行う
- fixture 生成スクリプトは tempdir 配下のみに作成し、パスを assert する

## Consequences

- 「checkout で変更が消えた」系の事故を構造的に防げる。
- 一部ユーザーには過保護に感じられる(将来 "expert mode" を検討する余地はあるが、デフォルトは常に安全側)。
- undo 機能(v1.0)は reflog + backup ref を前提に設計する。
