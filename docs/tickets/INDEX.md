# Ticket バックログ(MVP)

運用ルール:
- ticket は 1 つずつ詳細化(`TNNN.md`)→ subagent 実装 → PM レビュー → 次へ。
- 各 ticket には必ず: 背景 / 完了条件 / 触ってよいファイル / 触ってはいけないファイル / テスト方法 / リスク を書く。
- Status: `todo` → `in-progress` → `review` → `done`

| ID | タイトル | 依存 | Status |
|----|----------|------|--------|
| T001 | Rust + GPUI の最小アプリを起動する | - | done |
| T002 | repo path を指定して git repository を開く(git2 導入, GitBackend trait) | T001 | done |
| T003 | working tree status を取得して表示する | T002 | done |
| T004 | commit log を取得して内部モデル(Commit)に変換する | T002 | done |
| T005 | branch / remote branch / tag / HEAD を取得する(RepoSnapshot 完成) | T004 | done |
| T006 | commit graph layout の pure Rust モジュールを実装する | T004 | done |
| T007 | graph layout の unit test を作る(直線/分岐/merge/octopus/複数root) | T006 | done |
| T008 | GPUI で commit list を表示する(仮想化リスト) | T001, T005 | done |
| T009 | GPUI で commit graph lane を描画する | T006–T008 | done |
| T010 | commit selection で metadata panel を表示する | T008 | done |
| T011 | changed files list を表示する | T010 | done |
| T012 | file diff viewer を表示する | T011 | done |
| T013 | checkout branch を plan 確認付きで実装する(OperationController 導入) | T005 | done |
| T014 | create branch を実装する | T013 | done |
| T015 | stash push / list / apply を実装する | T013 | done |
| T018 | changed files をファイルツリー表示にする(ユーザー要望) | T011, T012 | done |
| T016 | cherry-pick dry-run preview を実装する | T013 | done |
| T019 | テキストオーバーフロー修正(ユーザー報告バグ) | T010, T012 | done |
| T020 | グラフエッジの直角(角R)化 + コミッターアバター(ユーザー要望) | T008, T009 | done |
| T017 | error handling と operation log を整える | T013–T016 | done |
| T021 | commit 行レイアウトを GitKraken 順に変更(ユーザー要望) | T020 | done |
| T022 | 詳細ペインの縦スクロール + 折り返し廃止(ユーザー報告) | T019, T021 | done |
| T023 | ペインのリサイズ対応(ユーザー要望) | T021, T022 | done |
| T024 | staging バックエンド(stage/unstage/commit + workdir diff) | T011, T013 | done |
| T025 | Commit Panel UI(GitKraken 風の作業台) | T024, T018, T022 | done |
| T026 | commit message 入力の IME 対応(gpui-component Input) | T025 | done |
| T027 | Commit Panel の Unstaged/Staged を 1:1 独立スクロールボックスに | T025, T026 | done |
| T028 | sidebar branch: クリック=ジャンプ / ダブルクリック=checkout | T013, T021 | done |
| T030 | commit list の列(branch/graph/message)を個別リサイズ可能に | T021, T023 | done |
| T029 | 外部変更の自動追従(.git 監視リフレッシュ) | T005 | done |

補助タスク(ticket 外、PM 管理):
- fixture repo 生成スクリプト(`scripts/make_fixture.sh`)— 用意済み(merge / ahead 1 / behind 1 / tag / stash / dirty WT を含む)

## Shell 拡張バックログ(requirements-shell.md / ADR-0007〜0011)

実施順は「依存」列と上から順が基本。ADR は設計フェーズで作成済み(T-BP-006 / T-HT-008 は ADR で完了)。

