# ADR-0084: Reflog-backed Undo/Redo + Cmd+Z / Cmd+Shift+Z

- Status: Accepted(2026-06-15、ユーザー依頼「Cmd+Z / Cmd+Shift+Z で Undo/Redo。今の Undo/Redo を改善。reflog ベースで、突然開いたリポジトリでも効くように。undo は reset --soft 相当で変更はステージされた状態で戻る」)
- Date: 2026-06-15
- Builds on: ADR-0081(operation undo/redo の plan→confirm→execute 基盤)、ADR-0023/0024(reset --hard 隔離)

## Context

ADR-0081 の Undo/Redo は **セッション内メモリの操作スタック**(`OperationHistory`)だけを見る。
よって:

- アプリ起動直後に開いたリポジトリでは undo できない(このセッションで操作していないため)。
- キーボードショートカットが無い(ツールバーボタンのみ)。
- undo の実体が **mixed reset**(index を親ツリーに戻す)なので、commit を undo すると
  変更が **unstaged** で戻る。ユーザーは `git reset --soft` 相当(**staged** のまま戻る)を期待。

git の本質: commit 時に起きるのは「新 commit object 生成 + branch ref が新 SHA を指す + reflog に
1行追記」だけ。undo = ref を親に戻すだけ(object は不変・消えない)。reflog が
「どこからどこへ ref が動いたか」を常に記録しているので、**誰がいつ作った commit でも
ref の移動として追える**。これを使えば「突然開いたリポジトリでも効く」undo が作れる。

## Decision

### 1. キーボードショートカット(text-input を壊さない)

- 新 gpui Action `HistoryUndo` / `HistoryRedo`(Edit メニューの `EditUndo`/`EditRedo`(OsAction)とは別物)。
- `cmd-z` → `HistoryUndo`、`cmd-shift-z` → `HistoryRedo` を **コンテキスト述語付き**で bind:
  `Some("!Input & !Terminal")`。
  - commit message 等の入力欄(gpui-component Input の key_context = `"Input"`)に focus がある時は
    bind が **発火せず**、OS 標準のテキスト undo(OsAction::Undo)がそのまま効く。
  - 統合ターミナル(key_context `"Terminal"`)でも発火させない。
  - それ以外(commit graph など)に focus がある時だけアプリ undo/redo が発火。
- ツールバーの Undo/Redo ボタンは同じ `open_history_undo_modal` / `open_history_redo_modal` を呼ぶ
  (挙動一致)。

### 2. reflog シードで「開いた直後でも効く」

- リポジトリ open / switch / reload 時、`OperationHistory` が空なら **現在ブランチの reflog から
  履歴をシード**する(`refs/heads/<branch>` の reflog。`git2::Repository::reflog`)。
  - reflog は新しい順。各エントリ `(old_oid, new_oid, message)` から `HistoryEntry { before: old,
    after: new, kind: infer(message), branch, summary }` を作る。
  - `kind` は message 接頭辞から推定: `commit`, `commit (amend)`→Amend, `commit (merge)`/`merge`→Merge,
    `revert`→Revert, `cherry-pick`→CherryPick, `pull`/`rebase`→(汎用), `reset`→(汎用 UndoCommit 扱い)。
  - **連鎖する分だけ**シード(`entry[i].before == entry[i+1].after` が続く範囲、最大 N=50)。連鎖が
    切れたら停止(別ブランチからの reflog ノイズや GC 境界を避ける)。
  - `cursor = len()`(全エントリ適用済み = 現在の HEAD が最新)。Cmd+Z で 1 つ戻り、Cmd+Shift+Z で進む。
- セッション中の操作は従来どおり `record_history` で push(正確な summary 付き)。reflog シードは
  「空のとき初回だけ」なので二重計上しない。ブランチ切替時は履歴をクリアして再シード。
- これにより **起動直後に開いたリポジトリでも直近の ref 移動を undo 可能**(reflog 由来)。

### 3. undo は soft(変更は staged で戻る)

- `execute_history_move` の index 取り扱いを **soft 相当**に変更:
  - **ref を移動するだけ**で index と working tree には触れない。
  - 結果: commit を undo すると HEAD が親に戻り、index は commit のツリーのまま →
    その差分が **staged** として復活(= `git reset --soft HEAD@{1}`)。working tree も不変。
  - redo は逆向きの ref 移動(after へ)。index がすでに after ツリーと一致していればクリーン。
- working tree の未コミット変更は常に保持(`--hard` は一切使わない、ADR-0023/0024 厳守)。
- stale 検出(ブランチが `from` から動いていたら blocker)、reachable 検出、conflicted blocker は
  ADR-0081 のまま維持。dirty WT は warning(保持される旨)。

### 4. 安全性(不変)

- plan→confirm(プレビュー modal)→execute→verify→oplog 記録 のパイプラインは維持。
- commit object は破壊しない(reflog/ODB に残る)。recovery テキストに `git reflog` / `git update-ref` を明示。
- `reset --hard` / `git clean` は使わない。

## Consequences

- 起動直後のリポジトリでも Cmd+Z で直近操作を undo できる(reflog シード)。
- commit undo の体験が GitKraken / 一般的な GUI と一致(変更が staged で戻る)。
- text-input / terminal の Cmd+Z は従来どおり(コンテキスト述語で分離)。
- 既存テスト `undo_preserves_working_tree_changes` 等は soft 化に合わせて期待値更新が必要
  (working tree 保持は不変、index は「commit ツリーのまま=staged」に変更)。
- 将来: oplog UI からの restore、merge-undo の多段対応、reflog kind 推定の精緻化。
