# 07 — Repo Tabs / Workspace & Session State (research)

Sub-agent #7 / Kagi v1.0 re-architecture (PM-led). RESEARCH ONLY.

Domain: repo tabs, workspace/session state, multi-repository management,
stale-while-revalidate (SWR) cache, async tab loading, FS watcher.

Sources read: `src/ui/tabs.rs`, `src/ui/watcher.rs`, the tab/session fields in
`src/ui/mod.rs` (`KagiApp` ~L1281, `TabViewState` ~L1638, `build_tab_view`
~L1661, `apply_tab_view` ~L2166, `reload`/`reload_external`, `switch_repo`,
`load_repo_async`, `close_tab`, `save_session`, `restore_saved_session`,
`arm_watcher`). ADRs 0027 / 0028 / 0030 / 0019. Reference: `docs/research/
zed-gpui-reuse-research.md`, `docs/research/gpui-component-audit.md`,
`docs/research/openlogi-learnings.md`, Zed `crates/workspace` persistence.

---

## 1. Kagi 現状

### 1.1 状態モデル — active-vs-cache split(中心的な負債)

ADR-0027 が選んだのは「**軽量 tab 記述子 + 単一の重量状態**」:

- `tabs: Vec<RepoTab>` / `active_tab: usize`(`RepoTab { path: PathBuf, name: String }`、`tabs.rs:36`)。
- **active な 1 repo 分の状態だけ** `KagiApp` のトップレベルに**直に**展開されている:
  `header / rows / details / selected / branches / remote_branches / tags /
  stashes / is_dirty / branch_targets / commit_row_index / status_summary /
  toolbar_state / branch_upstream_info / worktrees` ほか、加えて
  diff/modal/panel 系の transient(`diff_cache / diffstat_cache / main_diff /
  compare_view / *_modal / commit_panel*`)。
- **非 active tab の状態**は `tab_cache: HashMap<PathBuf, TabViewState>`(`mod.rs:1533`)
  に純データのみで保持(ADR-0030)。`TabViewState`(`mod.rs:1638`)は
  `header/rows/details/branches/stashes/is_dirty/branch_targets/
  commit_row_index/status_summary/toolbar_state/remote_branches/tags/
  branch_upstream_info/worktrees` の **snapshot 由来 owned/Send データのみ**。
  Entity/handle/scroll/transient は含まない。

→ **同じ「per-repo display data」が 2 つの形で二重定義**されている:
(a) `KagiApp` のトップレベルフィールド群(active 用)と
(b) `TabViewState`(inactive 用)。両者を橋渡しするのが
`build_tab_view`(snapshot → TabViewState, 純関数・Send)と
`apply_tab_view`(TabViewState → self へ**1 個ずつ代入**, main スレッド)。
フィールドを 1 つ足すたびに **struct 定義・build・apply・(welcome/from_snapshot)
の 4〜5 箇所**を手で同期する必要があり、ドリフトの温床。これが今回の主敵。

加えて transient UI 状態(selection / diff_cache / modals / commit_panel)は
**cache に乗らない** → tab を戻すと選択もスクロールも diff も失われる
(ADR-0027 が「tab ごとの selection・scroll 保持は later」と明記)。

### 1.2 switch_repo フロー(SWR + 世代ガード)

`switch_repo(index)`(`tabs.rs:104`):
1. `active_tab = index`、`repo_path` を差し替え、`reset_per_repo_ui()` で
   transient(selection/diff/modal/panel)を**全消し**。
2. `switch_generation` を `wrapping_add(1)`(連打ガード)。
3. `tab_cache.get(path)` ヒット → `apply_tab_view` で**即時 swap(0 フレーム)**;
   ミス → `loading_tab = Some("Loading …")` + `FooterStatus::Busy`。
4. `save_session()` → `arm_watcher(cx)` → `cx.notify()`。
5. `load_repo_async()` で background snapshot → 完了時に generation 一致を確認して
   `tab_cache.insert` + `apply_tab_view` + loading 解除(superseded なら破棄)。

`load_repo_async`(`tabs.rs:180`)は `cx.background_spawn` で
`git2::Repository::open` → `kagi::git::snapshot(.., 10_000)` → `build_tab_view`、
`cx.spawn` で main に戻す。**git2 を直接叩いているのは ui レイヤ**(レイヤ違反)。

### 1.3 reload / reload_external

- `reload()`(`mod.rs:2061`)は **同期** snapshot(ADR-0019)→ `build_tab_view`
  → `tab_cache.insert` も更新 → `apply_tab_view`。
