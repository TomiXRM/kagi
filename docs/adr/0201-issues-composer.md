# ADR-0201: Issues Composer と記録付き投稿

- Status: Accepted
- Scope: T-ISSUES-COMPOSER P1。ADR-0198 の読み取り専用制限を拡張する。
- Related: ADR-0042 / ADR-0177 / ADR-0197 / ADR-0200 §10

## 決定

Issues の中央は常設 Composer を先頭に置く。本文だけで作成でき、タイトル省略時は
本文から Markdown の heading / list / quote 接頭辞と続く空白を除き、fence marker 行を
飛ばした最初の有意味な行から60 Unicode文字を取る。元の本文は変更しない。
構造 marker だけで候補が残らない場合は固定 placeholder を補わず、明示タイトルを
必須とする plan blocker にする。
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

## 追記 1: Issue navigator と Composer home

sidebar は filter 付き navigation、未選択の main pane は常設 Composer と open Issue
全件一覧(最大100件、`updated_at` 降順)を表示する。main 一覧は sidebar の選択 filter に
連動せず、同じ純粋 filter の `RecentlyUpdated` 投影を再利用する。選択時は main を
Composer + Thread + Reply に切り替える。Thread には selection だけを解除して home の
全件一覧へ戻る明示 control を置く。Reply draft と detail cache は保持し、戻る操作は
detail generation を進めて遅い completion が Thread を復活させない。

sidebar の `Assigned to me` / `Created by me` / `Mentioning me` /
`Recently updated` は `TabUiState` が選択を所有し、同じ open Issue snapshot を純粋に
filter する。PR navigator と section/row chrome を共用する。mutation 用 repository
identity は既存 `gh repo view` の default-repository semantics で解決する(origin から
推測しない)。初回成功後は session に凍結した identity を refresh でも再利用する。
その identity を使う一つの GraphQL request に Issue、comment count、
`mentions:@me` の番号集合を含め、全結果を一つの owner/generation で原子的に受理する。
失敗時は全て最後の成功値を保持し、render や tab 切替から追加 fetch しない。viewer
login は既存 `KagiApp::github_login` cache を使う。

## 追記 2: title への複数行 paste (#751)

本文 input の Paste は上の決定のまま、複数行を fenced block として selection に
置換する。title input は単一行なので、同じ clipboard がそこへ落ちると全文が
title 1 行に潰れ body が空のままになっていた。title への複数行 paste は fence
せず**分割**する: 1 行目を title、最初の `\n` より後ろを body とする。

- CRLF を LF に正規化してから最初の `\n` で分割する。それ以外は byte 単位で
  保持する(trim しない、空行を落とさない、改行を足さない)。全文を fence で
  包まないのは、包むと Preview が全部 code になり #751 で確認したい構文が
  描画されないため。
- 複数行の判定は本文 paste と同じ `lines().count() >= 2`。1 行の clipboard は
  従来どおり Input 自身の paste が処理し、selection も undo も変わらない。
- 挿入は両 input の現在の selection に対して行い、既存の title / body を
  置き換えない。body は caret が行頭でないときだけ先頭に改行を足す(本文 paste
  と同じ規則)。挿入後は body を reveal して focus する。clipboard は書き換えない。
- 分割規則は `kagi_domain::issue_composer::title_paste_split` に置き、
  `fenced_code_paste` と同じ pure な domain 境界に留める。UI 側は title の
  既存 wrapper div(Enter guard と同じ div)で Paste を capture する。
