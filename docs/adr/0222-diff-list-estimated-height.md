# ADR-0222: Diff list の未計測行に推定高さと末尾 intent を使う（#1122）

- Status: **Accepted**
- Date: 2026-10-10
- Related: [ADR-0209](0209-pr-review-threads-on-diff.md)、#1122

## 文脈

可変高の diff list は全行を描画せずに必要な行だけを計測する。upstream GPUI の `list` は未計測行を高さ 0 として集計し、estimated-height API を持たないため、大きな diff の scrollbar が計測済み prefix だけを表し、大きなホイール入力でも実際の末尾に届かなかった。行の source identity を未変更 reload で交換すると、読んでいた位置も reset される。

## 決定

1. 公開 fork [TomiXRM/zed](https://github.com/TomiXRM/zed/commit/caf5007618c309a8ea66002d44a9c9ab629a2d35) の `caf5007618c309a8ea66002d44a9c9ab629a2d35`（branch `fix/kagi-list-estimated-height`、base `90b3aa0b3bd3b453775b11a386907c7ac9acd997`）を pin する。upstream PR 待ちの Kagi list fix であり、同じ Zed source の sibling crates もこの rev に統一し、一時的な absolute path patch を残さない。
2. `crates/kagi-ui-core/src/diff_list.rs` の `DiffListLayout::sync` から diff list だけが未計測行の推定高さと end intent に opt-in する。source / zoom / gutter の変更だけが reset を要求し、remeasure は位置を保つ。全行 eager render や第二の scroll model は作らない。
3. thread の展開・折りたたみは measured nodes を splice する。PR の同じ選択の reload と Editor の未変更 reload は adopt / install により source identity を保持する。幅による reflow は行内の fractional anchor を保持する。

## 帰結

- scrollbar range は未計測部分を含む推定 extent を反映し、実測に伴って更新される。
- 大きな末尾方向のホイール入力 1 回で真の最終行に到達する。最終行が viewport より高くても、その下端を viewport 下端に揃える。end intent はこの操作の解決であり、以後の変更に無条件で追従する設定ではない。
- thread 開閉、同じファイルの再クリック、未変更 Editor reload、幅変更で読んでいた位置を不必要に捨てない。別 source / zoom / gutter の変更とは区別する。

## 証拠と限界

本変更の受入記録は GPUI unit **30/30**、native の次の **7 oracles PASS**：
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
- oversized first content row は native item 0 / offset 0 に戻り、hunk header 上端と viewport 上端、first row 上端と header 下端の差が各 ≤ 1px、first row 下端が viewport 下端より下。収まる行は全体包含。
- thread card が `viewport.height − badge.height` より高ければ badge は全体包含、card 上端は badge 下端 − 1px 以降、body 上端は viewport 内、body は card 内。収まる card と badge は両方全体包含。
- collapse は item count を 1 減らし、item 9 を code row に戻す（row 8 の高さとの差 ≤ 1px）。`pre.extent − collapsed.extent` は正で、少なくとも `expansion.height − viewport.height`。新しく露出する行の実測を許す。

これは状態と実 layout geometry の証拠であり、FPS / CPU の改善は主張しない。
Tier B の実アプリ比較確認は pending。
