# ADR-0019: GitStateSnapshot Refresh Policy

- Status: Accepted / Date: 2026-06-12

## Decision
snapshot(RepoSnapshot)を更新するトリガを以下に限定する:
1. 起動時 / 2. 自アプリの操作成功後(各 confirm_* の reload)/ 3. **.git watcher(T029。外部変更・terminal 内 git を包括)**/ 4. 手動 Refresh
focus-in/out・タイマーポーリング・terminal 出力解析は採用しない(watcher で必要十分、誤発火と複雑化を避ける)。
表示の鮮度は Status Bar の最終 refresh 時刻で明示(ADR-0010)。reload は当面同期(大 repo での非同期化は別 ADR で扱う)。

## Consequences
- 新しい更新経路を足す場合は本 ADR を改訂してから
