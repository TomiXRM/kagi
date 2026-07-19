# ADR-0122: コミットグラフを Stable レーン + 親ノード収束(Fork 型)にする

- Status: Accepted
- Date: 2026-07-17

## Context

ADR-0104 で `GraphLayoutMode`（`Stable` = gitk スタイル / `Compact` = Gitru 風
swimlane コンパクション）を導入した際、既定は `Stable`・設定キー
`graph_lane_compact` で切替、と決めた。しかしその後のスプリントで
`build_commit_rows`（`src/ui/commit_list.rs`）が `Compact` を**ハードコード**し、
`graph_lane_compact` はアバターノード・レーン帯などの**見た目**専用フラグに転用
されていた（コード上のコメント・doc は「layout を切り替える」と書かれたまま乖離）。

`Compact` 常用で 2 つの見た目問題が顕在化した（issue 報告: 「新しいブランチが
生えた時にグニって途中から曲がる」）。

1. **重複レーン**: `layout_compact` には Stable の step-4 例外（first parent が
   既に他レーンで待たれていたら、そのレーンへ合流してレーンを閉じる）が移植
   されておらず、**同じコミットを待つレーンが 2 本**並走できた。結果、マージ線と
   ブランチ幹線が最後まで並走し、対象コミットの行で初めて横曲がりで収束する
   （長い線が「途中でグニッ」と曲がって見える）。
2. **shift による横揺れ**: コンパクションは左のレーンが閉じるたびに生存レーンを
   横シフトさせるため、分岐点と無関係な行でも線が曲がる。Fork / GitKraken の
   「各ブランチが自分の列を保ち、分岐・合流点でだけ曲がる」整列とは原理的に
   両立しない。

さらに Stable 切替後のフィードバックで第 3 の問題が判明した: gitk 由来の
**step-4 first-parent 例外**(first parent が既に他レーンで待たれていたら、
**子コミットの行で** `OutOfNode` を出して既存レーンへ合流しレーンを閉じる)は、
レーン距離が離れると子の行に長い水平ジョグを作り、「線が空中で他の線に合流する」
配線盤のような見た目になる。期待される見た目(Fork)は逆で、**分岐は親(fork)
コミットのノードから扇状に広がる** — 各線は自分の列を親の行まで直進し、親ノードに
吸い込まれる形で曲がる。

## Decision

1. **出荷レイアウトを `GraphLayoutMode::Stable` にする。**
   `build_commit_rows`（`src/ui/commit_list.rs`）と branch solo の再レイアウト
   （`src/ui/graph_solo.rs`）を `Stable` に切替。Stable はブランチが生きている間
   列を保持し、解放列は**新規レーンのみ**が再利用する（既存レーンはシフトしない）
   ので、長いブランチ線は直線のまま、曲がるのは分岐・合流点だけになる。
1b. **Stable から step-4 first-parent 例外を撤去し、親ノード収束（Fork 型）にする。**
   first parent が既に他レーンで待たれていても、ノード自身のレーンをそのまま
   first parent へ向けて継続する（重複 target を許す）。複数の線が同じコミットに
   到達したら**親コミットの行**で `IntoNode` として収束する — 分岐が親ノードから
   扇状に広がり、子の行での空中ジョグは発生しない。収束行のノード位置は
   **merge-born レーン優先**: マージの第 2 親以降が「そのコミットのために」開いた
   レーン（= マージされたブランチ自身の線。first-parent 継続で retarget されたら
   フラグは消える）があればそれが勝ち、マージ線がブランチ先端を垂直に貫く。
   無ければ最左の待機レーン（幹が直進を保つ）。`Lane` に `merge_born: bool` を追加。
2. **`graph_lane_compact` は swimlane 見た目フラグとして正式化。**
   （アバターノード・レーン帯・レーンパッド。）レイアウトモードはユーザー設定から
   切り替え不可となり、`Compact` は API（`layout_with`）専用に残る。乖離していた
   doc/コメント（`theme.rs` / `settings.rs` / `settings_view.rs`）を実態に合わせて修正。
3. **`layout_compact` には first-parent join を実装。**
   `parents[0]` を既に待っている生存レーンがある場合、重複レーンを開かず
   その行で `OutOfNode`（既存レーン色）を出して自レーンを閉じる。Compact では
   「open レーンの target は一意 ＝ どのコミットにも入ってくるレーンは高々 1 本」
   が成り立ち、パックされたレイアウトを細く保つ（収束方式は幅が伸びるため
   Compact の目的と両立しない）。`Compact` は現状 UI から到達不能だが、
   ADR-0104 の「後から外せる構造」を保ったまま健全化しておく（将来削除する場合は
   ADR-0104 記載どおり `Compact` 分岐＋テスト削除で済む）。

## Rationale

- 報告された「途中でグニッ」は (1) の重複レーン収束（ノードが最左レーンに置かれ
  長い線の側が曲がる）が主因、(2) の shift も同種の視覚ノイズ。Stable は両方を
  構造的に持たない（Pass は常に垂直、列は不動）。
- 収束方式 + merge-born 優先により、「長い線は直線・曲がりは必ずノードに接する」
  が成り立つ: 曲がるのは (a) マージエッジ（マージノードの行）、(b) fork への
  合流（親ノードの行）だけ。子の行で線が空中合流する step-4 例外の見た目問題を
  解消し、目標の見た目（Fork 風の階段整列・親から扇状に開く分岐）に一致する。
- 幅の犠牲は小さい: Stable も解放列を新規レーンで左詰め再利用するため、幅は
  「同時に生きているレーン数の最大」程度に収まる。
- 見た目フラグ（アバター等）とレイアウトを分離したことで、swimlane 見た目 ON の
  ままレーン整列が得られる（従来はフラグが層をまたいで混線していた）。

## Consequences / Risks

- グラフの見た目が変わる（列は増えうるが線は直線化）。GUI 目視は人間の確認待ち。
- 収束方式では fork の子ブランチ線が親の行まで開いたままになるため、step-4 例外
  ありの旧 Stable よりレーン数が増えうる（Fork 型整列の本質的トレードオフ）。
- 収束行では `IntoNode` が複数（from の異なる）並ぶ。ADR-0003 の行内完結エッジ
  モデルの範囲内で、描画側は従来から対応済み。
- `Compact` は到達不能コードになるが、`layout_with` の公開 API・テストは維持。
- `layout_compact` の join により、既存の Compact 形状（重複レーン前提の描画）とは
  非互換。ドメインの不変条件チェッカー（relaxed）はそのまま通る。

## Verification

- Stable 収束: `test_first_parent_converges_at_parent_row`（共有親への収束が
  親の行で起きる）、`test_branch_and_merge`（feature 線が fork ノードに
  `IntoNode 1→0` で入る）、統合 `test_stable_stacked_branches_staircase`
  （階段形状 + マージ線がブランチ先端を垂直に貫き、チェーンが先端の行で合流）。
- Compact join: `test_compact_first_parent_joins_existing_lane`（IntoNode ≤ 1・
  マージ線の垂直到達・join エッジ色）、`test_compact_tip_joins_existing_lane`。
- `cargo test --workspace` 全緑 / `cargo fmt --check` / `cargo clippy`（自 diff に
  新規警告なし）。
