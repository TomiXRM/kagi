# ADR-0017: Bottom Panel and Terminal Behavior

- Status: Accepted / Date: 2026-06-12(ADR-0007/0008 の補強)

## Decision
- default 高さ = **viewport の 18%**(≤20% 要件。最小 80px / 最大 60% は既存維持)。セッション内リサイズ記憶は既存維持、再起動間の永続化は設定ファイル導入時(later)
- Terminal: session 保持・cwd=root・出力の完全解析はしない(既存)。state refresh は **file watch(T029)を主**とし、補助として手動 Refresh。focus-out/command-exit フックは追加しない(watcher で十分、複雑化回避)
- 失敗時 Operation Log 自動オープン(既存)。Operation Log の各エントリは plan 概要・結果・失敗理由・復旧ヒント(recovery)を展開表示(既存 + recovery 表示を追補)

## Consequences
- B-1(default 18%)のみ即時変更。他は現状維持を正式仕様化
