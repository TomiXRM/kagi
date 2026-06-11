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
| T016 | cherry-pick dry-run preview を実装する | T013 | in-progress |
| T017 | error handling と operation log を整える | T013–T016 | todo |

補助タスク(ticket 外、PM 管理):
- fixture repo 生成スクリプト(`scripts/make_fixture.sh`)— 用意済み(merge / ahead 1 / behind 1 / tag / stash / dirty WT を含む)
