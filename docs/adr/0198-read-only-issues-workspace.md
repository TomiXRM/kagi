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