- `reload_external()`(`mod.rs:2194`)= watcher 起因。`selected` を CommitId で
  退避 → `reload()` → CommitId で再選択を試行 → `[kagi] refreshed (external change)`。
- ADR-0030 §6 が「watcher 起因 reload も background 経路に」を **stretch/later** と
  していて、現状は同期のまま(大 repo で auto-refresh がジャンク)。

### 1.4 世代カウンタ 2 系統(独立)

- `switch_generation: u64` — tab 切替の連打で**最後の switch だけ適用**(古い
  background snapshot 結果を破棄)。`load_repo_async` の guard。
- `watcher_generation: u64` — switch/open/close ごとに bump(`arm_watcher`,
  `close_tab`)。旧 watcher loop は generation 不一致を検知して自然終了
  (`tabs.rs:375` の loop、100ms ポーリングで `watcher_generation` を read)。

### 1.5 watcher(per active repo のみ)

`src/ui/watcher.rs`: `notify::RecommendedWatcher` を `<repo>/.git` に貼り
`RecursiveMode::Recursive`、`objects/` を skip、`HEAD/refs/packed-refs/index/
MERGE_HEAD/...` を relevant 判定、`mpsc::Receiver<()>` で coalesce 通知、
DEBOUNCE 500ms。`arm_watcher`(`tabs.rs:360`)が generation を bump して spawn。
**監視するのは active repo の 1 個だけ** → 非 active tab は stale を許容し、
切替時の revalidate で回復(ADR-0030 §5)。watcher 不能時(inotify limit 等)は
`None` を返して no-op。

### 1.6 terminal sessions(tab 横断・per-path)

`terminal_sessions: HashMap<PathBuf, KagiTerminalSession>`(`mod.rs:1430`)で
**tab を跨いで PTY を生存**(ADR-0027)。bottom panel は active repo の session を
表示、lazy 生成。`close_tab` で当該 path の session を remove(drop で PTY close)。
→ ここだけは既に「per-path に独立した session を持つ」正しい形。tab_cache も
terminal_sessions も path をキーにするので、**path を session ID とする**設計が
事実上の現状コンセンサス(canonicalize 済み path)。

### 1.7 persistence(open tabs の復元)

`save_session()`(`tabs.rs:243`)= `session_repos`(path を U+001F join)+
`session_active`(index)を settings.json へ。`KAGI_NO_RESTORE=1` で無効化。
`restore_saved_session()`(`tabs.rs:601`、pre-window・Context 無し)= split →
`open_repository` 検証 → 開けた path だけ tab 化 → active clamp → `reload()`(同期)。
**保存するのは path と active index のみ**(selection/scroll/dock/per-tab UI は無し)。

### 1.8 テストギャップ(明示)

`tabs.rs` / `watcher.rs` / switch・SWR・generation・session 復元に対する
**直接テストは皆無**。headless ログ(`[kagi] tabs:`, `[kagi] tab-switch:`,
`[kagi] tab-load:`, `[kagi] refreshed (external change)`)に依存。
re-arch では「session lifecycle / generation supersede / SWR 鮮度回復 /
session restore の skip-invalid」を**ユニットテスト対象**に格上げすべき。

### 1.9 現状の構造的問題まとめ

1. active(トップレベル) vs cache(`TabViewState`)の**二重定義**。
2. transient UI(selection/scroll/diff/modal)が cache に乗らず**切替で消える**。
3. ui レイヤが **git2 を直接** open/snapshot(`load_repo_async`)→ 目標レイヤ違反。
4. watcher / generation / session 永続が `KagiApp` の平坦フィールドに散在し、
   「1 repo = 1 まとまった状態」という単位が**型に存在しない**。
5. reload_external 同期(大 repo ジャンク)。

---

## 2. 参考プロジェクトの実装方針

### 2.1 Zed — Workspace / Pane / Item / Project(study only, GPL)

`docs/research/zed-gpui-reuse-research.md` と Zed `crates/workspace`
persistence より(**コード流用不可、設計言語のみ**):

- **階層**: `Workspace` ⊃ `PaneGroup`(`Member { Pane | Axis }` の再帰ツリー、
  H/V split + flex 比)⊃ `Pane` ⊃ `Item`(tab)。kagi の「repo tab strip」は
  Zed の **Pane 内 Item tab に相当**するが、kagi では split は不要 →
  **PaneGroup ツリーは過剰**(採用しない、§4)。
