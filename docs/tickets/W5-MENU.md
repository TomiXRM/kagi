# W5-MENU: メニューバー + Command Registry(ユーザー要望)

- Status: done(PM 実機メニュー確認待ち)
- 担当: worktree agent(Opus)
- 関連 ADR: 0029(registry・menubar)/ 0022(dispatch_commit_action)/ 0024(Reset disabled)

## 背景

主要操作をメニューバーから発見・実行できるようにする。ただしメニュー専用処理は書かず、
Command Registry(`src/ui/commands.rs`)を唯一の正準にして、メニュー/ショートカット/
(既存の)コンテキストメニューが同じ handler を参照する。

## スコープ

1. **Registry**(ADR-0029): `Command {id,label,keystroke,dangerous}` + `command_state(app,id)`
   + `actions!` 1:1 対応 + `build_menus()` + KeyBinding 一括登録 + 条件付き `on_action` 登録
   (disabled = ハンドラ未登録で macOS が自動灰色化 — gpui の mac 実装で動作を確認すること)
2. **メニュー構成と実装レベル**(placeholder = `Disabled(理由)`):

   | Menu | 項目 | 実装 |
   |------|------|------|
   | kagi(app) | About(modal)/ Quit `cmd-q` | 実装 |
   | File | New Tab `cmd-t`(= picker で repo を新 tab に)/ Close Tab `cmd-w`(tabs 空なら disabled)/ Clone Repository… `cmd-shift-o`(**placeholder**)/ Open Repository… `cmd-o`(既存 pick_repository)/ Open Repository in Terminal(bottom terminal を開く。repo なしで disabled)/ Refresh Repository `cmd-r` | 実装(Clone のみ placeholder) |
   | Edit | Undo/Redo/Cut/Copy/Paste/Select All | `MenuItem::os_action` のみ(グローバル KeyBinding 禁止 — input の標準動作を壊さない) |
   | View | Zoom In/Out/Reset(**placeholder**)/ Enter Full Screen `ctrl-cmd-f`(window.toggle_fullscreen)/ Toggle Sidebar / Toggle Terminal(既存 cmd-j と同一 action)/ Toggle Commit Details / Toggle Diff View(main_diff open 時のみ enabled) | 実装(Zoom のみ placeholder) |
   | Repository | Fetch(新規: CLI fetch → reload → toast。`src/git/ops.rs` に `fetch_remote` 追加可)/ Pull / Push(既存 modal)/ Open in Finder(`open <path>`)/ Open in Terminal(File と同一 command 参照) | 実装 |
   | Branch | New Branch…(既存 dialog)/ Checkout Branch…(branch picker dialog → 既存 plan_checkout)/ Rename Branch…(**placeholder**)/ Delete Branch…(picker → 既存 delete plan modal) | 実装(Rename のみ placeholder) |
   | Commit | Copy Commit Hash / Checkout Commit / Create Branch from Commit… / Cherry-pick / Revert / Reset HEAD to Commit…(ADR-0024 で disabled)/ Compare with Working Tree — **全て選択 commit に対する `dispatch_commit_action` 呼び出し**。未選択時 disabled | 実装(action 側が stub のものはその挙動のまま) |
   | Window | Minimize / Zoom / New Window / Close Window | gpui platform API があるものだけ実装、なければ placeholder |
   | Help | Keyboard Shortcuts(registry から自動生成した一覧 modal)/ Documentation(`cx.open_url` → https://github.com/TomiXRM/kagi)/ Report Issue(…/issues)/ About | 実装 |

3. **disabled 条件**(command_state に集約): repo 未オープン(welcome)時は
   Refresh/Fetch/Pull/Push/Open in Finder・Terminal/Branch 系/Commit 系/Close Tab を disabled。
   commit 未選択時は Commit 系を disabled。busy_op 中は git 操作系を disabled(既存 W3 と整合)
4. **Toggle Sidebar / Commit Details**: 新規の表示フラグ(`sidebar_visible` / `inspector_visible`、
   default true)。render_body で分岐。divider との整合に注意
5. headless: `KAGI_MENU_DUMP=1` で全 command の id/label/keystroke/state をログ出力
   (`[kagi] menu: <id> ... state=enabled|disabled(<reason>)`)— メニュー UI は headless 検証
   できないため、これを正準の検証手段とする

## 完了条件

- [~] メニューバーに kagi/File/Edit/View/Repository/Branch/Commit/Window/Help が出る(PM 実機確認待ち)
- [x] 全項目が registry 参照(menu 専用処理ゼロ。build_menus は COMMANDS / command_state 一本)
- [~] ショートカットがメニューに表記され、実際に動く(cmd-o/cmd-t/cmd-w/cmd-r/ctrl-cmd-f)— bind 済み、表記は PM 実機確認待ち
- [x] repo 未オープン / commit 未選択 / busy 中の disabled が機能(KAGI_MENU_DUMP 3 パターンで検証)
- [x] 既存動作(cmd-j、escape、矢印、テキスト入力のcmd-c/v等)に回帰なし(新規 bind は cmd-j 非再定義、Edit は os_action のみ)
- [x] `cargo test` 全パス + own-code warning 0、既存 headless 回帰なし
- [x] 追加 Command 一覧と placeholder 項目の報告(実装メモに記載)
- [x] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/ui/commands.rs`(新規、できる限りここに集約)
- `src/ui/mod.rs` / `src/ui/tabs.rs` / `src/main.rs`(最小限)
- `src/git/ops.rs` / `src/git/mod.rs`(fetch_remote のみ)
- `docs/tickets/W5-MENU.md`

## 触ってはいけないファイル

- `tests/*`(fetch のテスト追加のみ可)/ `scripts/*` / `Cargo.toml` / 他 docs

## テスト方法

1. `cargo test`(exit code 直接確認)
2. fixture で KAGI_MENU_DUMP + 既存 headless 全回帰
3. メニュー実機操作は PM が確認(スクリーンショット)

## リスク

- KeyBinding 衝突: 既存 cmd-j / escape / up / down と新規 cmd-t/w/o/r/ctrl-cmd-f。
  Edit 系(cmd-z/x/c/v/a)は**バインドしない**(os_action のみ)
- メニューの disabled が「ハンドラ未登録」で本当に灰色になるかは gpui の
  `platform/mac/` 実装を確認してから設計どおりに。ならない場合は set_menus 再構築方式に
  切替え、その判断を実装メモに記録
- force push / reset --hard / git clean のコード追加禁止。Fetch は fetch のみ(merge しない)
- 文字列切り詰めは chars() ベース

## 実装メモ(W5-MENU 完了)

### disabled 方式の最終判断: ハンドラ未登録方式で確定(set_menus 再構築は不要)
gpui-0.2.2 の mac 実装を確認:
- `platform/app_menu.rs::init_app_menus` が `on_validate_app_menu_command` を
  `cx.is_action_available(action)` に配線(App 生成時に自動で呼ばれる)。
- `platform/mac/platform.rs::validate_menu_item` が NSMenu の validateMenuItem: で
  その callback を呼び、戻り値で項目の enabled/disabled を決める。
- `window.is_action_available` → `dispatch_tree.is_action_available` は dispatch tree に
  その action の `on_action` ハンドラが登録されているかを見るだけ。

したがって **「command_state==Enabled のときだけ root 要素に on_action を登録」** すれば
macOS が自動で灰色化する。dispatch tree は毎フレーム再構築されるので、状態が変われば
次の `cx.notify()` 描画で menu validation も追従する。`set_menus` の再呼び出しは一切不要。
→ ADR-0029 の設計どおりに機能。フォールバック(set_menus 再構築)は採用せず。

配線箇所: `src/ui/mod.rs::KagiApp::register_menu_actions`(render から `.map()` で適用)。
menu validation は focus が root にある前提だが、menu 起動の dispatch_action は
focused window 全体の dispatch tree を歩くので input にフォーカスがあっても動く。

### 追加 Command 一覧(id / keystroke / 実装 or placeholder)
| id | keystroke | 状態 |
|----|-----------|------|
| app.about | - | 実装(Info overlay) |
| app.quit | cmd-q | 実装(cx.quit) |
| file.newTab | cmd-t | 実装(pick_repository) |
| file.closeTab | cmd-w | 実装(close_tab、tabs 空で disabled) |
| file.cloneRepository | cmd-shift-o | **placeholder**(常時 disabled) |
| file.openRepository | cmd-o | 実装(pick_repository) |
| file.openInTerminal | - | 実装(bottom Terminal、repo なしで disabled) |
| file.refresh | cmd-r | 実装(reload) |
| view.zoomIn / zoomOut / zoomReset | - | **placeholder**(常時 disabled) |
| view.fullScreen | ctrl-cmd-f | 実装(window.toggle_fullscreen) |
| view.toggleSidebar | - | 実装(sidebar_visible フラグ) |
| view.toggleTerminal | cmd-j | 実装(既存 ToggleBottomPanel を共有、新規バインドなし) |
| view.toggleCommitDetails | - | 実装(inspector_visible フラグ) |
| view.toggleDiffView | - | 実装(main_diff open 時のみ enabled → close_main_diff) |
| repo.fetch | - | 実装(新規 fetch_remote、CLI fetch のみ・merge なし) |
| repo.pull | - | 実装(既存 open_pull_modal) |
| repo.push | - | 実装(既存 open_push_modal) |
| repo.openInFinder | - | 実装(open <path>、shell 非経由) |
| branch.new | - | 実装(既存 open_create_branch_modal) |
| branch.checkout | - | 実装(branch picker overlay → 既存 open_plan_modal) |
| branch.rename | - | **placeholder**(常時 disabled) |
| branch.delete | - | 実装(branch picker overlay → 既存 open_delete_branch_modal、dangerous) |
| commit.copyHash | - | 実装(dispatch_commit_action CopySha) |
| commit.checkout | - | 実装(dispatch_commit_action CheckoutCommit、現状 stub 挙動) |
| commit.createBranch | - | 実装(dispatch_commit_action CreateBranchHere、現状 stub 挙動) |
| commit.cherryPick | - | 実装(dispatch_commit_action CherryPick、現状 stub 挙動) |
| commit.revert | - | 実装(dispatch_commit_action Revert、現状 stub 挙動) |
| commit.reset | - | **placeholder**(ADR-0024 で常時 disabled、dangerous) |
| commit.compareWorkingTree | - | 実装(dispatch_commit_action CompareWithWorkingTree、現状 stub 挙動) |
| window.minimize | cmd-m | 実装(window.minimize_window) |
| window.zoom | - | 実装(window.zoom_window) |
| window.new | - | **placeholder**(常時 disabled、複数 window 未対応) |
| window.close | - | 実装(window.remove_window) |
| help.shortcuts | - | 実装(registry から自動生成した Info overlay) |
| help.documentation | - | 実装(cx.open_url GitHub) |
| help.reportIssue | - | 実装(cx.open_url issues) |

Edit メニュー(Undo/Redo/Cut/Copy/Paste/Select All)は `MenuItem::os_action` のみ。
グローバル KeyBinding は **張っていない**(input の cmd-z/x/c/v/a を壊さないため)。

placeholder 一覧: file.cloneRepository / view.zoomIn,zoomOut,zoomReset /
branch.rename / commit.reset(ADR-0024) / window.new。

### KeyBinding(新規)
cmd-q / cmd-t / cmd-w / cmd-shift-o / cmd-o / cmd-r / ctrl-cmd-f / cmd-m。
cmd-j は **再バインドせず** 既存 ToggleBottomPanel を View → Toggle Terminal に流用。
既存 escape / up / down とも衝突なし。

### 触ったファイル全列挙
- `src/ui/commands.rs`(新規・本 lane のロジック集約)
- `src/ui/mod.rs`:
  1. `pub mod commands;` 追加
  2. KagiApp に `sidebar_visible` / `inspector_visible` / `menu_overlay` フィールド追加
  3. `from_snapshot` / `with_error` 両コンストラクタにデフォルト追加
  4. `push_toast` を `pub(crate)` 化(commands.rs から呼ぶため。シグネチャ不変)
  5. `KagiApp::register_menu_actions` 追加(条件付き on_action 登録)
  6. render: `.map(register_menu_actions)` / `.children(render_menu_overlay)` 追加
  7. `render_body`: sidebar を `sidebar_visible` で、inspector を `inspector_visible` で分岐
  8. `run_app`: `commands::register_keybindings(cx)` + `cx.set_menus(commands::build_menus())`
- `src/main.rs`: KAGI_MENU_DUMP=1 で `ui::commands::dump_menu_states` 呼び出し(welcome / repo 両経路)
- `src/git/ops.rs`: `fetch_remote` + `FetchOutcome` + `resolve_fetch_remote` 追加(fetch のみ・merge なし)
- `src/git/mod.rs`: `fetch_remote` / `FetchOutcome` を re-export

### テスト・headless 結果
- `cargo test`: 全パス(統合 + unit、19/39/37/… すべて 0 failed)。own-code warning 0。
- headless `KAGI_MENU_DUMP=1` 3 パターン:
  - repo あり(選択なし): commit 系 disabled(commit が選択されていません)、repo/branch 系 enabled。
  - 引数なし(welcome): repo/branch/commit/closeTab 系すべて disabled、newTab/openRepository/about/quit/window/help は enabled。
  - repo + KAGI_SELECT_FIRST=1: commit 系 enabled(reset のみ ADR-0024 で disabled)。
  - placeholder(clone/zoom/rename/new window)は全パターンで disabled。
- 既存 headless 回帰なし(repo/path/HEAD/sidebar/statusbar/toolbar/tabs/watcher、pull plan、commit-panel ログ全て従来どおり)。
- fetch は fixture の origin(ローカル bare)に対し `git fetch origin` 成功を確認(merge しない)。

### 未解決リスク
- メニューの実機表示・灰色化・ショートカット表記は PM のスクリーンショット確認待ち
  (headless ではネイティブ NSMenu を検証できないため。ロジックは KAGI_MENU_DUMP で検証済み)。
- commit.checkout/createBranch/cherryPick/revert/compareWorkingTree は
  dispatch_commit_action 側が現状 stub(footer に「未実装: 次 lane」)。menu からの配線自体は完了済みで、
  本体実装は別 lane の責務。
