# T-ISSUES-COMPOSER — Issues を「思いつきを実装可能な仕事へ変換する」画面にする

- コンセプト全文: `docs/tickets/T-ISSUES-COMPOSER-concept.md`(ユーザー + ChatGPT の相談メモ、原文保持)。
- PM: w5:p0(omp)。設計相談 + 実装: w5:p19(codex astra)。サブエージェントの使用を推奨。
- ブランチ: `feat/issues-composer`(`feat/pr-lazy-fetch` から派生、GitHub Stacked PR。
  PR #744 の後に積む)。作業ツリー: `git worktree add ../git-client-issues feat/issues-composer`
  (worktree ローカルの `target/`、`CARGO_TARGET_DIR` を共有しない — AGENTS.md)。
- 現状: `src/ui/issues_mode.rs` は **読み取り専用**(一覧 + 詳細)。`kagi_git::github::list_issues`
  / `issue_detail` は `gh issue list/view --json`。書き込みはまだ無い。

## 決定(PM、実装で変えない)

### スコープ = コンセプトの MVP 10 項目のうち **P1** を今回、P2 以降は別票

P1(今回):

1. **常時表示の Composer** が Issues 画面の主役(一覧の上、GitHub 式の New issue 遷移なし)。
   Quick Capture(1 行目は本文、タイトル必須ではない)→ 数行を超える / コードを貼ると
   自然に Full Editor に拡張(`⌘⇧Enter` で Focus Editor)。
2. **Markdown source editor + Preview**(Write | Preview)。WYSIWYG / ブロックエディタは
   **作らない**。既存の `InputState::code_editor` / `kagi_ui_editor` の markdown 描画を再利用。
3. **コード貼り付け**: クリップボードの複数行テキストが fenced code block になる
   (言語は拡張子/ヒューリスティックで推定、失敗時は素の ```)。
4. **Draft 自動保存**(リポジトリ × セッション単位、`settings/store.rs` の規則に従い
   `settings.json` には**書かない** — 別ファイル `~/.kagi/drafts/<repo-id>.md` 等、
   `KAGI_LOG_DIR` 隔離に従う)。
5. **タイトル**: 空なら本文 1 行目(60 字で切る)を既定に。AI 生成タイトル(✨)は
   既存 Smart Commit の provider(Ollama / Claude / Codex CLI)が使える場合だけ表示、
   **原文の本文は絶対に書き換えない**(タイトル欄のみ)。
6. **Issue 作成** = 書き込み: `gh issue create -R <base_repo> --title --body-file -`。
   PR コメントと同じ規律(`crates/kagi-git/src/github_comment.rs` を写す): 本文は stdin、
   transport 境界で oplog 記録(op `issue-create`)、exit 0 → Success(URL を detail に)、
   非 0 → Failed、不確定 → Unknown + hold。`finish_run` 経由、明示クリックが承認、
   空本文は plan blocker。成功で Composer を空にし一覧を再読込(owner/repo 固定)。
7. **Issue Thread 表示**: 詳細を「フォーム」ではなく Thread(本文 → コメント時系列)に。
   既存 `issue_detail` の comments を使う。Reply(コメント投稿 `gh issue comment`)は
   同じ写しで op `issue-comment`。
8. **repo 選択**: 開いているタブのリポジトリ固定(タブ = repo なので選択 UI は不要)。
   Composer のヘッダに `owner/repo` を表示するだけ。

P2(別票、今回やらない): 画像 Drag&Drop(添付アップロードの経路が gh に無い)、
`+ Add context`(branch/diff/selection/commit/PR を本文に差し込む — Git クライアントの
強み、次)、AI refinement(整理 / 再現手順 / AC / 分解)、Sub-issues、**Issue → Agent →
Worktree → PR** の一筆書き、Feed のカード化。

### 不変条件(既存)

- `src/ui/` に `git2` / `gh` 直呼びなし(`kagi_git` 経由)。書き込みは
  `plan → confirm → preflight → execute → verify → oplog`。
- modal は `ActiveModal` 1 スロット。入力(`InputState`)は window を持つ描画パスで生成。
- i18n は EN/JA 両方。`[kagi]` 契約行は `klog!`。LOC ceiling 800(超えたら feature 境界で分割)。
- 完了の適用先は開始時の `(session, repo)` を固定(PR 側で 2 度踏んだ罠)。

### 受け入れ条件

- Issues 画面を開くと Composer が上にあり、本文だけ書いて Create で GitHub に Issue が
  できる(oplog に `issue-create` と URL)。空本文は blocker。
- 作成後に一覧が更新され、その Issue を開くと Thread 表示、Reply で comment が付く。
- kagi を再起動しても Draft が残る。
- fake gh の transport_recording_test に `issue-create` / `issue-comment` の記録テスト。
- `cargo test --workspace` 緑、`check-all` `::error 0`、GUI E2E の既存 issues 系
  (`workspace_mode_toolbar` の Issues 節)PASS。

## 進め方 / 連絡(herdr)

1. w5:p19 はまず**実装計画**(モジュール分割・状態の置き場・書き込みの写し元・
   サブエージェントへの分担案)を自ペインに書き、`[plan]` で w5:p0 に送る。
   w5:p0 が承認(`[go]`)してから実装。
2. 実装は worktree `../git-client-issues` のみ。`cargo` は同時 1 コマンド。
3. 進捗 `[report] <hash> <要旨>`、質問 `[ask]` → `herdr pane send-text w5:p0 '…'`。
4. 完了: 自ペインに `[done]` + 検証結果。PR は w5:p0 が作る(base = `feat/pr-lazy-fetch`)。
5. 並行して w5:p1C からの PR レビュー依頼は従来どおり受けること。
