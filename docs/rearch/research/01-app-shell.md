# 01 — App Shell（GPUI 全体構造 / window・menu・command/action・app shell 責務)

- 調査日: 2026-06-14 / research subagent #1（Kagi v1.0 re-architecture, PM-led）
- 対象ドメイン: GPUI アプリ全体の骨格 — window / sub-window、menu bar、command/action 設計、app shell の責務分担
- 関連 ADR: 0012(app shell layout)/ 0029(command registry & menubar)/ 0013(header toolbar UX)/ 0027(repo tabs)/ 0028(repo picker)/ 0030(async repo loading)/ 0031(外部流用)/ 0034(zed/gpui reuse)/ 0036(themes)/ 0038(bundling)
- 既存リサーチ前提（再調査しない）: `docs/research/zed-gpui-reuse-research.md` / `gpui-component-audit.md` / `openlogi-learnings.md`
- v1.0 が目指す層: **domain(pure) → git-backend(trait + git2/CLI adapter) → app(AppState / OperationController / async・cancel / persistence) → ui(view-model + GPUI view + commands/actions)**
- 最重要不変条件: **UI は git2::Repository を絶対に開かない／git2:: を直接呼ばない**

---

## (1) Kagi 現状 — 具体ファイル/構造と「KagiApp に絡まっているもの」

### 1-1. エントリ・window 生成・menu 配線（`src/main.rs` 1457 行 / `src/ui/mod.rs::run_app` L16547〜）

- `main()` は **巨大な headless ハーネス**になっている（L202–1457）。`KAGI_*` 環境変数で plan→(auto-confirm 時)execute→verify を駆動するブロックが ~30 個並ぶ（`KAGI_PULL` / `KAGI_PUSH` / `KAGI_AMEND` / `KAGI_CHERRY_PICK` / `KAGI_REVERT` / `KAGI_CREATE_BRANCH` / `KAGI_PLAN_WORKTREE` / `KAGI_STASH_*` / `KAGI_DISCARD*` / `KAGI_COMMIT_*` …）。**main.rs だけで `git2::` を 28 回直呼び**（`git2::Repository::open` を各ブロックで何度も開き直す）。GUI ロジックとテスト用フックが同居している。
- window 生成: `run_app(app_state)`（L16547）が `Application::new()` →（`on_reopen` で Dock 再オープン時に session 復元）→ `application.run(|cx| { gpui_component::init(cx); theme::sync_gpui_component_theme(cx); cx.bind_keys([cmd-j, escape, up, down]); commands::register_keybindings(cx); cx.set_menus(commands::build_menus()); open_main_window(app_state, cx); })`。
- `open_main_window`（L16612）が `cx.open_window(WindowOptions{...}, |window, cx| { cx.new(|cx| { app_state.root_focus = Some(cx.focus_handle()); app_state }); ...; cx.new(|cx| gpui_component::Root::new(kagi, window, cx)) })`。**window は 1 枚固定**。gpui-component widget が動くよう **第一層は必ず `gpui_component::Root`**(直接 KagiApp を描くと `Root::read` で panic、ユーザ報告済)。`KAGI_WINDOW=WxH` で初期サイズ override。
- **sub-window は存在しない**。About / Keyboard Shortcuts / branch picker はすべて KagiApp 内の自前 `MenuOverlay`（`commands.rs` L656–668 / `wrap_overlay` で半透明バックドロップ + 中央パネル）として描画。マルチウィンドウは `window.new` が `Disabled(MultiWindowUnsupported)`（ADR-0029 placeholder）。
- watcher: `arm_watcher`（`tabs.rs` L360）が `.git` FS watcher を generation scheme で張り直す（switch/open/close で世代 bump → 旧ループ自滅）。`init_tab`（main.rs L171）は GUI context を持たないので `switch_repo` を使えず、tab を手で push して `reload()`。

### 1-2. Command Registry / メニュー（`src/ui/commands.rs` 1127 行・ADR-0029）

