# W5-MENU: メニューバー + Command Registry(ユーザー要望)

- Status: in-progress
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

- [ ] メニューバーに kagi/File/Edit/View/Repository/Branch/Commit/Window/Help が出る(PM 実機確認)
- [ ] 全項目が registry 参照(menu 専用処理ゼロ。grep で確認可能な構造)
- [ ] ショートカットがメニューに表記され、実際に動く(cmd-o/cmd-t/cmd-w/cmd-r/ctrl-cmd-f)
- [ ] repo 未オープン / commit 未選択 / busy 中の disabled が機能(KAGI_MENU_DUMP で検証)
- [ ] 既存動作(cmd-j、escape、矢印、テキスト入力のcmd-c/v等)に回帰なし
- [ ] `cargo test` 全パス + own-code warning 0、既存 headless 回帰なし
- [ ] 追加 Command 一覧と placeholder 項目の報告(完了報告に含める)
- [ ] 実装メモを本ファイル末尾に追記

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
