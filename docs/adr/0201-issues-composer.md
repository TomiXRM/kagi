# ADR-0201: Issues Composer と記録付き投稿

- Status: Accepted
- Scope: T-ISSUES-COMPOSER P1。ADR-0198 の読み取り専用制限を拡張する。
- Related: ADR-0042 / ADR-0177 / ADR-0197 / ADR-0200 §10

## 決定

Issues の中央は常設 Composer を先頭に置く。本文だけで作成でき、タイトル省略時は
本文の最初の非空行から60 Unicode文字を取る。元の本文は変更しない。
InputState の Markdown source editor と既存 Markdown renderer を使用し、
Write / Preview、行数による拡張、Focus Editor を提供する。WYSIWYG は追加しない。
Focus Editor の secondary-shift-enter は IssueComposer context に限定する。
既定のキー登録に同じ chord はなく、modal Enter は無修飾のみであり衝突しない。
Paste action を本文 input の親で捕捉し、複数行テキストを fenced block として
selection に置換する。既存 input の undo/redo を維持し clipboard は書き換えない。

## 所有と保存

TabUiState の IssuesComposerState が編集状態・入力 Entity を所有する。
Entity の生成と復元は Window のある描画経路、I/O は background で行う。
新規 Issue と各 Issue の Reply は異なる draft。保存先は既存 `drafts.rs` の
`KAGI_LOG_DIR/drafts`（未指定なら `~/.kagi/drafts`）を再利用する。
settings.json には保存しない。キーは worktree path と `:issue:new` / `:issue:N`。
Git branch に使えない colon を含むので commit draft と衝突しない。
runtime SessionId をファイル名に含めず、再起動後も同じ worktree から復元する。
同一 worktree を開く操作は既存 Sessions が同一 session に統合する。

外側は ADR-0042 の Draft record、message 内は title/body の JSON tuple とする。
共通 save_draft を同一ディレクトリの tempfile + atomic replace に改善する。
commit と Issue で保存実装は共有し、既存 commit draft 契約を変更しない。
編集時に最新値をメモリへ queue、250ms 後に background flush、正常終了時も flush。
flush と clear の順序を共通 mutex で直列化し、失敗は pending に保持する。
保存先は queue 時点で固定する。

保存キーごとに process-lifetime の単調な version を保持する。投稿は version を
固定し、成功時に一致した版だけ clear を queue する。元タブが閉じられていても
消費できるが、再表示後の新しい編集は消さない。同じ保存版を復元していた input
だけを同期する。失敗と Unknown では draft は残る。restore の遅い完了にも版を照合する。

## 書き込み

github_comment.rs と同じ bounded runner と recording::finalize を使用する。
`gh issue create/comment` は固定した base_repo を `-R` に渡し、本文は stdin のみ。
repository identity は Composer 初期化の read で解決し、mutation 時は再解決しない。
Create/Reply の明示クリックを承認とし、空本文は plan blocker。
既存 finish_run の admission / in-flight lease / owner stamp に参加する。
transport 終了0は Success（URL を receipt に保存）、確定した非0は Failed、
signal・不完全I/O・停止不確定は Unknown。結果不明を再送しない。
記録失敗でも成功済み投稿を再実行しない。

成功の draft 消費と再読込は presentation guard より前で処理する。
宛先は dispatch 時の `(session, repository)` であり、active tab から導出しない。
Reply の再読込はユーザーが後で選んだ別 Issue を選択し直さない。
Thread は既存 issue_detail を使い、本文の後にコメントを時系列表示する。

## 検証と今回の範囲外

domain の既定タイトル・貼付・編集版、既存 drafts_test と版競合、fake gh による
transport receipt と stdin を検証する。限定 workspace_mode_toolbar で入力イベント、
Focus / Preview / paste と Undo を検証する。GUI 実行は sandbox 外の PM が行う。

AI タイトルは P1 本体の差分を抑えるため PM 承認条件により P2 へ延期する。
画像添付、Add context、AI refinement、Sub-issues、Agent 起動、Feed カード化も対象外。