- **ここは現状で最もきれいに分離できている層**。単一の正準テーブル `COMMANDS: &[Command{ id, label, keystroke, dangerous }]`（L169）、tri-state `CommandState{ Enabled / Disabled(reason) / Hidden }`、`command_state(app, id)`（L243）に **enabled/disabled 判定を一元化**。menu / keybinding / context menu / toolbar / (将来の)palette が全部ここを参照する設計。
- gpui ネイティブ: `actions!(kagi_menu, [...])`（L52、command ↔ Action は 1:1）、`build_menus() -> Vec<Menu>`（L381、純関数）、`register_keybindings(cx)`（L581）。**disabled = ハンドラ未登録**で表現（macOS の `is_action_available` が dispatch tree を歩く gpui 0.2.2 挙動を利用、module doc L11–30 に検証記録）。
- `register_menu_actions`（mod.rs L9715）が `menu_act!` マクロで **全 action を `command_state==Enabled` のときだけ root div に条件登録**。ハンドラは全て `handle_menu_command(id, window, cx)`（commands.rs L681）に集約 → 既存の plan→confirm modal / `dispatch_commit_action`(ADR-0022) / tabs に委譲。これは ADR-0029 通り。
- ただし: theme/lang 切替は label に `✓` を焼くため `cx.set_menus(build_menus())` 再呼び（L834/849）。`menu_fetch`(L909) と `menu_open_in_finder`(L890) は **commands.rs の中で直接 `git2::Repository::open` / `std::process::Command::new("open")` を呼んでいる**（registry 層に git2 と OS 呼びが漏れている）。

### 1-3. KagiApp shell の肥大（`src/ui/mod.rs` L1281–1610、`impl Render` L9078–）

- **`KagiApp` は ~111 個の `pub` フィールドを持つ god struct**。1 つの struct に以下が全部入っている:
  - **window/focus**: `root_focus`
  - **per-repo 表示データ**: `header / rows / details / branches / remote_branches / tags / worktrees / stashes / status_summary / toolbar_state / branch_targets / commit_row_index / branch_upstream_info`（= `TabViewState` と重複、L1638）
  - **選択/差分 view 状態**: `selected / diff_cache / diffstat_cache / main_diff / compare_view / *_scroll_handle`
  - **20 個の modal Option フィールド**: `plan_modal / pull_modal / push_modal / undo_modal / amend_modal / pop_modal / merge_modal / create_branch_modal / create_worktree_modal / stash_*_modal / cherry_pick_modal / revert_modal / delete_branch_modal / discard_modal / branch_plan_modal / set_upstream_modal / rename_branch_modal / tracking_checkout_modal / conflict_continue_modal`
  - **tabs/cache**: `tabs / active_tab / tab_cache / switch_generation / loading_tab / watcher_generation`
  - **async/op 状態**: `busy_op: Option<&'static str>`（単一フラグで全 git op を直列化）、`pending_smart_msg`、`modal_replan_gen`、`draft_save_gen`
  - **bottom panel / terminal**: `bottom_panel_open / bottom_panel_height / bottom_tab / terminal_sessions: HashMap<PathBuf, KagiTerminalSession> / op_entries(oplog ring) / oplog_*`
  - **commit panel**: `commit_panel / commit_input / commit_template_* / smart_commit*`
  - **conflict mode（W30〜）**: 約 18 フィールド（`conflict / conflict_editing / conflict_editor_inputs / conflict_*_split / conflict_*_geom / conflict_selected_hunk / conflict_merge_commit_pending …`）
  - **menu/view toggle**: `sidebar_visible / inspector_visible / menu_overlay`
  - **avatars**: `avatar_images / avatar_fetch_for`
