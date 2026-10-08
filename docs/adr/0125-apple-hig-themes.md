# ADR-0125: Apple Light / Apple Dark themes (HIG system colors)

- Status: Accepted
- Date: 2026-07-19

## Context

kagi のテーマは W9-THEME(ADR-0036)の `Theme` トークン機構に載っている。
ユーザー要望: Apple の Human Interface Guidelines のシステムカラー
(https://developer.apple.com/design/human-interface-guidelines/color)から
Apple Light / Apple Dark の 2 テーマを追加したい — ボタン・文字色から
スイムレーンのパレットまで HIG 由来で。

HIG のシステムカラーは 2025-06-09(Liquid Glass 対応)で**値が更新**されて
いる。ページは SPA のため、docs データ JSON
(`/tutorials/data/design/human-interface-guidelines/color.json`)の
スウォッチ画像 alt テキスト(`R-255,G-56,B-60` 形式)から 12 色 ×
{Default, Increased contrast} × {light, dark} と `systemGray`..`Gray6` を
抽出した(2026-07-19 取得。例: red light は旧 `#FF3B30` → 現 `#FF383C`)。

## Decision

1. `crates/kagi-ui-core/src/theme_apple.rs`(sibling、LOC ratchet 配慮)に
   `APPLE_LIGHT` / `APPLE_DARK` を定義し `THEMES` に登録
   (slug: `apple-light` / `apple-dark`)。
2. **色の使い分けポリシー**:
   - Dark テーマは *Default (dark)* variant をそのまま使う(暗背景向けに
     調律済み)。
   - Light テーマは、細線・テキストとして読まれるもの(ステータス色・
     change バッジ・**スイムレーン 8 色**)に *Increased contrast (light)*
     variant を使う(default の yellow `#FFCC00` は白地の 2px レーンとして
     判読不能)。塗りチップ(ref バッジ)は鮮やかな *Default (light)*。
   - HIG の色意味論を保存: blue = アクセント/リンク、green = 成功、
     red = 破壊的、orange = 警告。
   - 背景/テキストは `systemBackground` + `systemGray` ランプ +
     label/secondaryLabel の実効(α合成済み)値。
3. スイムレーンの並びは既存パレットと同じ隣接最大差の順
   (pink→green→blue→orange→teal→purple→yellow→cyan、ADR-0104)。
4. ターミナル 16 色は normal = 一方の variant、bright = もう一方で構成
   (light: contrast/default、dark: default/contrast)。

## Consequences

- テーマ数 11 → 13。既定(index 0 = Catppuccin Mocha)は不変。設定・メニューは
  slug ベースなので挿入位置の影響なし。
- HIG の値が再改訂された場合は `theme_apple.rs` の定数を更新する
  (取得手順はこの ADR の Context に記載)。

## Amendment (2026-10-08): neutral macOS chrome and readable labels

Issues #1065 / #1066: Apple Light の灰色に色味があり、sidebar の見出し・更新時刻や
フォームの label が薄い。当初の公式 illustration と RGB 値だけの評価では、
実 Finder に対して sidebar を暗くしすぎた。ユーザーの再指摘を受け、同じ Mac で
空の検証フォルダを開いた Finder の実ウィンドウを撮影して比較した
([PR #1085](https://github.com/TomiXRM/kagi/pull/1085))。両方とも非 key window、
Light 表示、透明度低減・コントラスト増加は無効。ユーザーの foreground と
既存ウィンドウは変更していない。Finder の操作モデルや material の再実装ではない。

Apple Light の背景は iOS の `systemGray` 表の転記ではなく中立色にする:
sidebar / panel `#f7f7f7`、hover / divider surface `#e5e5e5`、
alternate row `#f5f5f5`、content `#ffffff`。更新先は現在の
`crates/kagi-ui-core/src/theme_apple_light.rs` の既存 token のみ。

撮影画像の Display P3 profile を native decoder で sRGB に変換した空白点では、
Finder の sidebar / toolbar は `(249,249,249)`、content は `(255,255,255)`。
旧案の Kagi sidebar は `(241,241,241)` で明度が低かった。更新後は sidebar /
toolbar が `(249,249,249)`、content が `(255,255,255)` となることを実画像で確認した。
source token と撮影 pixel の値は区別する。これは現在の非 active 状態の比較であり、
別 wallpaper / display / OS material 状態でも一致すると主張しない。

読む必要がある補助文字は `text_muted = #666666`、
`text_sub = text_label = #606060` とする。palette 値の sRGB relative
luminance から計算した contrast は、最も暗い対象 surface `#e5e5e5` でも
それぞれ 4.558:1 / 4.992:1、sidebar では 5.360:1 / 5.870:1。
disabled の opacity と装飾には通常文字と同じ要件を適用しない。

Apple Dark、ほかの built-in preset、Git の状態色・ref chip・lane palette、
font、geometry と操作契約は変更しない。custom theme の明示値は保持され、
`extends: "apple-light"` で省略した token だけは更新後の値を継承する。
これは静的 preset の調整であり、OS の dynamic semantic color や vibrancy を
導入したとは扱わない。

色の階層は実 Finder と比較して改善したが、Kagi の太い pane divider・大文字の
group heading・inactive window の文字/操作の減衰・Inter と system font の差は
残る。今回の palette 修正を「Finder と同じ native control/material」と評価しない。
font の別監査は #1068。custom override の検証で使った緑色 sidebar は preset と
混同させないよう、実機検証終了時に通常 Apple Light に戻す。

