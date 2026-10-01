# ADR-0217: Graph sidebar の 6 ペインと保存する高さ比

- Status: Accepted
- Date: 2026-10-01
- Related: #864、ADR-0014（Repository Navigator）、ADR-0197（window-global sidebar state）、ADR-0199（sidebar gesture）

## 背景

Graph の Repository Navigator は PR / local branch / remote branch / worktree / tag / stash を 1 本の仮想 list に平坦化していた。多数の local branch があると後続の section を見るために list 全体をスクロールする必要があり、それぞれの一覧を独立した高さにできなかった。

## 決定

1. **6 枠を固定**: Graph の sidebar に PR → local branch → remote branch → worktree → tag → stash の順で、見出しを固定した 6 つの Tree を表示する。PR が 0 件でも見出しと `(0)` を表示する。filter と Merged branches は上、worktree inspection は下のまま。各 Tree の葉だけ独立した `UniformListScrollHandle` で仮想スクロールする。既存のクリック / context menu / drag & drop / group collapse と accessibility の階層・兄弟位置は、共通の行 renderer を使い続ける。
2. **行の単一キャッシュ**: `render` は従来の fingerprint で `sidebar.rows` を構築し、その再構築時だけ 6 個の連続 range を求める。スクロールや divider drag は行を clone / group 化し直さない。pane geometry は paint 済みの bounds を持ち、root の `DividerDrag::SidebarPane(index)` が cursor の window 座標と zoom から、両側で最も近い開いた 2 枠の重みだけを変える。閉じた枠の高さは見出し分で固定し、開くと以前の重みに戻る。狭い viewport に見出し 6 個が収まらない場合は外側の stack を縦スクロールし、下の pane / bottom panel の上に描画しない。
3. **状態の所有者**: 既存の `SidebarState::collapsed: HashSet<&'static str>` だけが runtime の開閉状態。保存形式の mask はこの集合から毎回導出し、起動時の `SidebarState::new()` で一度だけ復元する。幅・filter・開閉・重み・スクロールは tab 切替でコピーしない window-global presentation state（ADR-0197）。Git repository に書き込まない。
4. **設定は 1 キーの文字列**: `settings.json` の `"sidebar_panes": "v1:1200,2600,2200,1800,1100,1100:0"` は 6 個の正の `u16` 重みと下位 6 bit の開閉 mask。`Settings::sidebar_pane_layout()` は欠落・壊れた版・件数違い・0・範囲外を既定値として読むだけで、起動・再描画で壊れた raw を上書きしない。ユーザーが divider を動かすか header を開閉したときだけ、**現在有効な重みと新しい mask の完全な v1 値**を `settings::write_setting` に渡す。`settings::store` が既存の原子的保存と drag の連続書き込み集約を担当する。別の設定 key は増やさない。

## 検証

- pure test: PR 0 件でも最初の pane の見出しが存在し、section と連続 range の順序が一致する。設定の v1 往復と不正値・0・件数違いの既定値フォールバック。
- native Tier A `sidebar_tree_roles` / `sidebar_panes`: 6 つの名前付き Tree、実マウスの separator drag、独立スクロール、開閉と重みの復元、不正値の起動時不変と header 操作後の v1 書き込み、900px と 600px / 125% の配置・スクロール、再 mount 後の状態復元。
- #911 review: divider の重みは header を含む pane 全体の実測高と同じ座標系で計算する。PR / LOCAL の高さが異なる状態で初回の小さな実マウスドラッグ後、separator の中心が pointer と一致することを Tier A で検証する。header 分を差し引く旧計算では 12px ずれることを変異で確認した。

## 却下した案

- pane ごとに branch / tag 等を再分類・再 clone する: divider drag と再描画が O(全 ref) になる。
- 開閉用の第 2 key / 独立した bool 配列: `SidebarState::collapsed` と保存値が乖離する。
- viewport が低いとき pane を自動で閉じる: 明示的なユーザーの開閉状態を変更してしまう。
