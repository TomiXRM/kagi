# ADR-0064: Conflict Editor Layout

- Status: Accepted(2026-06-13)
- 関連: requirements-conflict-ux.md §2.4/2.5 / ADR-0057(buffer)/ 0058(用語)/ 0066(marker)

## Decision

conflicted file クリックで **専用 Conflict Editor** に入る。MVP レイアウト:

```
Top Toolbar: [file path] [conflict n of m] [< prev] [next >] [Open external tool] [Reset] [Save]
Upper Split:  A = Current branch side        |  B = Incoming side
Lower:        Result / Output preview(由来 side を行ごとに明示)
```

各 hunk について(checkbox ではなく **文言の明確なボタン**):
- `Accept current` / `Accept incoming`
- `Accept both: current then incoming` / `Accept both: incoming then current`
- `Edit result`(手編集)/ `Reset this hunk`

### Result/Output は一級
- A/B 選択で Result Preview が**即更新**、各行の由来(current/incoming/manual)を表示(ADR-0057 の provenance)
- 未解決 hunk が残っていれば明示。marker 残存は **保存不可 or 強警告**(ADR-0066)
- Save 前に result diff を確認でき、Save 後はファイルを resolved candidate に移す(index へ書くのは
  continue 時。Save は buffer 永続化 + WT 反映候補。実装方式は ADR-0057 の buffer に従う)

MVP は both modified の hunk 単位。syntax highlight / inline editor / hunk ごと undo / minimap /
semantic 区切りは v0.2(§5)。