| ID | タイトル | 依存 | Status |
|----|----------|------|--------|
| T-BP-001 | AppShell layout slot 化(Header/Main/RightPanel/BottomPanel/StatusBar)※挙動不変リファクタ | - | done |
| T-BP-002 | BottomPanel open/close + 高さリサイズ | T-BP-001 | done |
| T-BP-003 | StatusBar(情報表示 + タブ toggle ボタン + cmd-j) | T-BP-001 | done |
| T-BP-004 | Operation Log タブ(メモリリングバッファ + 表示) | T-BP-002 | done |
| T-BP-005 | Git 操作結果を Operation Log に流す + 失敗時自動オープン | T-BP-004 | done(T-BP-004 に統合) |
| T-BP-006 | Terminal 実装方式の調査 ADR | - | done(ADR-0008) |
| T-BP-007 | MVP Terminal(単一 session) | T-BP-002, ADR-0008 | done |
| T-BP-008 | Terminal 内 git 操作後の state refresh | T-BP-007 | done 相当(T029 watcher が充足。T-BP-007 で確認のみ) |
| T-HT-001 | Header Toolbar UI skeleton + branch/ahead-behind 表示(T-HT-002 統合) | T-BP-001 | done |
| T-HT-003 | Pull(fetch 含む)の plan + 実行 | T-HT-001, ADR-0009 | done |
| T-HT-002 | branch / upstream / ahead-behind 表示 | T-HT-001 | done 相当(T-HT-001 に統合) |
| T-HT-004 | Push の plan + 実行(set-upstream flow) | T-HT-003 | done |
| T-UI-001 | Toolbar/StatusBar ボタンにアイコン(ユーザー要望) | T-HT-001 | done |
| T-UI-002 | Stage all / Unstage all + List|Tree 切替(ユーザー要望) | T025 | done |
| T-UI-003 | diff を main pane に全幅表示(ユーザー要望) | T012, T025 | done |
| T-UI-004 | diff シンタックスハイライト(tree-sitter、外部バイナリなし) | T-UI-003 | done |
| T-HT-005 | Branch Create dialog 拡張(作成後 checkout 選択) | T-HT-001 | todo |
| T-HT-006 | Stash plan 拡張(対象ファイル表示 / untracked 選択) | T-HT-001 | done(T-HT-007 に統合) |
| T-HT-007 | Stash pop(conflict 予測 blocker、apply 提案) | T015 | done |
| T-HT-008 | Undo Commit ADR | - | done(ADR-0011) |
| T-HT-009 | Undo Commit(ref 付け替えのみの soft 相当) | ADR-0011 | done |
| T-HT-010 | Header 操作後の refresh 統合確認 | T-HT-003〜009 | done(各 confirm_* が reload + watcher が補完) |
| W2-HEADER | Header 再グルーピング(左集約 / Pull↓N Push↑N / Undo ラベル / Refresh 分離) | ADR-0013 | done |
| W2-INSPECTOR | Commit Inspector(Summary→Metadata→Actions→Files / copy SHA / Path⇄Tree) | ADR-0015 | done |
| W2-SIDEBAR | Repository Navigator(REMOTE/TAGS / 折りたたみ+件数 / filter / upstream 表示) | ADR-0014 | done |
| W2-STATUS | Status Bar 拡張(conflict/stash/upstream/Busy)+ Bottom default 18% | ADR-0017 | done |
| W2-GRAPH | HEAD/merge node 視覚区別 / 選択強調 / compact mode / label接続 | ADR-0016 | done |
| W2-DELETE | plan_delete_branch backend(merged のみ、plan 経由)+ sidebar ✕ 起動 | ADR-0014 | done |
| W3-NOTIFY | スナックバー通知 + pull/push 非同期化(ユーザー要望) | - | done |
| T-CM-001 | Commit row の right-click event 取得 | ADR-0020 | todo |
| T-CM-002 | 右クリック commit を selection state に反映 | T-CM-001 | todo |
| T-CM-003 | Context Menu component(overlay 描画 + dismiss) | T-CM-001, T-CM-004, ADR-0020 | todo |
| T-CM-004 | Menu item model + availability 純関数 | ADR-0021 | todo |
| T-CM-005 | disabled reason と tooltip 表示 | T-CM-003, T-CM-004 | todo |
| T-CM-010 | Copy full SHA(menu 統合) | T-CM-003, ADR-0022 | todo |
| T-CM-011 | Copy short SHA | T-CM-010 | todo |
| T-CM-012 | Copy commit message | T-CM-010 | todo |
| T-CM-013 | Show commit details(selection 統合) | T-CM-002, T-CM-003 | todo |
| T-CM-020 | Create branch here の plan 統合(checkout オプション込み) | T-CM-003, ADR-0022(既存 plan_create_branch) | done |
| T-CM-021 | Create branch dialog の menu 起点対応 | T-CM-020 | done |
| T-CM-022 | Create worktree here の ADR | - | done(ADR-0025) |
| T-CM-023 | Create worktree here の plan + 実行 | ADR-0025, T-CM-024 | done |
| T-CM-024 | Worktree path validation | ADR-0025 | done |
| T-CM-030 | Cherry-pick availability 判定 | T-CM-004 | todo |
| T-CM-031 | Cherry-pick の menu 統合(既存 plan 流用) | T-CM-030, ADR-0022 | todo |
| T-CM-032 | Cherry-pick conflict handling の確認・補強 | T-CM-031 | todo |
| T-CM-033 | Revert の ADR | - | done(ADR-0023 §Revert + ADR-0022 §Revert 実行設計) |
| T-CM-034 | Revert の plan + 実行 | ADR-0022, T-CM-004 | todo |
| T-CM-040 | Checkout this commit の availability 判定 | T-CM-004 | todo |
| T-CM-041 | Checkout commit(detached)の plan + 実行 + 警告 | T-CM-040, ADR-0022 | todo |
| T-CM-042 | Compare with HEAD | ADR-0026, T-CM-004 | todo |
| T-CM-043 | Compare with working tree / local changes | T-CM-042 | todo |
| T-CM-044 | Compare View の Right Panel / Diff Viewer 統合確認 | T-CM-042, T-CM-043 | todo |
| T-CM-050 | Reset Semantics ADR | - | done(ADR-0024) |
| T-CM-051 | Reset menu 項目(disabled)の追加 | T-CM-003, ADR-0024 | todo |
| T-CM-052 | Soft reset の plan + 実行 | ADR-0024 実装承認 | later |
| T-CM-053 | Mixed reset の plan + 実行 | T-CM-052 | later |
| T-CM-054 | Hard reset(設計完了まで実装しない) | T-CM-053, ADR-0024 §hard の追加設計 | later |
| T-CM-060 | Context Menu 操作の Operation Log 記録 | T-CM-020〜041 | todo |
| T-CM-061 | 操作後 refresh の統合確認 | T-CM-060 | todo |
| T-CM-062 | Inspector Actions と Context Menu の handler 統合 | T-CM-010, T-CM-021, T-CM-031 | todo |
| T-CM-063 | 状況別 availability の unit test | T-CM-004 | todo |
| T-CM-064 | fixture での E2E 検証(cherry-pick/revert/checkout/compare) | T-CM-031, T-CM-034, T-CM-041, T-CM-043 | todo |
| W4-TABS | リポジトリ tab 切り替え + ディレクトリ選択(ADR-0027/0028) | - | in-progress |
| W5-MENU | メニューバー + Command Registry(ADR-0029) | W4-TABS | in-progress |
| W6-TABSPEED | tab 切替高速化(キャッシュ + 非同期読込、ADR-0030) | W4-TABS | in-progress |
| W7-INSPECTOR2 | Inspector レイアウト再設計(message スクロール枠 + 1:1 リサイズ) | ADR-0015 | in-progress |
| W8-TERMSEL | ターミナル選択 + Cmd+C(vendored gpui-terminal、ADR-0035) | ADR-0035 | in-progress |
| W9-THEME | カラーテーマ6種 + メニュー切替(ADR-0036) | ADR-0029 | in-progress |
| W10-TOOLBAR | ツールバー Finder/Keynote 風(アイコン主体+下ラベル) | W9-THEME | queued |
| W11-AVATAR | GitHub アバター取得(ADR-0037) | W9-THEME | queued |
| W12-GCADOPT | gpui-component 採用第1弾(theme sync / Scrollbar / Checkbox) | 監査doc | queued |
| W13-BRANCHTREE | branch list の / 区切りツリー表示 + toggle | - | in-progress |

