# T-CONFLICT-LINE-003: file/chunk/line チェックの tri-state UI と相互作用

- Status: done
- 仕様: ADR-0071

## スコープ
- 3 階層チェックの視覚(全=☑ / 一部=— / 無し=☐)と親子伝播。
- 両採用時の順序トグル(CurrentFirst/IncomingFirst)を chunk 単位に。
- marker 残存・未解決の評価を line 採用後に再計算(Save/Continue gate と整合)。

## 実装メモ (done)
file/chunk/line checkbox は All/Partial/None を `☑` / `—` / `☐` で表示し、上位 checkbox の
操作は下位 line selection に伝播する。line/chunk/file/order の各操作は Result を即時 reassemble し、
既存の buffer status / marker residue / autosave path を通るため Save/Continue gate と整合する。
MVP は CurrentFirst / IncomingFirst の chunk order toggle まで。ドラッグ複数行選択と Interleaved は deferred。
