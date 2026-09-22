# ADR-0198: Issuesは既存GitHub境界を拡張した読み取り専用workspaceとして扱う

- Status: Accepted
- Date: 2026-09-17
- Related: ADR-0163（MCPのGPUI分離）、ADR-0183（session-owned read model）、ADR-0197（session-owned UI state）、ADR-0078（UIのGit境界）
- Scope: `crates/kagi-domain/src/github.rs`、`crates/kagi-git/src/github*.rs`、`src/ui/github.rs`、`src/ui/tab_view.rs`、Issues workspace pane

## Context

KagiのPR表示は、`gh`を認証・取得境界として使い、純粋な `PullRequest` モデルをGit/UIから分離している。GitHub Issuesの読み取りUIには、一覧、選択したIssueの本文・ラベル・担当者・コメントを表示する要件がある。一方、現在のモデルと取得APIにはIssue型、ページング情報、詳細・コメント取得、session-owned表示状態がない。

外部参照e1にはHTTP、OAuth、ETagキャッシュ、共通Issue/PR型がある。しかし、それらを移植すると、Kagiに既にある `gh`認証とsession所有の非同期経路を並行実装することになる。

## Decision

1. Issuesは**読み取り専用**で実装する。作成、コメント、close、ラベル変更、担当者変更、branch/worktree作成は今回追加しない。
2. `kagi-domain::github` にGitHub Issue表示用の純粋モデルを追加する。外部の本文、タイトル、author、label、コメントは表示時に既存 `safe_text` 境界を通す。
3. `kagi-git::github` は既存 `gh_command` を使い、`gh issue list` と `gh issue view` のJSONを変換する。repo-local Git環境変数の除去、timeout、既存の失敗分類を維持する。UIは直接 `gh`／ネットワークを呼ばない。
4. `src/ui/github.rs` がbackground fetchを起動し、開始した `SessionId` にのみ結果を返す。成功した空一覧と失敗を区別し、失敗では前回成功データを消さない。
5. 一覧、選択、読み込み・失敗状態、詳細キャッシュ、request generationは `TabUiState` に置く。active tabへの書き戻しやpath文字列による所有判定は禁止する。
6. workspace modeとサイドバーナビは、既存のcanonical dispatcherを拡張する。Issues固有の並行モードboolを作らない。

## Consequences

- 最初のスライスはGitHub Issueの閲覧に限定され、HTMLモックアップ内の投稿・編集用コントロールは表示しない。
- GitHub APIのHTTPクライアント、OAuth、永続ETagキャッシュ、Issue/PR共通の大規模型は導入しない。
- 失敗、認証不可、空一覧、owner離脱、同一sessionの後発要求、詳細取得中に別Issueを選ぶ場合をテストする。
- 書き込み機能を追加する将来の変更は、`plan → confirm → preflight → execute → verify → oplog` に従う別ADRを必要とする。

## #752: 一覧の cursor ページング

- 一覧取得は既存の `gh api graphql` query と `mentions:@me` alias を維持し、`issues(first:100, after:$cursor, states:OPEN)` の `pageInfo` を `IssueListSnapshot::next_cursor` に変換する。不正・進まない cursor は取得失敗であり、終端と推測しない。
- `TabUiState` が cursor、追加取得中フラグ、`ListState` を所有する。`build_tab_view` の read model へ複製せず、session attach 時の `TabUiState::default` で初期化する。
- Composer と main 一覧は同じ仮想スクロール内に置く。最終行の visible range または clip 内の末尾表示を検知し、session・repository・generation を固定して次ページを取得する。append は既存行の位置を維持し、重複する Issue 番号を増やさない。
- 追加取得失敗では rows と cursor を維持し、既存 error 表示と末尾の再試行操作へ戻す。自動再試行はしない。全4フィルターの件数は読み込み済み件数で、cursor があり件数が正の場合だけ `(N+)` と表示する。
- 非仮想化の sidebar は各フィルターのソート後の先頭100件だけを行要素にする。見出しの件数は制限前の全読み込み件数を維持し、100件以降は仮想化された main 一覧で閲覧する。
- 戻り・手動更新は先頭ページを再取得し、cursor と scroll をリセットする。先頭ページ更新で generation を進めて古い追加取得を無効化し、detach 済み session の完了を別タブに適用しない。
- 未計測行には既存の最小行高を仮高さとして渡す。高さゼロの行が wheel / scrollbar の移動範囲を計測済み領域に制限することを防ぎ、描画時は実際の可変行高で置き換える。成功した一覧・追加ページは `klog!("github: issues page={} loaded={} has_more={}")` を出す。
