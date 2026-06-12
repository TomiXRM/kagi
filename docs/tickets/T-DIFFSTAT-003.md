# T-DIFFSTAT-003: DiffstatMiniBar の segment 計算ロジックを実装する

- Status: todo
- 依存: T-DIFFSTAT-001
- 関連: requirements-diffstat.md「Diffstat Mini Bar 仕様」「計算仕様」

## スコープ

- 純関数 `bar_segments(additions, deletions, max_segments) -> (green: usize, red: usize)`
- max_segments は 5〜8(定数)。比率配分 + **追加/削除が存在すれば最低 1 segment** 保証
- total=0 → (0,0)(UI 側で placeholder)

## 完了条件

- [ ] 要件の例(+10-0 / +0-10 / +5-5 / +1-20 / +200-10)を unit test で固定
- [ ] `cargo test` 全パス、own-code warning 0

## 実装メモ (done)

- Status: done
- 純関数 `bar_segments(additions, deletions, max_segments) -> (green, red)`。total=0→(0,0)。片側のみ→全segment。両側ありは各最低1 + 残りを additions 比率で四捨五入配分。`green+red <= max_segments` を保証。
- 要件の 5 例(+10-0 / +0-10 / +5-5 / +1-20 / +200-10)を unit test で固定 + 0..50×0..50×max1..8 の網羅 property test。全 green。