- `impl Render`（L9079）は **単一の巨大 render**。先頭で副作用を多数実行（`window.set_rem_size`、bottom panel 高さ解決、`ensure_avatars`、`detect_conflict_mode`、toast 期限切れ、`sync_modal_inputs`、`pending_smart_msg` 反映）→ error 画面 / welcome / conflict body / normal body を分岐。modal 入力は `cx.new(|cx| InputState::new(window, cx))` で **render 内 lazy 生成**（window が必要なため）。`cx.notify()` は mod.rs だけで **173 箇所**。
- **git2 漏れ（最重要不変条件の違反）**: `git2::` は **mod.rs に 81 回 / tabs.rs / commands.rs / commit_panel.rs / conflict_view.rs / avatar_fetch.rs に各 1+**、`Repository::open` 系の呼び出しは **UI+main 合計で 113 箇所**。view-model 層と backend 層の境界が無く、UI が `git2::Repository::open(&repo_path)` を直に開いて plan/execute を回している。**v1.0 の #1 不変条件は現状ほぼ全面違反**。
- async op の現状: 各 `confirm_*` が `self.busy_op = Some("pull")` を立て、`cx.background_spawn(async move { git2::Repository::open(...); plan/execute })` → `cx.spawn` で main に戻して `app.busy_op = None; reload()`。**cancellation は無く、generation guard も op には無い**（tab switch だけが `switch_generation` を持つ）。busy 中は menu/toolbar を `Disabled(OpInProgress)` にして直列化。

### 1-4. tabs（`src/ui/tabs.rs` 633 行・ADR-0027/0028/0030）

- `RepoTab{ path, name }`（軽量 descriptor）+ 単一 heavyweight per-repo state on KagiApp。`switch_repo`(L104) が stale-while-revalidate（`tab_cache: HashMap<PathBuf, TabViewState>` を即時 swap → background revalidate）。`reset_per_repo_ui`（L156）で modal/選択を手動全クリア（20+ フィールドを 1 つずつ None 代入 — god struct ゆえの定型作業）。`load_repo_async`（L180）は **`TabViewState` という pure・Send データを background で組んで main に apply** する良い分離が既にある（v1.0 の view-model 化の足場）。picker は `cx.prompt_for_paths`（NSOpenPanel）。

---

## (2) 参考プロジェクトの実装方針

### 2-1. Zed（`zed-industries/zed`、`docs/research/zed-gpui-reuse-research.md` より）

- **ライセンス境界が最重要ゲート**: `crates/gpui` のみ **Apache-2.0**（Action / `actions!` / Keymap / KeyBinding / context predicate / Entity / Render / App / Context / elements）→ 概念・API として利用可（Kagi は既に crates.io gpui 0.2.2 依存済）。`workspace`(Panel/Dock/PaneGroup/StatusBar)・`command_palette`・`ui`(43 components)・`terminal`・`git_ui`・`editor` は全て **GPL-3.0+ → コード転写不可、設計パターンのみ**。
- 設計パターンとして学べるもの:
  - **`Panel` トレイト**(workspace/dock.rs): `Focusable + EventEmitter<PanelEvent> + Render` を要求し、`position / default_size / icon / toggle_action / activation_priority` を持つ **登録式パネル + メタ** モデル。dock に runtime 登録。
  - **`PaneGroup`**: `Member{ Pane | Axis }` の再帰ツリーで H/V split、persistence.rs で永続化。
  - **`StatusItemView`**(status_bar.rs): `set_active_pane_item()` で active item 変化を購読して status item を更新。
  - **Action/Keymap**(Apache/gpui): `actions!`（namespace 付き unit struct）、`Keymap`(TypeId→binding)、context predicate、`window.available_actions(cx)` → これが Kagi の Command Registry の正しい土台。
  - **Command palette**(GPL UI): `GlobalCommandPaletteInterceptor` で hook するパターンのみ参考。registry がそのまま供給源になる。
- **結論（既存リサーチ）**: gpui core / Action / Keymap は Adopt（既に使用中）。Panel/Dock/PaneGroup/StatusItemView/palette/context-menu は **Study only**。ui(43 components)/git_ui/editor は **Reject**（gpui-component で代替済）。

### 2-2. gpui-component（`docs/research/gpui-component-audit.md` より）

- 全 widget が色を `cx.theme()`（`ActiveTheme` トレイト = `gpui_component::Theme` global、`ThemeColor` 103 フィールド）から取る。Kagi 自前 `theme()` を single source に保ち、境界で `Theme::global_mut(cx).colors` へ push（`sync_gpui_component_theme`）すれば二重化しない。
- 採用容易度の分類（S=stateless / E=Entity state 必須 / D=delegate / R=Root 必須 / I=init 必須）。**window 第一層に `Root` が必須**(現状 run_app が満たしている)。Input は `Entity<InputState>`（E）で window context 必須 → render 内 lazy 生成という現状の制約はこれ由来。
- app shell 観点: Kagi は既に gpui-component を **toolbar/Input/Tooltip/Scrollbar/Checkbox/CodeEditor** に部分採用。シェルの「枠」（dock/tab/status bar）は自前。

