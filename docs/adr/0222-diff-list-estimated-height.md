# ADR-0222: Diff list の未計測行に推定高さと末尾 intent を使う（#1122）

- Status: **Accepted**
- Date: 2026-10-10
- Related: [ADR-0209](0209-pr-review-threads-on-diff.md)、#1122

## 文脈

可変高の diff list は全行を描画せずに必要な行だけを計測する。upstream GPUI の `list` は未計測行を高さ 0 として集計し、estimated-height API を持たないため、大きな diff の scrollbar が計測済み prefix だけを表し、大きなホイール入力でも実際の末尾に届かなかった。行の source identity を未変更 reload で交換すると、読んでいた位置も reset される。

## 当初の決定（2026-10-10、下記 #1479-S5C 改訂で supersede）

1. 公開 fork [TomiXRM/zed](https://github.com/TomiXRM/zed/commit/caf5007618c309a8ea66002d44a9c9ab629a2d35) の `caf5007618c309a8ea66002d44a9c9ab629a2d35`（branch `fix/kagi-list-estimated-height`、base `90b3aa0b3bd3b453775b11a386907c7ac9acd997`）を pin する。upstream PR 待ちの Kagi list fix であり、同じ Zed source の sibling crates もこの rev に統一し、一時的な absolute path patch を残さない。
2. `crates/kagi-ui-core/src/diff_list.rs` の `DiffListLayout::sync` から diff list だけが未計測行の推定高さと end intent に opt-in する。source / zoom / gutter の変更だけが reset を要求し、remeasure は位置を保つ。全行 eager render や第二の scroll model は作らない。
3. thread の展開・折りたたみは measured nodes を splice する。PR の同じ選択の reload と Editor の未変更 reload は adopt / install により source identity を保持する。幅による reflow は行内の fractional anchor を保持する。

## 帰結

- scrollbar range は未計測部分を含む推定 extent を反映し、実測に伴って更新される。
- 大きな末尾方向のホイール入力 1 回で真の最終行に到達する。最終行が viewport より高くても、その下端を viewport 下端に揃える。end intent はこの操作の解決であり、以後の変更に無条件で追従する設定ではない。
- thread 開閉、同じファイルの再クリック、未変更 Editor reload、幅変更で読んでいた位置を不必要に捨てない。別 source / zoom / gutter の変更とは区別する。

## 証拠と限界

当初の fork 変更の受入記録は GPUI unit **30/30**、native の次の **7 oracles PASS**（#1479-S5C の検証結果ではない）：
`pr_diff_extent_unified`、`pr_diff_extent_split`、`pr_diff_extent_reflow`、
`pr_diff_extent_threads`、`pr_diff_extent_source_recolor`、
`pr_diff_extent_same_selection`、`editor_diff_extent_unchanged_reload`。

genuine Before FAIL は GPUI regression
`test_estimated_end_resolves_taller_tail_without_following`、Editor oracle
（大きなホイール入力 1 回の後も tail が viewport 外）、same-selection oracle
（`tests/recovery/pr_diff_extent_same_selection.rs:260`、再クリックで位置 reset）
に記録されている。same-selection の Before は `pr_mode.rs` の 3 logic hunks
（same-file / commit short-circuits と reload の `highlight::install` / adopt）
だけを戻し、recorders を保持した。保存した Before patch を `git apply` して
同じ oracle を FAIL させ、`git apply -R` 後は PASS した。

geometry oracle は収まらない行や card に全体包含を要求しない：

- 通常 tail は全体が viewport 内。oversized tail は下端差 ≤ 1px かつ上端が viewport 上端より上。
- first content row と先行 hunk header の合計高さが viewport を超える場合、native item 0 / offset 0 に戻り、hunk header 上端と viewport 上端、first row 上端と header 下端の差が各 ≤ 1px、first row 下端が viewport 下端より下。header と first row が一緒に収まる場合は first row の全体包含を要求する。
- thread card が `viewport.height − badge.height` より高ければ badge は全体包含、card 上端は badge 下端 − 1px 以降、body 上端は viewport 内、body は card 内。収まる card と badge は両方全体包含。
- collapse は item count を 1 減らし、item 9 を code row に戻す（row 8 の高さとの差 ≤ 1px）。`pre.extent − collapsed.extent` は正で、少なくとも `expansion.height − viewport.height`。新しく露出する行の実測を許す。

これは状態と実 layout geometry の証拠であり、FPS / CPU の改善は主張しない。
Tier B の実アプリ比較確認は pending。

## 改訂 — 2026-10-15: Kagi-owned variable geometry（#1479-S5C）

slice 定義: [upstream migration #1479](../rearch/upstream-migration-1479.md)。

diff の geometry は `kagi_ui_core::variable_list::VariableListState` と
`variable_list` Element が所有する。Main Diff、File History、PR diff、
PR conflict、Editor WIP、Editor History の全 6 retained owner を同時に移す。
`DiffListLayout` は既存の source / split / gutter identity を引き続き管理し、
row renderer、selection、keyboard navigation、操作の承認境界は置き換えない。
fork の `ListState` 内部や独自追加 API に diff が依存する経路は残さない。
依存 pin 自体の削除は後続の official package cutover で行う。

- owner は source generation 内の row key、実測値と未計測 estimate、prefix
  index、viewport、reading key + 行内 fraction を保持する。visible + overdraw
  だけを構築し、`measure_all` の production 採用閾値は **0**。小件数 fallback、
  count-only threshold、全件 eager shaping は採用しない。
- source 差替えは reset。recolor、同じ選択、未変更 reload は実測 cache を保持。
  width / rem / font / line-height / split / gutter の metric 変更は再計測し、
  reading fraction を新しい実測高さへ解決する。
- same-count の画面外 review card 差替えでも対象の実測値を失効する。
  splice は新 slot に非ゼロ estimate を与え、survivor の reading key を保つ。
- 大きな wheel と thumb 終端は実最終行から viewport 分だけを計測する
  one-shot End。oversized tail の下端差は ≤ 1px。解決後の append で
  tail-follow を始めない。thumb drag 中の mapping extent は開始時に固定する。
- `bounds_for_item` は直近 frame で実際に paint した行だけを返す。
  distant cache の oracle は `measured_height` を使う。estimate を「測定済み」
  として返す互換 shim や画面外の仮想 paint bounds は作らない。

既存 7 native oracle の 20,005 行、単一巨大 wheel の往復、oversized tail、
source/recolor/reclick/reload の読み位置と distant measurement の強度を維持し、
実 pointer による thumb interior/end と画面外 review card 更新を追加する。
2026-10-15 の native slice gate は **16/16 PASS**（既存 7 extent oracle と
keyboard selection / split order / revision / hunk actions / threads / review
hunk / adaptive layout / lifecycle）。上記の過去 fork PASS や `measure_all`
の実験値は本実装の latency の証拠ではない。
新 owner の model contract 11 tests と workspace 3,424 tests は PASS
（workspace の既存 ignored 44 件）。`cargo build --locked` / plain build /
fmt / clippy / `uv run --project ci check-all` も PASS。clippy は未変更ファイルの
既存警告のみで、この改訂のファイルに新しい警告はない。

### fork の依存契約と新 owner の対応

移行時に `90b3aa0..caf5007` の `crates/gpui/src/elements/list.rs` 差分を読んだ。
移植対象はソースではなく、diff consumer が依存していた以下の契約である。

| 旧 fork の挙動 | Kagi owner の regression |
|---|---|
| 未計測 estimate が cold layout / reset / splice / 幅変更後も非ゼロ | `estimate_survives_reset_splice_and_empty_source`、`prefix_inverse_uses_actual_and_nonzero_estimated_slots`、native reflow |
| 同じ estimate / repaint で実測 cache を捨てない | `width_font_zoom_gutter_invalidation_resolves_fraction_against_actual_height`、native source_recolor / same_selection / unchanged_reload |
| 連続する metric 更新と未計測 anchor の fraction 保持 | `unmeasured_fraction_survives_metric_change`、width/font/zoom/gutter unit、native reflow |
| 同件数 source 交換 / 画面外 slot 交換 / 前方 splice の失効と survivor | `diff_list` の既存 2 unit、`offscreen_same_count_replacement_invalidates_only_affected_measurements`、`insert_delete_before_reading_preserves_key_and_fraction`、native threads |
| 大 wheel / scroll_by / thumb 終端が真の最終行を解決し append を follow しない | `end_resolution_is_one_shot_with_oversized_tail_and_no_append_follow`、`empty_end_does_not_become_follow_on_later_append`、native unified / split / reflow |
| thumb drag 開始時の extent 固定と prefix inverse | `drag_extent_is_frozen_and_endpoint_requests_true_end`、native physical thumb interior / held / end |
| source 破棄が古い pending / drag / measurement を残さない | `source_reset_drops_anchor_pending_intent_drag_and_measurements` |

新 owner の native 試行では thumb 終端の f64→Pixels 丸め差と、未計測 predecessor
を含む keyboard reveal が真に FAIL した。前者は公開 scrollbar と同じ Pixels の
終端を比較し、後者は対象より前の viewport 分を実測してから下端を合わせる。
`scrollbar_endpoint_uses_the_exposed_pixel_extent_not_f64_rounding` も修正前 FAIL を
記録した。推定値を実測と呼ぶ test shim や tolerance の緩和では解消しない。
空 source の End→append 契約も最初は pending End、次に存在しない reading key
の splice relocation で FAIL した。空 source には End intent も survivor anchor も
存在しないため、append 後は先頭から読み始める。
