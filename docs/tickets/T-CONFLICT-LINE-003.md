# T-CONFLICT-LINE-003: file/chunk/line チェックの tri-state UI と相互作用

- Status: todo(実装は flow レーン merge 後)
- 仕様: ADR-0071

## スコープ
- 3 階層チェックの視覚(全=☑ / 一部=— / 無し=☐)と親子伝播。
- 両採用時の順序トグル(CurrentFirst/IncomingFirst)を chunk 単位に。
- marker 残存・未解決の評価を line 採用後に再計算(Save/Continue gate と整合)。
