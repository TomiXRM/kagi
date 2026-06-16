Rust + GPUI製のGitクライアントに「File History」機能を実装したい。

目的:
特定ファイルについて、過去にどのcommitで変更されたかを一覧表示し、選択したcommitにおけるそのファイル単体のdiffを確認できるようにする。これはリポジトリ全体の履歴表示ではなく、1ファイルに特化した履歴閲覧機能である。

重要方針:
File Historyはモーダルではなく、中央メイン領域に表示する専用ビューとして実装する。既存UIは左にブランチ/タグ/ワークツリー、中央にcommit graphやdiff、右にcommit detail、下にterminal/operation logがある構成なので、それを壊さず、中央領域をFileHistoryViewに切り替える設計にする。

導線:
以下の場所からFile Historyを開けるようにする。
1. modified/staged/untracked file listのファイル右クリックメニュー
2. commit detail内のchanged filesのファイル右クリックメニュー
3. diff viewのファイルパスヘッダー付近のHistoryボタンまたはメニュー

メニュー名は “Show File History” とする。

UI構成:
FileHistoryViewは以下の構成にする。

- FileHistoryHeader
- FileHistoryCommitList
- FileHistoryDiffViewer
- FileHistoryDetailPane

画面レイアウト:
左サイドバーは既存のBranch/Tag/Worktree/Stash一覧を維持する。
中央メイン領域をFile History専用ビューに切り替える。
右ペインは選択中commitの詳細表示に使う。
下部terminal/operation logは既存通り表示・非表示できるようにする。

中央メイン領域の構成:
上部にHeader、中央上にcommit list、中央下にdiff viewerを配置する。
commit listとdiff viewerの境界は縦方向にリサイズ可能にする。

Header:
以下を表示する。

- Backボタン
- File History: <relative file path>
- 現在のbranch名
- history件数
- Refreshボタン
- Copy Pathボタン
- Open Fileボタン
- Follow Renames: On/Off表示またはトグル

Backを押すと直前の画面に戻る。
長いファイルパスは省略表示し、hoverでfull pathを見られるようにする。
Copy Pathはリポジトリ相対パスをコピーする。
Open Fileは通常のfile/diff表示へ戻る。
Refreshは履歴を再取得する。

Commit List:
選択したファイルを変更したcommit一覧を表示する。
表示項目は以下。

- change type
- commit subject
- author
- relative date
- insertions/deletions
- short hash

表示例:
M  Fix parser error handling        Tomix  2h ago   +12 -4   a1b2c3d
R  Rename file_history.rs           Tomix  1d ago   +0 -0    d4e5f6a
A  Add file history view            Tomix  5d ago   +220 -0  9a8b7c6

change type:
- A: Added
- M: Modified
- D: Deleted
- R: Renamed
- C: Copied(optional)
- WIP: Uncommitted changes

対象ファイルに未コミット変更がある場合、commit listの最上部に synthetic entry として WIP を表示する。
表示例:
WIP — Uncommitted changes

WIPが存在する場合は初期選択をWIPにする。
WIPがない場合は最新commitを初期選択にする。

Commit rowの操作:
- click: そのcommitのdiffを下部diff viewerに表示
- double click: commit detail viewまたはgraph上のcommitへ移動してよい
- right click: context menuを表示

context menu:
- Copy Commit Hash
- Copy File Path at This Commit
- Open Commit
- Show Commit in Graph

v1では Restore this file や Checkout this version は実装しない。危険操作なので後回しにする。

Diff Viewer:
選択中entryに対して、そのファイル単体のdiffを表示する。
既存のdiff viewerを必ず再利用する。新しいdiff rendererを作らない。

commit選択時は、そのcommitで対象ファイルに入った変更のみを表示する。
WIP選択時はworking tree/staged changesのdiffを表示する。

Added fileの場合:
“This file was added in this commit.” を表示した上でdiffを表示する。

Deleted fileの場合:
“This file was deleted in this commit.” を表示した上で削除diffを表示する。

Renamed fileの場合:
old path → new path を表示し、rename commitのdiffを表示する。

Binary fileの場合:
クラッシュさせず “Binary file changed. Preview is not available.” を表示する。

Right Detail Pane:
右ペインには選択中commitの詳細を表示する。ただし情報を詰め込みすぎない。
表示項目は以下。

- full commit hash
- short hash
- commit subject
- commit message body
- author
- committer
- author date
- file change type
- insertions/deletions
- path before
- path after

Actions:
- Open Commit
- Show in Graph
- Copy Hash

v1では破壊的操作は置かない。

データ取得要件:
v1ではlibgit2だけで無理に実装しない。rename追跡が面倒でバグりやすいので、Git CLIを使ってよい。

履歴取得は git log --follow 相当の挙動にする。
最低限、renameされたファイルでも可能な範囲で過去履歴を追えること。

想定コマンド:
- git log --follow --format=... -- <path>
- git show --name-status --find-renames --format= -- <commit> -- <path>
- git show --format= --find-renames <commit> -- <path>
- git diff -- <path>
- git diff --cached -- <path>
- git status --porcelain=v1 -- <path>

注意:
pathには空白、日本語、記号が含まれる可能性があるので、必ず安全に引数として渡す。shell文字列結合で実行しない。

状態表示:
Loading:
“Loading file history...”

Empty:
“No history found for this file.”

Untracked:
“This file is untracked. No commit history yet.”

Error:
“Failed to load file history.”
エラー詳細とRetryボタンを表示する。

実装モデル案:
FileHistoryRequest
- repo_path
- file_path
- follow_renames
- include_wip

FileHistory
- repo_path
- current_path
- entries

FileHistoryEntry
- kind: Wip or Commit
- commit summary
- file change summary

CommitSummary
- full_hash
- short_hash
- subject
- body
- author_name
- author_email
- author_date
- committer
- committer_date

FileChangeSummary
- change_type
- path_before
- path_after
- insertions
- deletions
- is_binary

FileHistoryDiff
- title
- patch
- is_binary

実装品質:
Git操作はUI threadをブロックしない。
履歴取得中にUIが固まらないこと。
repository refresh時にfile historyも再取得できること。
外部変更検知時はstale表示または自動refreshを行うこと。
Git command failureを握り潰さずUIに表示すること。
既存のcommit graph、diff view、terminal layoutを壊さないこと。

非スコープ:
v1では以下を実装しない。

- line blame
- per-line history
- file history graph
- 複数ファイル同時履歴
- restore file from commit
- checkout this file at commit
- commit間比較UI
- semantic diff
- binary preview

Acceptance Criteria:
- file context menuからShow File Historyを開ける
- commit changed filesからShow File Historyを開ける
- diff headerからShow File Historyを開ける
- FileHistoryViewが中央メイン領域に表示される
- 選択ファイルを変更したcommit一覧が表示される
- commitを選択すると、そのcommitでの対象ファイルdiffが表示される
- WIP変更がある場合、最上部にWIPが表示される
- untracked fileでもクラッシュしない
- deleted fileでも履歴が表示される
- renamed fileでもgit log --follow相当で履歴を追える
- binary fileでもクラッシュせず専用メッセージを表示する
- loading/empty/error stateがある
- commit listとdiff viewerはリサイズ可能
- 既存diff viewerを再利用している
- Git操作でUI threadをブロックしない