# ADR-0217: Graph sidebar の 5 ペインと保存する高さ比

- Status: Accepted
- Date: 2026-10-01
- Related: #864、ADR-0014（Repository Navigator）、ADR-0197（window-global sidebar state）、ADR-0199（sidebar gesture）

## 背景

Graph の Repository Navigator は local branch / remote branch / worktree / tag / stash を 1 本の仮想 list に平坦化していた。多数の local branch があると後続の section を見るために list 全体をスクロールする必要があり、それぞれの一覧を独立した高さにできなかった。PR は上部の PRs タブで扱い、Graph には重複させない。

## 決定

1. **5 枠を固定**: Graph の sidebar に local branch → remote branch → worktree → tag → stash の順で、見出しを固定した 5 つの Tree を表示する。PR は上部の PRs タブだけに置く。filter と Merged branches は上に置く。worktree inspection は行にホバーした時だけカードに表示し、下部に常設ペインを置かない。各 Tree の葉だけ独立した `UniformListScrollHandle` で仮想スクロールする。既存のクリック / context menu / drag & drop / group collapse と accessibility の階層・兄弟位置は、共通の行 renderer を使い続ける。
2. **行の単一キャッシュと密度**: `render` は従来の fingerprint で `sidebar.rows` を構築し、その再構築時だけ 5 個の連続 range を求める。スクロールや divider drag は行を clone / group 化し直さない。section header と通常の leaf は 20px（section の文字は xs）、group の見出し内容は 18px、worktree leaf はその pane 専用の 24px。通常の leaf は xs、相対時刻などの補助文字は zoom に連動する 11px 程度。`uniform_list` は各 pane 内で同じ行高を要求するため、group は 20px の slot 内に収め、worktree leaf のみ別 pane の 24px slot にする。pane geometry は paint 済みの bounds を持ち、root の `DividerDrag::SidebarPane(index)` が cursor の window 座標と zoom から、両側で最も近い開いた 2 枠の重みだけを変える。閉じた枠の高さは見出し分で固定し、開くと以前の重みに戻る。狭い viewport に見出し 5 個が収まらない場合は外側の stack を縦スクロールする。
3. **状態の所有者**: 既存の `SidebarState::collapsed: HashSet<&'static str>` だけが runtime の開閉状態。保存形式の mask はこの集合から毎回導出し、起動時の `SidebarState::new()` で一度だけ復元する。幅・filter・開閉・重み・スクロールは tab 切替でコピーしない window-global presentation state（ADR-0197）。Git repository に書き込まない。
4. **設定は 1 キーの文字列**: `settings.json` の `"sidebar_panes": "v1:3500,2500,2000,1000,1000:0"` は local 35% / remote 25% / worktree 20% / tag 10% / stash 10% の 5 個の正の `u16` 重みと下位 5 bit の開閉 mask。未リリースの旧 6 重み値は件数違いとして既定値にフォールバックし、raw は上書きしない。`Settings::sidebar_pane_layout()` は欠落・壊れた版・件数違い・0・範囲外を既定値として読むだけで、起動・再描画で壊れた raw を上書きしない。ユーザーが divider を動かすか header を開閉したときだけ、**現在有効な重みと新しい mask の完全な v1 値**を `settings::write_setting` に渡す。`settings::store` が既存の原子的保存と drag の連続書き込み集約を担当する。別の設定 key は増やさない。

## 検証

- pure test: 最初の pane が LOCAL で section と連続 range の順序が一致する。設定の 5 重み v1 往復と旧 6 重み・不正値・0・件数違いの既定値フォールバック。
- native Tier A `sidebar_tree_roles` / `sidebar_panes` / `worktree_inspection`: 5 つの名前付き Tree、Graph に PR pane がないこと、実マウスの separator drag、独立スクロール、開閉と重みの復元、不正値の起動時不変と header 操作後の v1 書き込み、900px と 600px / 125% の配置・スクロール、再 mount 後の状態復元。
- #911 review: divider の重みは header を含む pane 全体の実測高と同じ座標系で計算する。高さの異なる LOCAL / REMOTE の境界を初回の小さな実マウスドラッグ後、separator の中心が pointer と一致することを Tier A で検証する。旧 6 ペイン時に header 分を差し引く計算では PR / LOCAL が 12px ずれることを変異で確認した。

## 却下した案

- pane ごとに branch / tag 等を再分類・再 clone する: divider drag と再描画が O(全 ref) になる。
- 開閉用の第 2 key / 独立した bool 配列: `SidebarState::collapsed` と保存値が乖離する。
- viewport が低いとき pane を自動で閉じる: 明示的なユーザーの開閉状態を変更してしまう。