## Commit 便利機能スイート(requirements-commit-suite.md / ADR-0039〜0045)

設計フェーズ完了(requirements + ADR 0039〜0045)。lane 分割は requirements §実装 lane 分割案(W14-x)。
0040(amend の pushed 扱い)/ 0044(Smart Commit 既定 backend)は **Proposed** = ユーザー決定待ち。

| ID | タイトル | 依存 | Status |
|----|----------|------|--------|
| T-COMMIT-001 | Commit Preview — staged 概要(count/summary/branch/author) | T025〜, ADR-0039 | todo |
| T-COMMIT-002 | Commit Preview — staged diff preview | T-COMMIT-001, T012 | todo |
| T-COMMIT-003 | Checklist module(純関数)+ block/warn 統合 | ADR-0039/0043 | todo |
| T-COMMIT-004 | Checklist — conflict marker 検出(block) | T-COMMIT-003 | todo |
| T-COMMIT-005 | Checklist — secret/.env 検出(warn, override 可) | T-COMMIT-003 | todo |
| T-COMMIT-006 | Checklist — large binary 検出(warn, override 可) | T-COMMIT-003 | todo |
| T-COMMIT-007 | Draft Autosave — backend(branch ごと保存/復元/clear) | ADR-0042 | todo |
| T-COMMIT-008 | Draft Autosave — UI 配線(debounce/復元/clear) | T-COMMIT-007 | todo |
| T-COMMIT-009 | Message Template — type/scope/.../risk + plain⇄template | T025〜, T-COMMIT-007 | todo |
| T-COMMIT-010 | Amend — backend(plan/execute, 3 モード, SHA 変化) | ADR-0040(Proposed) | blocked |
| T-COMMIT-011 | Amend — UI(モード選択/SHA 表示/2段階確認) | T-COMMIT-010 | blocked |
| T-COMMIT-012 | Undo Last Commit — UI 配線(+ oplog 元 sha 表示) | T-HT-009, ADR-0041 | todo |
| T-COMMIT-013 | Undo Last Commit — soft 相当 backend(既存で充足) | ADR-0011/0041 | done 相当 |
| T-COMMIT-014 | Undo Last Commit — oplog before/after(既存で充足) | ADR-0011/0041 | done 相当 |
| T-COMMIT-015 | Smart Commit Message — backend(enum dispatch/ollama/fallback) | ADR-0044(Proposed) | blocked |
| T-COMMIT-016 | Smart Commit Message — UI(Generate/日英/静かな fallback) | T-COMMIT-015 | blocked |
| T-COMMIT-017 | Split Commit(file 単位)+ Commit to New Branch | 既存 stage/plan | todo |
| T-COMMIT-018 | Fixup/Squash commit 作成(prefix のみ, autosquash later) | ADR-0045 | todo |
| W15-ASYNCOPS | 同期 git 操作の background 化 + checkout dirty 予測修正(QA BUG-1/2) | qa-audit | in-progress |
