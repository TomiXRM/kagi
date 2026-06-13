# 02 — Commit Graph / Virtualized List / Refs / Selection (re-architecture research)

- Status: Research (research sub-agent #2 / PM-led v1.0 re-architecture)
- Date: 2026-06-14
- Scope: commit graph layout, virtualized list for 10k+ rows, branch/ref badge display, selection state ownership.
- 一次資料(直読): `src/graph/mod.rs`, `src/ui/graph_view.rs`, `src/ui/commit_list.rs`, `src/ui/mod.rs`(該当フィールド/メソッド), `src/ui/theme.rs`(`lane_color`), `tests/graph_layout_test.rs`。
- 関連 ADR: 0003(graph layout)/ 0010(ahead-behind)/ 0006・0034(gpui-component)/ 0036(color themes)/ 0037(avatars)。
- 既存 research: `docs/research/gpui-component-audit.md` §2.9–§2.10, `docs/research/zed-gpui-reuse-research.md`。
- TARGET LAYERING: domain(pure) → git-backend → app(AppState/selection) → ui(view-models + views)。
  Invariant: **UI は git2 を直接呼ばない。graph layout は pure-domain で unit-test 済み。**

---

## 1. Kagi 現状

### 1.1 既に綺麗な部分 — graph layout は既に pure-domain

`src/graph/mod.rs` は **既に目標層構造に合致している**:

- 依存は `crate::git::{Commit, CommitId}` のみ。**gpui / git2 への依存ゼロ**(ファイル冒頭コメントで明記)。
- 公開 API は `pub fn layout(commits: &[Commit]) -> GraphLayout` の 1 本。入力は topo order(子が親より先)。
- 出力型 `GraphLayout { rows: Vec<GraphRow>, lane_count }`、`GraphRow { commit, lane, edges }`、
  `GraphEdge { from_lane, to_lane, kind }`、`EdgeKind { Pass, IntoNode, OutOfNode }`。
- アルゴリズムは gitk 系の **行単位 active-lane 割り当て**(ADR-0003 方式1)。状態は `active: Vec<Option<CommitId>>`
  だけ。5 ステップ(waiting lane 探索 → node lane 割当 → top-half edge → parent 配置 → lane_count 追跡)。
- **edge は 1 行内で完結**(`from_lane` = 行上端、`to_lane` = 行下端)。これが仮想化と極めて相性が良い:
  描画層は 1 行を描くのに上下の行を一切参照しなくてよい。
- 計算量 O(commits × active_lanes)。`debug_assert!` で行内不変条件を自己検査。
- テストは `tests/graph_layout_test.rs` に **12 ケース**(linear, branch+merge, octopus 3-parent, criss-cross,
  parallel long branch, multiple roots + lane reuse, single commit, 3-level nested stress, **実 git2 経由の
  end-to-end**)+ 5 項目の `check_invariants`(行数一致 / edge-kind と lane の整合 / 隣接行 edge 連続性 /
  lane_count 整合 / 行内 edge 重複なし)。**この不変条件チェッカは再設計後も資産として残すべき。**

→ **結論: 層 1(domain)としての graph layout は既に完成度が高い。再設計では「動かさず保全」が原則。**
　唯一 domain として欠けているのは color-stability(後述 §3.5)と incremental relayout(§3.6)。

### 1.2 まだ UI に張り付いている部分(層分離が崩れている箇所)

- **view-model 構築が ui レイヤ内**: `src/ui/commit_list.rs::build_commit_rows(snap: &RepoSnapshot)` が
  `layout()` を呼び、`CommitRow`(graph の lane/edges/lane_count + badges + 表示文字列 + is_head/is_merge)を生成。
  これは「snapshot → 表示用 row」の view-model 化で、本来は **app 層(AppState)寄り**の責務。
  さらに `BadgeKind`/`RefBadge`/`build_badge_map` も同ファイル(=ui)に居る。ref-badge は **domain/app の概念**
  なので層がずれている。
- **`CommitRow` が graph と badge と表示文字列を 1 構造体に混載**: `lane`/`edges`/`lane_count` という
  pure-graph 出力を、`summary`/`author`/`date`(`SharedString`)や `badges` と同居させている。
  graph layout 結果をそのまま row に展開コピー(`r.edges.clone()`)しており、layout の `Vec<GraphRow>` を保持せず
  毎 row に edges を複製している。
- **selection state は KagiApp(=巨大 god-object)に直結**: `src/ui/mod.rs`(**16,775 行**)が
  `selected: Option<usize>`, `rows`, `details`, `diff_cache`, `commit_scroll_handle: UniformListScrollHandle`,
  `branch_targets: HashMap<String, CommitId>`, `commit_row_index: HashMap<CommitId, usize>`,
  `graph_compact: bool`, `graph_scroll_x: f32` を全て保持。`KagiApp::select(index)` が選択トグル + main_diff/
  compare_view クリア + **その場で `fetch_changed_files`(=git2 アクセス)を呼ぶ**(mod.rs:7891)。
  → **UI が選択時に直接 git バックエンドを叩いている**。これは目標 invariant 違反。
- **`selected` が row index(usize)**: snapshot 再読込で行が動くと選択が壊れる。実際 reload は
  `self.selected = None`(mod.rs:2099)で都度リセットし、`prev_commit_id` を退避して reload 後に
  `commit_row_index` で再解決する回避コードがある(mod.rs:2198-2208)。**選択の真の identity は CommitId**
  なのに index で持っているための歪み。
- **WIP row の特別扱い**: `commit_panel_open`(WIP 行選択)が `selected` と別系統で管理され、`select()` 冒頭で
  `commit_panel_open = false` にする(mod.rs:7867)。選択状態が 2 つの真実源に分裂している。

### 1.3 描画(graph_view.rs)— 概ね健全、ただし純粋層と混ざる懸念は薄い

- `graph_canvas(node_lane, edges, visible_lanes, is_head, is_merge, has_badges, scroll_x)` が gpui `canvas` +
  `PathBuilder` で 1 行を描く。geometry helper(`lane_center_x`/`node_center_y`/`node_radius`/`lanes_for_width`)
  は Window 不要に切り出し済で **unit-test 済**(zoom 一律スケールの整合テスト)。
- 色は `theme().lane_color(lane)` =(`src/ui/theme.rs:133`)**`lane_hsl[i % 6]`**。完全に lane index 依存。
- 横スクロール(`graph_scroll_x`)と per-row 水平クリップ(`visible_lanes`)で狭い列でも破綻しない。
  → 描画層は「layout が吐いた lane/edge を素直に絵にする」だけで、再設計でほぼそのまま流用可能。

---

## 2. 参考プロジェクトの実装方針

### 2.1 lane algorithm(gitk / git log --graph / Sourcetree / GitKraken 系)

- **gitk / `git log --graph`**: topo order 走査 + active column 集合の引き継ぎ。Kagi の ADR-0003 はこれを踏襲済。
  first-parent が幹を継承して branch の主線が縦に安定する点も同じ。Kagi の実装は **既に業界標準系**で、
  「参考にして作り直す」必要は無い(むしろ Kagi 側が教科書実装)。
- **GitKraken**: 角丸コーナー edge(Bézier)・HEAD ノード強調・merge ノードの差別化。Kagi は `graph_view.rs` で
  既に角丸 Bézier(`draw_into_node`/`draw_out_of_node`, `CORNER_R` clamp)・HEAD リング・merge ダブルサークルを実装済。
- **color stability(参考各 client の通弊)**: lane index ベースの配色は履歴更新で色が踊る。GitKraken 等は
  ブランチ単位で色を割り当てて安定させる傾向。Kagi の現状は index % 6 で **不安定**(ADR-0003 の既知 risk)。

### 2.2 仮想化(virtualized list)— gpui / gpui-component / Zed

- **gpui `uniform_list`(現採用)**: 全行同一高さ前提の組み込み仮想化。`range` で可視部分だけ closure を呼ぶ。
  `UniformListScrollHandle` + `.track_scroll()` + `scroll_to_item(ix, ScrollStrategy::Center)`。
  Kagi の commit list / oplog / main-diff-list は全てこれ。10k+ 行でも可視行だけ描くので O(viewport)。
- **gpui-component `VirtualList` / `v_virtual_list`**(`docs/research/gpui-component-audit.md` §2.9):
  uniform_list の上位互換(可変高さ対応 + 独自スクロールハンドル)。
- **gpui-component `List`/`Table`**: `ListDelegate`/`TableDelegate` 抽象。**audit の結論は「commit list は現状維持」**
  (§2.9): Kagi の per-row graph canvas + ref badge + 選択 + 詳細連動を `ListItem` 固定描画に乗せる利得が無く
  リスク大。一方 **`Scrollbar` は uniform_list のハンドルにそのまま被せられる**(§2.10、高優先・低コスト・
  レイアウト非破壊)。
- **Zed の list 仮想化 / GitPanel**(`docs/research/zed-gpui-reuse-research.md`): Zed workspace/git_ui は
  **GPL-3.0+ でコードコピー不可・パターンのみ**。設計言語(可視 range だけ描く、scroll handle を view が持つ)は
  gpui の `uniform_list` と同根なので、Kagi は既にこの利点を得ている。Zed コードの取り込みは不要。

### 2.3 ref badge / selection 参照

- Zed GitPanel・GitKraken: ref(branch/remote/tag/HEAD)を commit 行にチップ表示。Kagi の `RefBadge`/`BadgeKind`
  は既に同等(HeadBranch/Branch/Remote/Tag、HEAD は branch チップに `✓` 統合、detached は専用 HEAD チップ)。
- selection は **どのクライアントも「選択 = ref/commit identity」で保持**し、行 index では持たない(再読込耐性)。

---

## 3. 採用すべき設計

### 3.1 graph layout は domain に据え置き(移動・改変最小)

`src/graph/` を **domain crate(または domain module)へそのまま昇格**。`layout()` の入力 `&[Commit]`・出力
`GraphLayout` は不変。`tests/graph_layout_test.rs` の `check_invariants` を **domain の回帰ゲート**として維持。
git-backend が topo-order の `Vec<Commit>` を供給し、domain がそれを `GraphLayout` に変換する一方向の流れを固定する。
**UI は `GraphLayout`(または派生 view-model)を読むだけで git2 にも graph アルゴにも触れない。**

### 3.2 view-model を app 層へ引き上げ、graph 出力を複製しない

- `build_commit_rows` / `build_badge_map` / `RefBadge` / `BadgeKind` を **ui から app(AppState)へ移動**。
  ui は表示専用の薄い row view を受け取るだけにする。
- `GraphLayout` を **AppState が 1 つ保持**し、row には「graph row への参照(index は GraphLayout の row と同じ
  順序なので暗黙対応)」だけ持たせ、`edges.clone()` の毎 row 複製をやめる。描画は `graph.rows[i].edges` を借用。
- 表示文字列(summary/author/date)・badge・avatar は表示 view-model 側。graph 幾何は GraphLayout 側。**両者を
  別構造体に分離**(現 `CommitRow` の混載を解消)。

### 3.3 仮想化戦略(10k+ rows)

- **`uniform_list` 継続を採用**(同一行高 = Kagi の commit 行は単一高さ。compact/通常の 2 値は再読込時固定)。
  10k+ でも O(viewport) で描画コストは行数に依存しない。layout の O(commits×lanes) 事前計算が唯一の全行コストだが
  ADR-0003 通り 10k で数十 ms。
- **`gpui_component::Scrollbar` を被せる**(audit §2.10 高優先):`commit_scroll_handle` をそのまま
  `Scrollbar::vertical(&handle)` に渡し、レイアウト非破壊でスクロールバーを得る。
- **読み込み上限 10k を維持**(ADR-0003)。それ以上は §3.6 incremental / lazy log で対処。
- `List`/`Table`(gpui-component)への置換は **採用しない**(§4、audit §2.9 と整合)。

### 3.4 selection-state の所有 — app 層が CommitId で持つ

- **真実源を app 層に 1 本化**。`selected` を **`Option<usize>` から `Option<CommitId>`(または専用
  `Selection { Wip, Commit(CommitId) }` enum)へ変更**。これで reload 後の index 退避/再解決ハック
  (mod.rs:2198-2208)が不要になり、WIP 行も `Selection::Wip` として **同一 enum に統合**(`commit_panel_open`
  という別フラグを廃止)。
- **選択時に git2 を呼ばない**:現 `select()` 内の `fetch_changed_files`/`fetch_diffstat`(git2 直叩き)は
  app→git-backend 経由の **非同期 fetch + キャッシュ**へ移す。ui の click handler は app に「選択変更」意図を
  送るだけ。`diff_cache`/`diffstat_cache` は app 層の cache として保持。
- `commit_row_index: HashMap<CommitId, usize>` は CommitId→可視 index 解決のために app 層で維持(scroll_to_item
  / 選択ハイライト用)。`branch_targets` も app 層。

### 3.5 ref-badge model

- `RefBadge { kind: BadgeKind, label }` の形は維持しつつ **app 層へ移設**。`build_badge_map` が
  `RepoSnapshot`(branches/remote_branches/tags/head)から `HashMap<CommitId, Vec<RefBadge>>` を作る現方式は
  健全(commit 行に O(1) 引き当て)。HEAD の `✓` 統合・detached HEAD チップ・remote `remote/name` 表記も維持。
- 将来拡張:同一 commit に多数 ref が付く場合の折りたたみ/+N 表示、ブランチ色との連動(§3.5 と §3.5color)。

### 3.6 color stability

- **現状は lane index % 6 で不安定**(履歴更新で色が踊る、ADR-0003 既知 risk)。
- **採用方針**: 配色キーを lane index から **branch identity(first-parent chain / ref 由来の安定キー)へ
  寄せる**。domain の `GraphRow` に「安定色キー(例: lane を起こした tip commit の安定ハッシュ、または
  branch lineage id)」を **オプションで付与**し、`theme().lane_color()` の入力をそのキーにする。
  これは domain-pure で計算可能(git2 不要)なので層を汚さない。
- ただし **MVP(v1.0 初手)では index 配色のまま据え置き可**。color stability は「domain に色キーを足す」
  追加であって、描画/層構造を壊さない。優先度は中。

### 3.7 incremental vs full relayout

- **MVP は full relayout を維持**(snapshot 再取得の都度 `layout()` を 1 回)。10k で数十 ms、refresh は
  ユーザ操作後/watcher/手動なので許容(ADR-0019 refresh policy と整合)。
- **incremental は later**(ADR-0003 の "later")。`active` 状態を先頭から差分再計算する余地はあるが、edge が
  行内完結なので「上から N 行だけ変わった」差分検出が必要で複雑。**v1.0 では非採用、設計上の拡張点として
  GraphLayout に再計算 API の余地だけ残す**(関数純粋なので後から差分版を足せる)。

---

## 4. 採用しない設計

- **gpui-component `List`/`Table` への commit list 置換**: per-row graph canvas/badge/選択/詳細連動が
  `ListDelegate`/`TableDelegate` の抽象に乗らず利得 < リスク(audit §2.9 と一致)。`uniform_list` 直を維持。
- **Zed git_ui / workspace コードのコピー**: GPL-3.0+。パターン参照のみ(zed research と一致)。
- **汎用グラフレイアウト(Sugiyama 等)/ `git log --graph` ASCII パース**: ADR-0003 で却下済(過剰・遅い /
  構造情報喪失)。再設計でも踏襲。
- **selection を行 index で持つ現方式**: reload 脆弱。CommitId/enum へ置換(§3.4)。
- **graph layout の書き直し**: 完成度が高く不変条件テスト付き。**触らない。**
- **incremental relayout を v1.0 で導入**: 複雑度に見合わない(§3.7)。

---

## 5. リスク

- **R1 god-object 解体の波及**: `selected`/`rows`/`details`/`diff_cache`/scroll handle/`graph_*` が
  16,775 行の `KagiApp` に密結合。selection を CommitId 化 + view-model を app 層へ移す改修は touch 面が広い。
  → 段階移行(まず graph layout を据置確認 → view-model 抽出 → selection enum 化)。
- **R2 selection identity 変更の互換性**: index→CommitId への変更で reload/scroll_to_item/WIP 行の
  全経路を更新要。`commit_row_index` の解決タイミング(reload 後)に注意。headless ログ(`[kagi] selected:` 等)
  の互換維持。
- **R3 UI からの git2 直叩き除去**: `select()` 内 fetch を非同期化すると、選択直後に diff が無い瞬間が出る。
  loading state(`Spinner`/`Skeleton`, audit §2.12)で埋める設計が要る。
- **R4 color stability 導入時の見た目変化**: 既存スクリーンショット/テストの色期待値がずれる可能性。
  MVP 据置で回避可。
- **R5 row 高の二値(compact/通常)**: `uniform_list` は同一高さ前提。compact 切替は全行再構築なので OK だが、
  将来「行ごとに高さが違う」要件が出ると `VirtualList`(可変高)へ移行が必要。

## 6. 未解決事項

- **Q1 domain の置き場所**: 単一 crate 内の `graph` module のままか、workspace を分割して `kagi-domain` crate に
  切るか(他 sub-agent の crate 分割方針と要すり合わせ)。
- **Q2 view-model の所有者**: `GraphLayout` + badge map + 表示文字列 view を AppState が直持ちか、
  「RepoView」中間構造に束ねるか(selection・diff_cache と同居させる単位の設計)。
- **Q3 color-stability の安定キー定義**: branch lineage をどう同定するか(first-parent tip / ref 名 / 安定ハッシュ)。
  domain で完結できるが「どの tip がどの色」の決め方が UX 判断を含む。
- **Q4 selection enum の射程**: `Selection { Wip, Commit(CommitId) }` に compare(2 コミット選択)や複数選択を
  将来含めるか(ADR-0026 compare view との関係)。
- **Q5 incremental relayout の必要性閾値**: 10k 上限で full relayout が体感問題になる実測がまだ無い。
  プロファイル後に判断(現状は数十 ms 想定で非採用)。
- **Q6 大量 ref(同一 commit に多数 branch/tag)の badge 折りたたみ UX**: モデルは Vec<RefBadge> で持てるが
  表示ポリシー(+N 折りたたみ等)未定。