- **Workspace ↔ Project**: `Workspace` は UI/レイアウト、`Project` は
  ファイル・worktree・言語サーバ等の**データ/バックエンド**。Kagi に写すと
  **Workspace = UI(tab strip + active session の view)、Session/Project =
  per-repo の backend + 導出データ**という分離になる。これは PM 指定の
  TARGET LAYERING(app が N session を所有、ui は active session の view)と一致。
- **永続化**(persistence.rs): SQLite に
  workspace(paths/identity/window bounds)→ pane_groups(tree/axis/flex)→
  panes(`active` flag)→ items(`active`/`position`/kind/preview)を階層保存。
  **identity paths = git worktree root** で重複排除。復元は逆順に再帰再構築 +
  順序通り item を rehydrate。**active は pane と item の両方に boolean/position**。
  → kagi の `session_repos`+`session_active`(path+index のみ)を、将来
  「per-tab の selection / scroll / dock 状態」まで含む**構造化スキーマ**へ
  拡張する設計の手本。ただし SQLite は重い → kagi は settings.json/JSON で十分。

### 2.2 gpui-component — Dock / TabBar(`docs/research/gpui-component-audit.md`)

- `dock/*`(`DockArea`/`Dock`/`Panel`、`E+R+登録`)は **ADR-0034 で「重い・
  Study only」既決**。kagi の bottom panel/tabs は自前で足りる。
- `tab/tab_bar.rs` の `TabBar`/`Tab`(**stateless, state は呼び出し側保持**)は
  「tab strip 代替」候補(audit §2.4)。`Tab::new().label().icon().suffix()
  .selected().on_click()`、`TabBar::new(id).selected_index(i)`。
  → kagi tabs.rs の自前 strip(W6-TABSPEED 最適化済)を捨てる判断が要るため
  **現状維持寄り(採用は任意)**。re-arch では「描画は TabBar で代替可、状態は
  `Workspace` 側が保持」という**状態と描画の分離**だけ確定させておけばよい。
- `resizable/`(`ResizablePanelGroup`/`ResizableState`)は **外部保持で永続化可**
  (audit §2.6 `ResizableState.sizes()` を session に保存)。将来の per-session
  divider 比率の永続化に使える。

### 2.3 OpenLogi — AppState global / view-local route(`openlogi-learnings.md`)

- **単一 `AppState` を gpui global**(`impl Global`, `set_global` を最初の window
  前に)、`try_global` 読み / `update_global` 書き / `observe_global` 購読。
- 「**gpui にルータは無い、ナビは小さな view-local enum で十分**」。
  → kagi の Welcome ⇄ repo 表示の切替も view-local enum(`tabs.is_empty()`)で
  足りている。AppState global は「N session を所有する app レイヤ」を
  **gpui global として 1 つ持つ**指針として採用候補(TARGET の app レイヤに合致)。
- WindowRegistry / single-instance / config / theme sync 等は移植可。

---

## 3. 採用すべき設計

### 3.1 中核 — `RepoSession`:tab = 完全自己完結のセッション

active/cache 二重定義を**廃止**し、**全 tab を同型の `RepoSession`** で持つ。
active も非 active も**同じ型**で、唯一の違いは「active が view layer に
bind されているか」だけ。

```text
// app レイヤ(git2 を直接触らない。git-backend 経由)
AppState {
    sessions: Vec<RepoSession>,   // 順序 = tab strip 順
    active: Option<usize>,        // None = Welcome
    switch_generation: u64,       // 連打 supersede(現状維持・session 外の app 単位)
}

RepoSession {
    id: SessionId,                // = canonicalized PathBuf(現状の事実上のキー)
    name: String,
    backend: RepoBackend,         // per-repo git-backend instance(snapshot 供給)
    data: SessionData,            // ← 現 TabViewState 相当(snapshot 由来 純データ)
    ui: SessionUiState,           // ← 現「トップレベル transient」相当(selection/
                                  //    scroll/diff_cache/diffstat/main_diff/compare/
                                  //    modals/commit_panel)を session 内へ移動
    terminal: Option<TerminalSession>, // 現 terminal_sessions[path] を session へ内包
    watcher: Option<RepoWatcher>, // §3.4
    freshness: Freshness,         // Fresh | Stale | Loading(SWR 状態機械)
    conflict: Option<ConflictState>, // sub-agent #? 領域だが session 内に同居
}
```

ポイント:
- **`apply_tab_view` / `build_tab_view` の橋渡しが消える**。tab 切替 =
  `active = index` を変えるだけの **0 フレーム state swap**(snapshot を main で
  作らない)。「active を self のトップレベルに展開」を**やめる**ことで二重定義が
  原理的に消滅(フィールド追加は `SessionData`/`SessionUiState` 1 箇所のみ)。
