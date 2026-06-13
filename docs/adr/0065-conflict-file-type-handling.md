# ADR-0065: Conflict File Type Handling

- Status: Accepted(2026-06-13)
- 関連: requirements-conflict-ux.md §3.2 / research/conflict-ux-models.md(index stage 1/2/3)

## Decision

conflict file に **type** を付け、UI(badge)と解決動線を type で分岐する:

| type | 判定(index stage 1/2/3 + status)| MVP の扱い |
|------|-----------------------------------|------------|
| both modified | stage 2 & 3 あり、両者 blob | **Conflict Editor(hunk 単位)** 最優先 |
| added by both | stage 1 無し、2 & 3 あり | Editor or 片側選択 |
| deleted by current | stage 1 & 3、stage 2 無し | 専用選択 UI(keep deleted / restore incoming) |
| deleted by incoming | stage 1 & 2、stage 3 無し | 専用選択 UI(keep current / accept deletion) |
| modified/delete | 片側削除・片側変更 | 専用選択 UI |
| rename/delete | 片側 rename・片側 delete | 専用選択 UI(MVP は外部ツール推奨) |
| rename/rename | 両側で別名 rename | 外部ツールへ逃がす(MVP) |
| binary | いずれかが binary | choose-only(Editor では開かない) |
| submodule | gitlink conflict | 外部ツール/手動(MVP は表示 + 逃がす) |

- **MVP は both modified を最優先**で Editor に流し、それ以外は専用選択 UI or 外部ツール(ADR-0060)へ。
  無理に GUI で抱え込まない(VSCode/GitKraken が苦手とする領域 — research 参照)
- type は W26 の ConflictKind を拡張(現状 Content/RenameDelete/ModifyDelete/Binary → 上表へ細分)
