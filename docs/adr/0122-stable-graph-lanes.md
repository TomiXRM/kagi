# ADR-0122: コミットグラフのレーンレイアウトを Stable に戻し、Compact に first-parent join を移植する

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

## Decision

1. **出荷レイアウトを `GraphLayoutMode::Stable` にする。**
   `build_commit_rows`（`src/ui/commit_list.rs`）と branch solo の再レイアウト
   （`src/ui/graph_solo.rs`）を `Stable` に切替。Stable はブランチが生きている間
   列を保持し、解放列は**新規レーンのみ**が再利用する（既存レーンはシフトしない）
   ので、長いブランチ線は直線のまま、曲がるのは分岐・合流点だけになる。
2. **`graph_lane_compact` は swimlane 見た目フラグとして正式化。**
   （アバターノード・レーン帯・レーンパッド。）レイアウトモードはユーザー設定から
   切り替え不可となり、`Compact` は API（`layout_with`）専用に残る。乖離していた
   doc/コメント（`theme.rs` / `settings.rs` / `settings_view.rs`）を実態に合わせて修正。
3. **`layout_compact` に first-parent join（step-4 例外）を移植。**
   `parents[0]` を既に待っている生存レーンがある場合、重複レーンを開かず
   その行で `OutOfNode`（既存レーン色）を出して自レーンを閉じる。これで両モード
   とも「open レーンの target は一意 ＝ どのコミットにも入ってくるレーンは高々
   1 本」という不変条件が成り立つ。`Compact` は現状 UI から到達不能だが、
   ADR-0104 の「後から外せる構造」を保ったまま健全化しておく（将来削除する場合は
   ADR-0104 記載どおり `Compact` 分岐＋テスト削除で済む）。

## Rationale

- 報告された「途中でグニッ」は (1) の重複レーン収束が主因、(2) の shift も同種の
  視覚ノイズ。Stable は両方を構造的に持たない（Pass は常に垂直、join は step-4
  例外で子ブランチ側の行に置かれる）ので、目標の見た目（Fork 風の階段整列）に
  一致する。
- 幅の犠牲は小さい: Stable も解放列を新規レーンで左詰め再利用するため、幅は
  「同時に生きているレーン数の最大」程度に収まる。
- 見た目フラグ（アバター等）とレイアウトを分離したことで、swimlane 見た目 ON の
  ままレーン整列が得られる（従来はフラグが層をまたいで混線していた）。

## Consequences / Risks

- グラフの見た目が変わる（列は増えうるが線は直線化）。GUI 目視は人間の確認待ち。
- `Compact` は到達不能コードになるが、`layout_with` の公開 API・テストは維持。
- `layout_compact` の join により、既存の Compact 形状（重複レーン前提の描画）とは
  非互換。ドメインの不変条件チェッカー（relaxed）はそのまま通る。

## Verification

- 新規ドメインテスト: `test_compact_first_parent_joins_existing_lane`（重複レーン
  再現トポロジで IntoNode ≤ 1・マージ線の垂直到達・join エッジ色）、
  `test_compact_tip_joins_existing_lane`（新規 tip の合流）。
- 新規統合テスト: `test_stable_stacked_branches_staircase`（積み重ねブランチで
  各セグメントが自分の列を保つ「階段」形状と IntoNode ≤ 1 を固定）。
- `cargo test --workspace` 全緑 / `cargo fmt --check` / `cargo clippy`（自 diff に
  新規警告なし）。