### 2-3. OpenLogi（`/Users/tomixrm/Dev/sandbox/OpenLogi`、`docs/research/openlogi-learnings.md` より）

- ライセンス = **MIT OR Apache-2.0**（コード Port 可。ただし gpui が **zed git main + 別 crate `gpui_platform`** で API がズレる → 多くは「コードでなく手順/設計の移植」）。
- app/window/state の参考パターン（Kagi にそのまま効く）:
  - **`AppState` を gpui global**（`impl Global`、`set_global` を最初の window 前に）。読み `cx.try_global::<AppState>()`（不在許容で中立フレーム）、書き `cx.update_global`、view は `cx.observe_global` を `Subscription` で保持し再描画。
  - **接続/ロード状態を単一 enum**（`AgentLink{ Connecting / Unreachable / Ready(..) }`）で持ち、`bool`/`Option` のミラー散在を型で排除。
  - **ナビは view-local enum**（`enum Route` を view 内に。AppState にルータを置かない。「gpui にルータはない」）。
  - **`WindowRegistry` global + `open_or_focus<V: AuxWindow>` + `AuxWindow: Render` トレイト**で **シングルトン sub-window**(settings / about / add_device)を強制。Dock 再オープンも同 handle を focus。
  - quit-on-last-window（`cx.on_window_closed` で最後の window 閉鎖時 `cx.quit()`）は常駐 agent の無い Kagi にそのまま採用可。
  - config: TOML + `load_or_default` + `save_atomic`（tmp→fsync→rename, 0600）+ `SCHEMA_VERSION` gate + `#[serde(default)] + skip_serializing_if`。`paths.rs`/`config.rs` は **gpui 非依存で最も Port 容易**。single instance = `fs4` advisory lock。
- **しない**: 2 プロセス agent + IPC（HID 常駐ゆえ。Git GUI に過剰）、zed-git gpui 追従、マウスフック/Accessibility、`design/` ブランド資産（All Rights Reserved）。

---

## (3) 採用すべき設計 — Kagi v1.0 app shell / Action・command / window vs view ownership / state 配置

目標層（domain → git-backend → app → ui）に正しく落とす。

### 3-1. 4 層の責務（app shell を「薄い合成層」にする）

- **domain（pure）**: plan / blocker / Head / StateSummary 等の純データと判定。git2 非依存。
- **git-backend**: `trait GitBackend`（snapshot / plan_* / execute_* / preflight / fetch / status …）+ git2 adapter（+ 将来 CLI adapter）。**git2::Repository を開くのはこの層だけ**。`Send` な戻り値（現 `RepoSnapshot` / `TabViewState` は既に Send）。
- **app（新設の中核）**:
  - **`AppState`**: tabs・active tab・per-repo の view-model（後述）・preferences・oplog を保持。OpenLogi に倣い gpui global か `Entity<AppState>` のどちらか（後述 3-4）。
  - **`OperationController`**: 「plan → confirm → preflight → execute → verify → oplog」を **1 本の API に集約**。現状各 `confirm_*` に散る `busy_op = Some(...)` / `background_spawn` / `reload()` / `record_headless_op` を **単一の `run_operation(op, cx)`** に寄せ、`busy_op` を `enum OpState{ Idle, Running(OpKind), … }` に格上げ。**op 単位の cancellation token と generation guard**（tab switch 用 `switch_generation` と同じ仕組みを op にも）を導入。
  - **persistence**: session（現 `save_session`/`restore_saved_session`）・settings を OpenLogi の `config.rs`(atomic save + schema gate) パターンに移行。
- **ui**: GPUI view + commands/actions のみ。**git2 を一切 import しない**（lint/grep gate で機械的に保証、3-6）。

### 3-2. Command / Action system（ADR-0029 を全面採用・拡張）

