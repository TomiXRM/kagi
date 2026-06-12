# T-DIFFSTAT-004: DiffstatMiniBar gpui component を実装する

- Status: todo
- 依存: T-DIFFSTAT-003
- 関連: requirements-diffstat.md

## スコープ

- `src/ui/diffstat_bar.rs`(新規): 固定幅 mini bar(緑/赤 segment、theme() semantic color、ハードコード禁止)
- `+N -M` 数値表示(右揃え・桁揃え/monospace)込みの行末ユニットとして使える関数 or component
- BIN / placeholder(変更 0)対応

## 完了条件

- [ ] 全 6 テーマで成立(dark/light 両方で視認)
- [ ] `cargo test` 全パス、own-code warning 0

## 実装メモ (done)

- Status: done
- `src/ui/diffstat_bar.rs` に `diffstat_unit(id_seed, Option<&FileDiffStat>) -> impl IntoElement`。`+N -M` 数値(緑=additions/赤=deletions)+ 固定幅5segment bar(緑→赤→muted残track)を右揃え・flex_shrink_0 で返す。
- 色は全て theme()(change_added / change_deleted / surface / text_muted)。ハードコードなし→6テーマ成立。
- binary→`BIN`、変更0→薄い placeholder track、None→空spacer(列 alignment維持)。
- MAX_SEGMENTS=5(spec 5–8 内)。