- **selection / scroll / diff / modal が tab ごとに保持される**(現状の最大の UX
  欠落を解消、ADR-0027 の "later" を回収)。

### 3.2 レイヤリング(PM 指定に整合)

- **domain**(pure): ほぼ無し(CommitRow 等の純データ型のみ)。
- **git-backend**: `RepoBackend`(per-repo インスタンス)。`snapshot(limit)` を
  `Send` な `RepoSnapshot` で返す。**git2 はここに閉じる**(現 `load_repo_async`
  の ui 内 git2 直叩きを移設)。
- **app**: `AppState` が N `RepoSession` を所有。各 session が自分の
  backend / data / ui / terminal / watcher / freshness を持つ自己完結ユニット。
- **ui**: tab strip(全 session の name/active を描画)+ active session の
  `data`/`ui` を bind した view 群。**ui は git2 を一切呼ばない**(invariant)。
  unsafe な「active を self に展開」は廃し、ui は `app.active_session()` を参照。

### 3.3 session lifecycle

- **open**(`open_session(path)`): canonicalize → 既存 session があれば
  `activate` のみ。無ければ backend 検証(現 `open_repository`、失敗時は
  tab を作らず toast+footer)→ `RepoSession::new(Loading)` を push → activate →
  async load 起動。
- **activate**(`switch_repo` 後継): `active = idx` + 0 フレーム swap。
  `freshness` を見て:Loading なら placeholder、Fresh/Stale なら即表示しつつ
  **Stale なら revalidate 起動**(SWR、§3.5)。`switch_generation` は app 単位で
  維持(古い load を supersede)。**watcher は §3.4 の方針次第**。
- **close**(`close_session`): session を `Vec` から remove(drop で PTY/watcher
  も落ちる ⇒ 現状の手動 `terminal_sessions.remove` + `watcher_generation` bump が
  **RAII で自動化**される)。空なら Welcome、非空なら active を clamp/左シフト。
- persistence は activate/open/close で `AppState::save_session()`(§3.6)。

### 3.4 watcher per session(2 案、推奨は B)

- **A: active のみ監視(現状踏襲)**。リソース最小。非 active は stale 許容で
  切替時 revalidate。generation スキームは session.watcher を RAII 化すれば
  `watcher_generation` カウンタ自体が不要になる(session drop で watcher 終了)。
- **B(推奨): 全 session が自分の watcher を持つ**。各 `RepoSession` が
  `RepoWatcher` を所有し、外部変更で**その session の `freshness = Stale`** を
  立てるだけ(非 active は即 reload しない=ジャンク回避)。active session が
  Stale になったら background revalidate を起動。これで「切替するまで気付かない」
  欠点が消え、tab に dirty/stale バッジを出せる。コスト = repo 数ぶんの watcher
  (inotify limit に注意 → 失敗は no-op、A へフォールバック)。
- どちらでも **generation カウンタは RAII(session drop)へ置換**できる。
  switch_generation(async load supersede)は app 単位で残す。

### 3.5 async load + SWR(現 ADR-0030 を session 内へ)

- load は **git-backend の background snapshot**(`cx.background_spawn`)→
  `SessionData` 構築 → main で当該 session に格納(`switch_generation` 一致時のみ)。
  ui 内の git2 直叩きは backend へ移設。
- **SWR 状態機械**を `Freshness` で明示:`Loading`(初回・data 無し→placeholder)
  / `Fresh`(直近 revalidate 済)/ `Stale`(watcher が立てた・即表示+裏で更新)。
  現状の「cache hit=instant, miss=loading」を型で表現し直したもの。
- reload_external も同経路(ADR-0030 §6 の stretch を**回収**):watcher → Stale
  → background revalidate → selection は CommitId で再 bind(現 `reload_external`
  のロジックを session.ui に移植)。**大 repo の auto-refresh ジャンクも解消**。

### 3.6 persistence of open tabs(構造化)

- 最小は現状維持(path リスト + active index)。re-arch では Zed に倣い
  **per-session の軽量 UI 状態まで**構造化保存(JSON 推奨、SQLite は過剰):
  `[{ path, selected_commit_id?, scroll?, active_panel? }] + active`。
  - selection は **index でなく CommitId** で保存(reload で index がズレるため。
    現 `reload_external` が既に CommitId 退避をしている前例)。
  - divider 比率は gpui-component `ResizableState.sizes()` を session に同梱可。
