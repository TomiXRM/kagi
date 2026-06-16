# ADR-0088: コミットグラフに stash を描画(base commit への枝線)

- Status: Accepted(2026-06-16、ユーザー依頼「どこから stash が生えたのか追跡したい。graph に stash を描画。icon は stash、行は黄色。WIP の下に配置し graph line をそこまで伸ばす」「base commit へ枝線を引く(フル)」)
- Date: 2026-06-16
- Builds on: ADR-0087(stash 操作)、グラフレイアウト(`kagi_domain::graph::layout` / `GraphEdge`)

## Context

stash は「どのコミットから生えたか(=作成時の HEAD)」が分かると追跡しやすい。これを
コミットグラフ上で表現したい。難点は:

- グラフレイアウト `layout()` は `snap.commits`(refs から到達可能なコミット)だけを
  対象にする。stash commit はそこに含まれない。
- 行 index ↔ `snap.commits` index の 1:1 対応に多数の箇所が依存している
  (selection / details / compare / context menu / oplog)。stash を通常コミットとして
  リストに混ぜると、この結合を全面的に触る必要があり高リスク。

## Decision

**メインのグラフレイアウトは一切変更しない**。その上に stash 用の「追加レーン」を重ねる:

1. **データ**: `Stash` に `base: Option<CommitId>`(stash commit の第1親 = 生成元)を追加。
   snapshot の `collect_stashes` で `find_commit(oid).parent_id(0)` から解決する。
2. **レーン/エッジ注入**(`commit_list::build_commit_rows_with_stashes`):
   - まず従来どおり `build_commit_rows`(mainline は不変)。
   - 各 stash に mainline の右側の**専用レーン** `L+i` を割り当てる。
   - base commit が読み込み済みウィンドウ内にあれば、先頭から base 行の手前まで各コミット行に
     `Pass` エッジ、base 行に `IntoNode` エッジを **注入**する。これで stash ノード(上の
     固定行)から base commit まで枝線が伸びる。
   - base が範囲外なら枝線なし(ノードのみ)。
   - `(commit_rows, stash_rows, stash_lanes)` を返す。行 index 結合は **不変**
     (stash 行は別 Vec で、仮想リストには入れない)。
3. **描画**:
   - stash 行は **WIP 行の直下**に固定ブロックとして描画(`render_stash_graph_rows`)。
     列構成はコミット行と同じ(badge | graph | message)。
   - badge 列に **inbox アイコン + "stash" チップ**(黄 = `color_warning`)、message も黄。
   - graph 列は既存の `graph_canvas` を再利用(stash ノード + 下向き `OutOfNode`)。
   - `graph_canvas` に `stash_lanes` を渡し、その**レーンのノード/エッジを黄色**で塗る。
   - 左クリック = Pop、右クリック = stash メニュー(ADR-0087 と同じ)。

## Consequences

- 「stash がどこから生えたか」がグラフ上で一目で分かる。base が HEAD 以外の古いコミットでも
  正確に枝線が伸びる(フル要望を満たす)。レイアウトは mainline 非破壊、行 index 結合も不変で
  低リスク。
- stash 1件につき 1 レーン増えるためグラフ幅がやや広がる(数件想定なので許容)。
- stash 行は仮想化リストの外(固定)。base が下方にスクロールしても、コミット行側に注入した
  `Pass`/`IntoNode` エッジがスクロールに追従するので枝線は base に繋がり続ける。
- `Stash.base` 追加に伴い `collect_stashes` を2パス化(foreach で oid 収集 → parent 解決)。
- 関連: stash 操作は [[0087-stash-sidebar-actions-and-drop]]。
