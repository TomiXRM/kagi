# ADR-0006: UI コンポーネントは gpui-component を段階導入する(Zed 本体の流用は不可)

- Status: Accepted
- Date: 2026-06-12
- 発端: ユーザー提案「Zed のファイルビューワー / コミットペインの実装を使えないか」

## Context(調査結果 2026-06-12)

| 選択肢 | crates.io | gpui 0.2.2 互換 | ライセンス | 判定 |
|--------|-----------|----------------|-----------|------|
| Zed の project_panel / git_ui | ✗(publish=false) | ✗(git main の分割版 gpui。型が別世界。依存 ~24〜33 内部 crate ≒ Zed 全体) | **GPL-3.0-or-later** | 直接依存不可。**コード転写も不可(GPL 汚染)**。設計パターンの参考のみ |
| Zed 公式の ui コンポーネント crate | 未公開 | — | GPL | 存在しない |
| **gpui-component 0.5.1**(longbridge) | ✓(2026-02) | **✓(`gpui ^0.2.2` 依存 — うちと完全一致)** | **Apache-2.0** | **直接依存可能** |
| adabraka-ui | ✓ | ✗(独自 fork の gpui に依存) | MIT | 不可 |

gpui-component が提供するもの(関連分): **Tree**(階層表示)/ **Dock・Resizable**(リサイズ可能ペイン)/
**IME 対応 Input**(macOS NSTextInputClient 経由 — **日本語 commit message が打てる**)/ Scrollbar / 仮想化 Table。

注意: gpui-component の**公式サイトのドキュメントは git main(Zed 版 gpui)準拠**。
うちの組(crates.io gpui 0.2.2 + gpui-component 0.5.1)では **docs.rs/gpui-component/0.5.1 と
0.5.1 タグの examples を一次資料**とする(サイトを見ると API がズレる)。

## Decision

1. `gpui-component = "0.5.1"` を依存に追加し、**段階導入**する:
   - **第1弾(T025)**: commit message 入力に gpui-component の Input を使う(IME 対応が即座に効く)
   - 以降、効果が明確な箇所から置き換え(Scrollbar、Tree の折りたたみ、必要なら Dock)
2. 既存の自前実装(uniform_list ベースのリスト/ツリー、graph canvas、T023 のディバイダ)は**動いている限り維持**。
   全面置き換えの big-bang はしない
3. Zed 本体のコードは「読んで設計を学ぶ」用途に限定(GPL のためコピー禁止を subagent 指示にも明記する)

## Rationale

- 自前実装の弱点(IME 非対応 input、スクロールバー非表示、ツリー折りたたみ)がライブラリで一気に解消し、
  ライセンス(Apache-2.0)も gpui 本体と揃う
- バージョンが `gpui ^0.2.2` 固定なので、うちの「crates.io 0.2.2 に pin」戦略(ADR-0001)とも整合
- 一方で 60+ コンポーネントの大型依存なので、置き換えは価値の出る箇所だけに絞る

## Consequences / Risks

- ビルド時間と依存サイズが増える(初回 +数分見込み)
- gpui-component の theme 系と自前の Catppuccin 配色の整合を取る必要(Input 等の見た目調整)
- gpui-component が将来 git main 版 gpui に全面移行した場合、0.5.x 系に留まる判断が必要になりうる
  (その時は gpui 本体のバージョン戦略ごと再検討 = ADR 改訂)
