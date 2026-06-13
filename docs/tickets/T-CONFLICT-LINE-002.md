# T-CONFLICT-LINE-002: A/B pane を行リスト + 左チェックボックス化

- Status: todo(実装は flow レーン merge 後)
- 仕様: ADR-0071 / ADR-0069 改訂

## スコープ
- A/B pane を InputState CodeEditor → uniform_list の行 row(左 line checkbox + 行番号 + monospace code)へ。
- chunk checkbox(hunk header)+ file checkbox(toolbar/pane header)。tri-state 連動。
- 選択で Result 即更新。scrollbar 標準。Result-edit のみ InputState 維持。
- A/B 縦スクロール同期(shared ScrollHandle、ADR-0070)を行リストで実装。
