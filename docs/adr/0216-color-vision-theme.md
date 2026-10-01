# ADR-0216: 色覚対応テーマ（青 / 橙）と、色差を数値で守るテスト

- Status: **Accepted**
- Date: 2026-10-01
- Related: [#354](https://github.com/TomiXRM/kagi/issues/354) slice 4、ADR-0036（theme registry）、ADR-0104（lane palette）

## 文脈

既定の Catppuccin Mocha は「追加 / 削除」「成功 / blocker」「ours / theirs」「diff 行の背景」を赤 / 緑で区別している。1 型・2 型色覚（protanopia / deuteranopia）ではこの軸がほぼ潰れる。実測（下表の計算方法と同じ）:

| Mocha の組 | 通常 | protan | deutan | tritan |
|---|---|---|---|---|
| `change_added` #a6e3a1 / `change_deleted` #f38ba8 | 58.6 | 29.0 | **13.1** | 51.5 |
| `diff_added_bg` #1c3a2a / `diff_removed_bg` #3a1c1c | 35.3 | **7.8** | **4.3** | 37.6 |

diff 行の背景は deutan で ΔE 4.3 — 見分けられない。

## 決定

1. **テーマを 1 本足す**: slug `color-vision`、表示名 EN「Color Vision (Blue/Orange)」/ JA「色覚対応（青 / 橙）」（`Theme::display_name()`、ブランド名の他テーマは従来どおり英語固定）。`crates/kagi-ui-core/src/theme_color_vision.rs` で Mocha を `..CATPPUCCIN_MOCHA` で継承し、意味の対になる token だけを Okabe–Ito パレットの青 / 橙に置き換える。

   | 役割 | token | 色 |
   |---|---|---|
   | 追加・成功・ours | `change_added` `color_success` `color_branch` `selection_tint` `term_green` | #56B4E9 |
   | 削除・blocker・theirs | `change_deleted` `color_blocker` `color_remote` `color_head` `term_red` | #E69F00 |
   | warning・modified（輝度で区別） | `color_warning` `change_modified` `term_yellow` | #F0E442 |
   | renamed・hunk 見出し | `change_renamed` `diff_hunk` | #CC79A7 |
   | blocker（無効時） | `color_blocker_muted` | #8A6420 |
   | diff 行背景 | `diff_added_bg` / `diff_removed_bg` | #123A52 / #4D3008 |
   | graph lane 8 色 | `lane_hsl` | #E69F00 #56B4E9 #F0E442 #CC79A7 #009E73 #D55E00 #3D8FD6 #B8B8B8 |

   設定は既存の `settings.json` の `"theme": "color-vision"`（`Settings::theme()` は slug 文字列で、theme の型付き列挙は `THEMES` registry そのもの）。Settings の theme 選択・command palette・View メニュー（`display_name`）に現れる。

2. **色差を数値で守る**: `crates/kagi-ui-core/src/color_vision.rs` に pure な CIEDE2000（Sharma 2005 の参照値 5 組で検証）と Machado, Oliveira & Fernandes (2009) の dichromat 行列（severity 1.0、linear sRGB）を置き、追加 / 削除の 4 組（badge・status・ours/theirs・diff 背景）について
   - 通常視 **ΔE ≥ 20**
   - protan / deutan / **tritan** の各シミュレーション後 **ΔE ≥ 15**

   を unit test と Tier A（`color_vision_theme`: アプリの `set_theme` 経路で切り替え、実際に active になった token で計算）で assert する。対照として Mocha の最悪 deutan ΔE が 15 未満であることも assert する（色を戻せば落ちることを確認済み）。

## 実測（Tier A の出力）

| 組 | 通常 | protan | deutan | tritan |
|---|---|---|---|---|
| change added / deleted（#56B4E9 / #E69F00） | 53.6 | 50.5 | 54.3 | 53.6 |
| success / blocker | 53.6 | 50.5 | 54.3 | 53.6 |
| ours / theirs | 53.6 | 50.5 | 54.3 | 53.6 |
| diff 背景（#123A52 / #4D3008） | 32.1 | 32.0 | 34.4 | 38.0 |

**tritanopia も閾値 15 を満たす**ので対象に含める（青 / 橙は 3 型でも輝度と彩度の差が残る）。

## 制約・既知の限界

- **warning と blocker**（#F0E442 / #E69F00）は deutan で ΔE 11.6、protan 15.2、tritan 18.8。warning は輝度（黄は最も明るい）と glyph（⚠ / ✗）で区別する設計で、この組は閾値 assert の対象外とした。
- 色覚シミュレーションは近似（Machado 2009 の 2 色覚モデル、異常 3 色覚の severity < 1 は扱わない）。実際の見え方の確認は当事者による。
- dark 版のみ。light テーマの派生は需要が出たら同じ test を再利用して足す。
- 構文ハイライト（`syntax`）は Mocha のまま（追加 / 削除の意味を持たないため）。

## 却下した案

- **既存全テーマに色覚モードを掛け算する**（token 変換を実行時に適用）: theme 数 × モードの組合せで見た目の確認が爆発し、各テーマの意図した配色を壊す。
- **単純 RGB 距離**: 暗い背景色同士で知覚差と大きくずれる（Mocha の diff 背景は RGB 距離では十分に見えるが deutan ΔE 4.3）。CIEDE2000 を使う。