- **`commands.rs` の registry を v1.0 の正準として維持・強化**。現状この層が一番きれいなので壊さない。改善点:
  - **registry を app 層に置く**（`command_state(state, id)` は AppState/view-model を読むだけの純関数に）。menu / keybinding / context menu / toolbar / **command palette(cmd-shift-p)** が全部ここを参照。palette は registry をそのまま列挙すれば供給源になる（Zed パターン、UI は自前 or gpui-component の Picker(E)）。
  - **command handler は OperationController を呼ぶだけ**にする。`menu_fetch` / `menu_open_in_finder` 内の `git2::Repository::open` / `std::process::Command` を backend / app 層へ追い出す（registry から副作用を排除）。
  - **disabled=ハンドラ未登録** モデル（gpui mac 検証準拠）と **Edit=os_action**、**theme/lang は set_menus 再構築**は現行どおり維持（検証済の良い設計）。
  - context menu / toolbar ボタン（既存 Pull/Push 等）を段階的に registry id 経由へ移行（ADR-0029 follow-up を v1.0 で完遂）。

### 3-3. window vs view ownership

- **メイン window は 1 枚**（現行維持）。第一層は **`gpui_component::Root`** 必須（既存制約）。root focus handle は AppState ではなく **window/view 所有**（key dispatch は focus path）。
- **sub-window は OpenLogi の `WindowRegistry` + `AuxWindow` トレイトを Port** して導入候補:
  - 現状 `MenuOverlay`(About / Keyboard Shortcuts / branch picker) は KagiApp 内の自前オーバーレイ。**About / Settings / Keyboard Shortcuts のような独立 UI はシングルトン sub-window 化**できる（`open_or_focus`）。ただし MVP 必須ではない（branch picker のような repo 文脈に紐づくものは overlay のままが自然）。
  - マルチ repo window（`window.new`）も将来 `WindowRegistry` で実現可能。各 window が `Entity<AppState>` の weak を持つ形が素直。
- **ナビ/モード（normal / conflict / welcome / error）は view-local enum**（OpenLogi 方針）。現状 render 冒頭の if 分岐を `enum Screen{ Welcome, Error(_), Repo(RepoView), Conflict(ConflictView) }` に型化し、巨大単一 render を**画面ごとの子 view（`Entity`）に分割**する。

### 3-4. state がどこに住むか

- **per-repo view-model を独立型に切り出す**（最重要）。現状 `TabViewState`（pure・Send）が既にあるので、これを **「描画される per-repo state の唯一の置き場」**に拡張し、選択/diff/conflict/commit-panel/modal もここへ寄せる。KagiApp は「welcome/error/tab strip/menu overlay + active RepoView への委譲」だけの **薄い shell** に縮める（~111 フィールドの god struct を解体）。
- **modal は 20 個の Option フィールドを `enum ActiveModal{ None, Pull(_), Push(_), CreateBranch(_), … }` 1 つに統合**（同時に 2 つ開かない不変条件を型で表現、`reset_per_repo_ui` の 20 行手動クリアが 1 行に）。
- **AppState の所有形**: OpenLogi は global を採るが、Kagi は **`Entity<AppState>`（または per-repo `Entity<RepoView>`）を推奨**。理由: Kagi は async op の結果を `weak.update(cx, ...)` で書き戻す既存パターンが大量にあり、Entity の方が gpui の `update`/`observe`/generation guard と素直に噛む。global は preferences / theme / i18n のような **プロセス唯一のもの**に限定（現状 theme/i18n は既に process-global atomic で OK）。
- oplog（`op_entries` ring + JSONL）は app 層所有、OperationController が追記。

### 3-5. headless ハーネスを main.rs から剥がす

- main.rs の ~30 個の `KAGI_*` ブロック（git2 直呼び 28 回）は **app 層の OperationController を呼ぶ薄いコマンドに置換**する。テストは「UI を bypass して app 層 API を叩く」形にし、main.rs は「引数解析 → AppState 構築 → run_app」だけにする。これにより main.rs からも git2 直呼びが消え、不変条件が main にも及ぶ。

### 3-6. 不変条件の機械的保証

- ui crate（module）から `git2` を **import 禁止**にする（`#![deny]` 相当の lint、or CI grep gate `grep -r 'git2::' src/ui/ → 0`）。現状 81+ 箇所あるので、移行は backend trait 経由へ段階置換。

