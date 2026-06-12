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
| T-UI-004 | diff シンタックスハイライト(gpui-component tree-sitter、外部バイナリなし) | T-UI-003 | todo |
| T-HT-005 | Branch Create dialog 拡張(作成後 checkout 選択) | T-HT-001 | todo |
| T-HT-006 | Stash plan 拡張(対象ファイル表示 / untracked 選択) | T-HT-001 | done(T-HT-007 に統合) |
| T-HT-007 | Stash pop(conflict 予測 blocker、apply 提案) | T015 | done |
| T-HT-008 | Undo Commit ADR | - | done(ADR-0011) |
| T-HT-009 | Undo Commit(ref 付け替えのみの soft 相当) | ADR-0011 | done |
| T-HT-010 | Header 操作後の refresh 統合確認 | T-HT-003〜009 | done(各 confirm_* が reload + watcher が補完) |
