# ADR-0126: Translucent ref badges on light themes + Apple Dark yellow accent

- Status: Accepted
- Date: 2026-07-19
- Amends: ADR-0125(Apple themes)、badge_style の GitKraken 方式(ADR-0036 系)

## Context

ユーザー判断(2026-07-19):

1. ライトテーマの ref バッジ(ブランチラベル)が不透明ベタ塗りで
   ハイコントラストになっており、ダークテーマの tint チップ(Liquid Glass 的な
   ニュアンス)と揃っていない。GitKraken のライトテーマは「半透明 tint +
   黒文字」— これに合わせたい(Apple Light に限らず全ライトテーマ)。
2. Apple のダークモード系アプリ(カメラ、メモ)は systemYellow をアクセントに
   使う。Apple Dark テーマのアクセントも黄色にしたい。ライトは青のまま。

## Decision

1. **`badge_style` のライト分岐を tint 化** — fill = ref 色 20%(0x33)、
   border = 40%(0x66)はダークと同一の文法、文字色のみ
   `text_main`(ライトでは略黒)。全テーマ・全バッジ(ref チップ / WIP /
   stash / worktree レーンチップ)が単一の seam で切り替わる。
2. **Apple Dark のアクセント = systemYellow `#FFD600`** — kagi では
   `color_branch` が UI アクセント(primary ボタン・アクティブタブ・リンク・
   トースト)を兼務しているため、`color_branch` を黄色にする。付随して:
   - `color_warning` を orange `#FF9230` へ(HIG の警告意味論。黄色は
     アクセントに譲る)
   - `selected` を systemGray4 dark `#3A3A3C` へ(メモ.app 風の無彩色選択。
     旧い青 tint は黄色アクセントと衝突)
   - `term_cursor` を黄色へ
   - branch ref チップも黄色 tint になる(メモ.app の黄色チップ/リンクと
     同じ雰囲気 — 意図どおり)
3. 将来 `color_branch` からの UI アクセント分離(`ui_accent` トークン新設、
   44 call site の再分類)は本 ADR の非目標(必要になったら別 ADR)。

## Consequences

- 全ライトテーマのバッジが淡い tint + 黒文字になり、ダークと視覚文法が揃う。
- Apple Dark は黄色アクセント(ブランチチップ・リンク・アクティブタブ・
  primary ボタン)+ 無彩色選択となり、Apple ダークアプリの雰囲気に寄る。
- Apple Dark の warning と tag はどちらも orange になるが、表示文脈
  (テキスト vs チップ)が重ならないため許容。