- restore は pre-window 経路を維持(`restore_saved_session`)。開けない path は
  skip。`KAGI_NO_RESTORE=1` は維持。**復元時の selection 再 bind / skip-invalid /
  active clamp をユニットテスト化**(§1.8 のギャップ解消)。

### 3.7 段階移行(構造の大手術を避ける — ADR-0030 §Consequences と整合)

1. `SessionData`(= 現 `TabViewState`)+ `SessionUiState`(現トップレベル
   transient)を struct として括り出す。
2. `KagiApp` のトップレベル per-repo フィールドを `active_session().data/ui`
   への委譲に置換(`apply_tab_view` を削除、ui は session 参照へ)。
3. git2 直叩き(`load_repo_async`)を git-backend に移す。
4. terminal_sessions / watcher / conflict を session に内包、generation を RAII へ。
5. persistence を構造化、テスト追加。
各段は単独でコンパイル可能・headless ログ互換を保つ。

---

## 4. 採用しない設計

- **Zed の PaneGroup(再帰 split ツリー)**: kagi に repo の同時 split 表示要件は
  無い。tab strip(1 次元)で十分。過剰な複雑さ。
- **gpui-component Dock/DockArea**: ADR-0034 で Study only 既決。重い・登録式
  パネルは bottom panel/tabs を自前実装済の kagi に不要。
- **TabBar への即時全面移行**: W6-TABSPEED で最適化済の自前 strip を捨てる
  コスト・UX 差分があり、re-arch の本筋(状態モデル)とは独立。描画刷新は別タスク。
- **SQLite 永続化(Zed 方式)**: 数 tab + 軽量 UI 状態に SQLite は過剰。
  settings.json/JSON で足りる。
- **active を `KagiApp` トップレベルに展開し続ける(現状)**: 二重定義の元凶。廃止。
- **focus/timer ポーリング refresh**: ADR-0019 で却下済(watcher で必要十分)。維持。
- **全 session を常時 background reload**: メモリ/CPU の無駄。Stale フラグ +
  revalidate-on-activate(または active のみ即時)で十分。

## 5. リスク

- **大リファクタの波及**: トップレベル per-repo フィールドは UI 全域から参照され
  ており(header/rows/details/selected/...)、`active_session()` 委譲への置換は
  広範囲。段階移行(§3.7)と headless ログ互換で緩和。
- **メモリ**: 全 tab が `SessionData`(最大 10k commits × rows/details)+ 案 B で
  watcher を保持 → tab 数に比例。実用規模では問題ない見込み(ADR-0030 §2)が、
  多数 tab で要計測。必要なら非 active の data を LRU で drop(Loading に戻す)。
- **inotify/FSEvents limit**(案 B): repo 数ぶん watcher → limit 超過の可能性。
  失敗を no-op で握り潰す現状方針を維持しつつ案 A フォールバック。
- **generation → RAII 置換**の取りこぼし: drop タイミングと in-flight task の
  競合。switch_generation(async load supersede)は app 単位で**残す**のが安全。
- **selection 永続を CommitId 化**:大 history で row index 解決のコスト
  (`commit_row_index` は既に build 済なので軽い見込み)。
- **テスト不在(§1.8)**: リグレッション検知が headless ログ頼み。re-arch と
  同時にユニットテストを入れないと壊れたまま気付けない。

## 6. 未解決事項

1. watcher は **案 A(active のみ)/ B(全 session + Stale バッジ)**どちらか。
   tab に未読変更バッジを出す UX を v1.0 に入れるかで決まる。
2. **非 active session の data 保持ポリシー**:常時保持(現状)か LRU drop か。
   tab 数の想定上限が要る。
3. **persistence のスコープ**:path+active のみ(現状)か、selection/scroll/
   panel/divider まで構造化か。後者なら CommitId 化と divider state 同梱が前提。
4. **terminal/conflict を session 内包**する際の所有権:他 sub-agent(terminal /
   conflict 領域)との境界調整が必要(本 doc は「session に同居させる」方針のみ提示)。
5. **TabBar 採用**を v1.0 でやるか(描画刷新)後回しか — 状態モデルとは独立に決定可。
6. **`AppState` を gpui global にするか**(OpenLogi 流)、`Entity<KagiApp>` 直保持の
   ままにするか。global 化は単一 window 前提と multi-window 将来要件に依存。
7. reload/snapshot を**完全 async 化**(ADR-0019 は「当面同期」)する範囲 —
   switch/external は async、手動 Refresh と起動時 restore も async にするか。