---

## (4) 採用しない設計

- **Zed の workspace / Panel / Dock / PaneGroup / command_palette / ui crate のコード転写**: GPL-3.0+。設計パターンのみ参照（ADR-0006/0031/0034 既定）。Kagi の dock/tab/status bar は自前 + gpui-component で足りる。
- **マルチ window をデフォルト構成にする**: MVP は単一 window。`window.new` は `Disabled` 維持、必要時に `WindowRegistry` で追加。
- **OpenLogi の 2 プロセス agent + tarpc IPC**: Git GUI に過剰。単一プロセスで足りる。
- **gpui を zed git main + `gpui_platform` で追従**: crates.io **0.2.2 pin 継続**（ADR-0001、再現性）。OpenLogi コード参照時は API ズレ前提で Port。
- **AppState を全部 gpui global にする**: per-repo の重い view 状態を global に置くと async 書き戻し・複数 repo・generation guard と相性が悪い。global は process 唯一の preferences/theme/i18n に限定。
- **ルータ/フレームワーク的ナビ**: gpui にルータは無い。view-local enum で十分（OpenLogi 方針）。

---

## (5) リスク

- **大規模リファクタの一括破壊リスク**: KagiApp(~111 フィールド)・mod.rs(16775 行)を一度に分割すると回帰が読めない。→ **strangler 方式**（先に backend trait と OperationController を導入して新規 op をそこへ、既存は段階移行）を推奨。`TabViewState` の既存 pure/Send 分離が足場になる。
- **git2 排除の波及**: `Repository::open` 113 箇所・`git2::` 81+ 箇所。avatar/conflict/commit_panel/terminal など各所に散る。backend trait のメソッド網羅が不足すると UI が再び git2 を覗く誘惑が残る → trait 設計を先に固める必要。
- **gpui 0.2.2 / gpui-component rev 固有制約**: window 第一層 `Root` 必須、`InputState` の window-context lazy 生成、`is_action_available` ベースの menu disabled、`set_menus` 再構築（theme/lang）。これらは OpenLogi(zed-git gpui)からの Port で API がズレる。docs.rs/gpui/0.2.2 を一次資料に。
- **OperationController の cancellation 導入**: 現状 op に cancel/generation guard が無い。導入時に「途中で repo 状態が変わった op の結果を破棄する」判定（tab の `switch_generation` と同型）を全 op に通す必要があり、漏れると stale write。
- **headless テスト資産の移植コスト**: main.rs の `KAGI_*` 駆動テストが既存テスト suite の前提。app 層 API へ付け替える際にログ文字列（`[kagi] planned:/executed:/verified:`）の互換維持が要る（test fixture 期待値）。

---

## (6) 未解決事項

- AppState の最終所有形: **単一 `Entity<AppState>`（内部に Vec<RepoView>）** vs **`Entity<RepoView>` を tab ごと** のどちらが gpui の observe/update/focus と最も噛むか（プロトタイプで決める）。
- modal を `enum ActiveModal` に統合する際、各 modal が持つ `Entity<InputState>` / focus handle / 再 plan generation をどう enum variant に内包するか（window-context lazy 生成の制約と両立する形）。
- sub-window 化の線引き: About/Settings/Shortcuts を `AuxWindow` 化するか overlay 維持か（UX 決定。i18n/theme の live 反映が sub-window でも効くか要検証）。
- command palette(cmd-shift-p) の UI 実体: 自前 vs gpui-component の Picker(delegate トレイト D 必須)。registry 供給は確定だが UI 採否は別途。
- main.rs の headless ハーネスを app 層 API に移した後の **テスト互換ログ**の正確な維持範囲（どのログが fixture 期待値に縛られているかの棚卸しが未実施）。
- OperationController の API 粒度: 全 op を 1 つの `run_operation(OpKind)` enum dispatch にするか、op ごとの型付き method にするか（dangerous 二段階確認・preflight 差異の表現）。
- 既存 `MenuOverlay`(branch picker 等)と新 view 分割の責務境界（overlay は RepoView 所有か shell 所有か）。
