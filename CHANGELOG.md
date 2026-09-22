# Changelog

All notable changes to Kagi are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.

## [Unreleased]

### Added

- WIP 行に「次のコミットが載る点」を追加しました。HEAD の lane 色で中空 ring と badge→ring→HEAD の点線を描き、同じ HEAD の複数 WIP は一本の縦線を共有します。ring は実 commit の表示径に揃えた 2px stroke で、hover／選択でも塗りつぶしません。detached HEAD にも対応し、未ロード／unborn の HEAD に架空の点は描きません。（#767、ADR-0174）
- Issues / PRs の一覧に共通フィルターを追加しました。Open/Closed/All、複数ラベル、author、タイトル部分一致を組み合わせ、PR は draft と取得済み checks でも絞れます。updated / created / number / comments の昇降順、適用後の件数、既存の collection との AND に対応し、設定はタブ内だけで保持します。Closed は実際の closed 一覧を取得し、PR の merged も含みます。（#753、ADR-0198 / ADR-0200）
- PR の Closed/All は専用の読み取り結果として保持し、Graph の open PR 一覧・branch badge・inspector chip・定期更新には混入させません。PR mode を離れると state は Open に戻ります。closed PR は actionable な bucket から外し、Issues は Recent を既定とし、絞り込み中の追加ページ取得は明示操作にしました。（#753 review follow-up）

### Fixed

- preflight refusal の native 回帰テストを現在の通知契約へ更新しました。旧 dismiss-only modal の代わりに EN/JA の footer / error toast、oplog の具体的な拒否理由、repository 不変を検証します。製品の通知動作は変更していません。（#764）
- remote write の結果を観測できない場合、停止証明後に「未確認のまま制限を解除」を二段階で選べるようにしました。EN/JA の警告と解放理由の監査ログを残し、元の Unknown は保持します。実行中・結果不一致・通信失敗・ログ保存失敗では解除しません。SSH alias の実 host を期限付きで解決し、PR fetch の remote 識別にも使います。（#706、ADR-0196）
- 短いウィンドウでも確認ダイアログの対象リストと Cancel / Confirm を確認できるようにしました。zoom 換算800px以下では余白と見出しをコンパクトにし、復旧説明だけを既定で折り畳みます。警告・拒否理由・最終確認の注意文は表示領域内に保持し、対象は最後の行までスクロールできます。開閉の選択は高さ変更で反転せず、次の確認ではリセットされます。（#462）
- fork PR の merge 後も local branch を安全に扱えるようにしました。計画を background で作り、承認時の branch 名・full OID・不在を固定します。checkout 中・PR head 不一致は事前に「保持」と表示し、queue 投入時も branch を残します。gh 成功と server の merge 成立を両方確認した場合だけ既存の削除ガードを通し、後発の OID 変更・同名 branch 出現は削除せず Partial とします。receipt と EN/JA 通知に結果を残し、fork remote は削除しません。Unknown の照合で未着手 local branch の削除を要求することもありません。（#705、ADR-0202）
- conflict の Save / Abort が拒否された理由を EN/JA の通知に表示し、計画後の状態変化や conflict marker の残存を判別できるようにしました。footer のログ契約と oplog の英語詳細は維持します。（#711）
- Issues 一覧を最下行までスクロールすると次の100件を追加取得するようにしました。読み込み済み件数は続きがある間 `(N+)` と表示し、失敗時は一覧と cursor を保持して再試行できます。戻り・手動更新は先頭ページから取り直します。（#752）
- Issues / PR の本文とコメントの Markdown 描画を直しました。画像はリンクとして表示し取得しません（`![alt](url)` → `[alt](url)`、alt がなければ URL 自体をリンクに、参照画像も定義先の URL を保持）。URL に文字参照で改行などの制御文字を埋め込んでもリンクが壊れず、画像として復活しません（制御文字は percent-encode して 1 本のリンクに収めます）。HTML コメントは従来どおり非表示ですが、コード span や fence の中に書かれた `<!-- -->` は消えずに残ります。fence 内のコードに inline code 用の細空白が混入しなくなりました。4 本以上の backtick で囲んだ fence の中に書いた ``` も fence の終わりとは見なさず、その中のコードの改行をそのまま保ちます。Issues Thread・Composer の Preview・PR 会話は `kagi_ui_editor::markdown::prepare_github_markdown` の 1 経路を共有します。Editor の preview の画像表示は従来どおりです。（#751 / ADR-0142 追記）
- Issues / PR の本文で、コード span や fence に書いた `<!--` `-->` が `<!—` `—>` に合字化されず、書いたとおりの文字で表示されるようにしました。会話本文は contextual alternates（`calt`）を切って描画します。コード span は renderer 側に run 単位の hook がないため、地の文を含む本文全体で切れます（本文のどの文字も、原文の文字のまま描画されます）。Editor の preview の描画は従来どおりです。（#751 / ADR-0142 追記2）
- 新規 Issue の title に複数行を貼り付けると全文が title に潰れて body が空になっていたのを直しました。1行目から本文の既定タイトルと同じ規則で title を導き（`#` などの記法を剥がし、60 文字で打ち切ります）、残りを body に、Markdown と改行をそのまま（fence で包まずに）入れるので Preview でそのまま描画できます。1行目が空行や fence の開始で何も名付けないときは title 欄に触れず、貼り付けた全文を body に入れます。手で入力した title は従来どおり短縮しません。挿入は両方の入力欄の現在のカーソル位置に対して行い、入力済みの title / body を消しません。1行だけの貼り付けは従来どおり title に入り、本文欄への貼り付けの fenced block 化と Undo も変わりません。（#751 / ADR-0201 追記2）

### Changed

- **PR ページを Issues と同じタイムライン chrome に揃えました。** description / review / comment は Issues Thread と同じ borderless な行（40px avatar、`@login · 経過時間`、1px の区切り線）になり、line comment の diff hunk・suggestion・severity tag・`path:line` はその行に残ります。pinned composer は avatar・単一の eye ↔ square-pen トグル・有効なときだけ amber になる送信ボタンを Issues と共有し、PR home は triage 用の表のまま行の avatar・余白・hover を共通部品に合わせました。行と composer の chrome は `src/ui/timeline_row.rs` の 1 実装で Issues と PR の両方が使います。PR の下書きは従来どおりタブのメモリ上にあり（永続化なし）、「下書き保存済み」はその保持を指します。既存の投稿・review 経路、owner 固定、transport 記録は変更ありません。（ADR-0200 追記 / ADR-0201）

## [0.39.0] - 2026-09-20

### Added

- Issues に常設の Markdown Composer、Preview、コード貼付、永続 draft と本文からの既定タイトルを追加。Issue 作成と Thread の返信は送信先をタブに固定し、transport 境界で記録します。結果不明の投稿は再送しません。（ADR-0201）

### Fixed

- **未fetchのbranchを持つPRも、一覧から1回のクリックで開くようにしました。** PRページへ先に遷移してGitHub詳細を読みながら、base refとPR head refをバックグラウンドで取得します。fork PRはbase repositoryのsynthetic PR refを使うため、同名branchを別remoteから誤って読むこともありません。（ADR-0200）
- **長いエラー通知が画面を覆わないようにしました。** スナックバーは画面幅内の1行要約に収め、完全な内容はOperation Logに残します。Operation Logへ記録済みの失敗では閉じるだけのポップアップを出さず、確認・照合が必要な場合とログ記録自体に失敗した場合だけモーダルを使います。（ADR-0192、ADR-0196）
- **大規模 repository でも PR 一覧が GitHub GraphQL の 504 で開けなくならないようにしました。** 一覧は軽量な field だけを最大100件取得し、checks と mergeability は画面に見えている行へ、body と変更統計は開いた PR へ最大2並列で後追いします。未取得の CI を「check なし」や merge 可能として扱わず「判定待ち」と表示し、一覧更新・失敗・head 更新をまたいでも各段階が所有する値だけを安全に保持または無効化します。HTTP 504 の再試行は一覧だけ1回です。（ADR-0186）
- **Operations run from `cargo run` are recorded again.** Cargo hands the binary `CARGO_MANIFEST_DIR`, which the operation log reads as "this is a test harness — refuse the real `~/.kagi` unless `KAGI_LOG_DIR` says where to write". That guard exists so a failed fixture can never write into a developer's home, but a developer launching the app through cargo is not a fixture, and every operation came back "changed but not recorded". The app now drops the marker at startup when `KAGI_LOG_DIR` is unset; test binaries never run that startup, and every test that spawns the app sets `KAGI_LOG_DIR`, so their isolation is unchanged.

### Changed

- **A horizontal trackpad gesture now slides the sidebar instead of switching the whole window at once.** Only the sidebar has a previous/next page: it follows the fingers — damped, so it trails them and can never travel past one page — and the main pane stays exactly where it is, showing the same content, for the whole gesture. Releasing under 20% of the sidebar's width springs it back; past that it snaps to the neighbouring page, and only when that spring comes to rest does the workspace itself change. One gesture therefore moves at most one page, however far it is flicked, and a release no longer makes the sidebar jump. (ADR-0199)
- **The Graph sidebar's `Pull Requests (N)` row was removed.** The pinned Graph / PRs / Issues navigator above the list already names that workspace, so the row was a second entry point to the same takeover. (ADR-0199)
- **The page a gesture is sliding toward shows its real content when that content is already loaded.** The branch navigator always does (it is local Git data); a PR or Issue page does once its list has arrived, so moving between workspaces you have already visited previews the actual lists rather than a placeholder. A page whose list has never loaded still slides in as a shell — the preview only reads what is cached, and never starts a fetch. (ADR-0199)
- **The sidebar now starts 240px wide** instead of 200px, so grouped branch names fit before being ellipsised. Dragging the divider still overrides it.
- **A sideways swipe no longer scrolls the list underneath it.** Once the gesture is clearly horizontal it owns the wheel, so the page it is dragging stops taking the vertical part of the motion. A gesture that is anything else — including one with a slight sideways component — stays the list's, and scrolls it exactly as before. (ADR-0199)
- **In-flight indicators actually turn.** The rotating arrow on a "working" snackbar and in the status-bar footer was a text glyph, which cannot rotate — so a running operation looked like a hung one, while Fetch's own spinner (a real animated icon) turned as expected. Both now use that same animated icon, and the plain Info toasts — which report something that has already happened, like "Copied …" — use a bullet instead of an arrow that promises motion. Reduce Motion still renders every spinner still. A repository gate refuses a new rotating-arrow glyph in a string, so this cannot come back.
- **The PR workspace was rebuilt around what you triage on.** With no PR open the centre is a table — number, title over its branch pair, state, author, checks, changed files, age — under a strip carrying the open/draft counts and one sort control, in place of the wall of cards. The list's numbers now come from the fetch itself, so a row says how big a PR is and how stale it is without opening it. (ADR-0200)
- **The PR navigator is INBOX / MY PRS / REVIEW / ASSIGNED,** each with a count and a fold. They are filters rather than buckets, so a PR that is yours, awaiting your review and assigned to you appears in all three — the way GitHub's own views overlap. Without a known GitHub login the viewer-relative sections stay empty instead of guessing whose the PRs are. (ADR-0200)
- **Open PRs get a swimlane beside the body:** one lane per open PR tab, its commits newest first, the other PRs' rows faded. Clicking a commit shows it in the PR that owns it, switching to that PR's tab first. It draws only commits the open tabs already carry, so it costs no extra request. (ADR-0200)
- **A PR with a long conversation scrolls smoothly.** The PR page is now a virtualized list — the same element the diff uses — so only the cards on screen are laid out; before, every review and line comment was built on every frame and a PR with dozens of Copilot comments stuttered. (ADR-0200)
- **The PR's 概要 and レビュー are one page again.** They were two separate scrolling panes, so reading a review meant losing sight of the description. Now the description (with its merge-status card) and the whole conversation are one scroll, the way the PR reads on github.com, and the two tabs are navigation into it: pressing 概要 or レビュー scrolls that page to the section instead of replacing what is on screen. The description is readable while the conversation is still being fetched. FILES, COMMITS and Conflicts remain real tabs. (ADR-0200)
- **The PR page opens with the PR's own title.** `#N`, the title at full width and its `head → base` pair now head the page, above the properties — the toolbar's copy is truncated into a strip it shares with the buttons, which is not where you look for what you just opened. The reviewers/assignees/labels rows are boxed like the description and the comments below them, and the page itself is the app's base background instead of the grey panel it used to paint. (ADR-0200)
- **The PR page reads like the PR: title, who, checks, then the discussion.** The head of the page now carries the state, the author with their avatar, the `head → base` pair and the size of the change (`+N −M`, N files). CI is a card on the page instead of a list filed in the swimlane pane: one line — "all checks have passed · N successful checks" — that unfolds in place to the individual checks, each with a button that opens its run in the browser. And the comment box gained the other two things you do at the foot of a PR: **APPROVE** and **REQUEST CHANGES**, posted through `gh pr review`. GitHub requires words on a "request changes" review and allows a wordless approval, so an empty box refuses the first and permits the second, with the reason shown rather than a dead button. (ADR-0200)
- **Reviewers, assignees and labels can be changed from the PR page.** Each row carries a gear that opens a picker: it lists what the PR already has plus what the repository offers, and applying it sends only the difference through `gh pr edit`. The PR on screen takes the new values at once and the list is re-fetched to confirm them. (ADR-0200)
- **Commenting works when GitHub is slow.** Posting a comment or review looked up the repository with a network call before sending, and failed with "not a GitHub repo" when that call timed out — even though the PR already knows which repository it belongs to. Every GitHub write now uses that stored identity instead. (ADR-0200)
- **A PR's properties are shown, and a comment can be posted from the PR page.** Reviewers, assignees, labels and the worktree line are now the first rows of the PR's own page, as a name/value table ahead of the description — GitHub keeps them in a right-hand column, which is exactly the width the diff would lose. GitHub logins carry their avatar: the PR's reviewers and assignees, and every author in the conversation, get the same circle the commit list uses (a real avatar once fetched, the initial circle until then). At the foot of the page there is a comment box: typing and pressing COMMENT posts through `gh pr comment` and re-reads the thread. The text follows the PR it was typed for — switching PRs parks the draft and brings back that PR's own — an empty box cannot be posted, and a post whose result could not be proven is reported as unproven instead of being retried. (ADR-0200)
- **The PR's commits are their own tab** instead of a 210px strip pinned above every view, and the tab row carries counts — files, discussion, commits. Picking a commit still takes you to its diff. (ADR-0200)
- **The PR detail rail is gone; its contents moved under the swimlane.** The 320px pane on the right of the PR workspace — the stack, CI checks, and the changed-file list — no longer exists, and the diff gets the whole width of the body. The checks, reviewers, assignees, labels and worktree line now sit in the lower third of the swimlane pane, which is already on screen for the PR being read; on the FILES and Conflicts tabs that area lists the files instead, so a file is still one click. The inferred PR **stack** was dropped with the pane — it came from other open PRs' base/head links rather than from `gh` — and ←/→ now cycles list → commits → files. Labels keep GitHub's own colour, and the worktree line answers what no GitHub field can: whether a worktree here has this PR's branch checked out, and whether it is clean. (ADR-0200)
- **A PR row in the navigator is the title, then a smaller line with its state and its number.** The title gets its own line with air under it; below it the state dot sits at the left and `#N` hard against the right, so a list of rows ends in a column of numbers. The head branch left the row — it repeated what the title says — and so did the attention *reason*: the section header above already says why the PR is there, and the home table still spells it out. The dot carries the row's attention colour, the open PR is marked at its left edge, and the failed/total check count and the agent badge ride between the state and the number. (ADR-0200)

## [0.38.0] — 2026-09-17

### Added

- **A worktree can be opened as a tab from the sidebar.** Right-clicking a row under WORKTREES now offers Open in new tab, Reveal, and Copy path alongside the lifecycle actions it already had; previously the sidebar listed every worktree but could only remove or lock them, and opening one meant finding its badge in the graph — or its WIP row, which appears only while that worktree is dirty. The main worktree's row gets the same path actions (and still no lifecycle ones), so from a linked worktree's tab it is the way back to the repository. Opening a worktree that is already open switches to its tab instead of duplicating it. (#733)

### Fixed

- **A toast no longer appears cut in half at the window's left edge.** The cards slid in from 500px to the left of a 460px-wide card, on the theory that a notification should arrive from off-screen — but a window edge is not a screen edge, so what it produced was a card sliced by the window boundary for the length of the animation. The travel is now bounded by the stack's own inset, so the card stays whole and the fade carries the motion. Where a toast comes to rest is unchanged. (#709)

- **The editor no longer claims your file changed on disk when it did not.** A working-tree watcher event says only that *something* under the tree changed, and Kagi's own fetch or save is enough to fire one — so an unsaved buffer used to be handed a "File changed on disk" banner, offering to Reload (discard) an edit nobody had touched. Each unsaved buffer's own file is now re-read and compared against the bytes that buffer loaded; the banner appears only where they actually differ. A buffer whose content could not be hashed when it loaded (binary, too large, unreadable) still gets the conservative banner, because it cannot be proven unchanged. (#736)

- **The stash-drop prompt offered after you resolve a stash-pop conflict can be confirmed again.** It reserves the modal slot while it still knows the stash only by OID, and the resolved index arriving with the plan was rejected as a mismatch — leaving the prompt on its loading state, refusing confirmation, so the stash you had just applied could not be dropped from the prompt. The unresolved target is now typed as such, and only the plan that resolves it may fill it in; confirming still requires a resolved target, and an ambiguous OID still offers nothing. (#723)
- Delayed remote refreshes, fetch completions, and file-menu actions stay bound to their originating tab session. Reopening the same repository cannot inherit an older fetch's display updates; dirty Pull can join its own in-flight fetch without starting another write. (#643, ADR-0197 S2a)
- Remote Browse and Update now occupy the same modal slot as repository confirmations and app notices. Workspace and Welcome apply one shared modal-key routing wrapper, so Enter and Escape act on the same slot regardless of which surface is visible and cannot fall through to a selected commit or diff. Independently-rendered modal stacks are no longer possible. A notice displaced by a newer modal returns to the queue whether or not it has actions because the user has not read it; a notice explicitly closed by the user returns only when it carries Inspect/Acknowledge. Asynchronously arriving notices always wait behind any occupied slot—including an earlier plain notice—without disturbing FIFO order. The running update installer remains window-owned when its modal closes, preventing duplicate installs and retaining completion status for the next open. (#643, #718, ADR-0197 S3a)
- Async operation completions now arbitrate the shared modal slot by origin and freshness. Explicit user actions may replace the foreground; delayed plans appear only when the slot is vacant and otherwise expire with a retry notice; terminal failures remain in the operation log and wait as app notices; in-place Remote Browse updates require the same modal generation. A failed Push or delayed Merge plan can no longer discard connection input entered while it was running. (#718, ADR-0196)
- GitHub and Branch Cleanup evidence now stays with its tab session, including results arriving in the background. Analyze caches and scan revisions cannot cross tab incarnations, and a newer HEAD supersedes an older mine. Conflict detection's run-once guard is session-local. (#643, Wave 4 S2b)
- Cleanup and squash scan results are now bound to their owner's read revision as well as their scan generation. A read accepted in the background cannot be overwritten by obsolete deletion candidates, PR evidence, or squash connectors. (#717)
- Cleanup and squash scans also track the exact published read model, closing the race where a scan and an accepted full load shared one read-request revision. (#717)
- Read caches, working-tree status, WIP diffstat, and undo history now belong to their tab session. Returning to a tab immediately distrusts retained cache payloads, rejects late results from the previous activation, and publishes a fresh full read; each tab restores only its own history, while undo/redo still live-preflight the branch and expected ref before writing. Ref-moving operations completed while their owner is in the background still enter that owner's history, but a detached owner receives nothing. (#643, ADR-0197 S4)
- Repository sessions, terminals, Conflict, File History, Analyze, Editor, Commit Panel, Main Diff, and Compare now live with their owning tab session instead of the window root. Switching tabs retains pane identity, scroll and edited buffers; background pane completions carry the frozen owner and cannot alter another tab's pane, modal, or footer. Activation distrusts retained repository-derived state—especially Conflict—and closing the owner drops its PTY and entity graph without cancelling an in-flight operation. Ownerless Welcome state rejects resource writes. (#643, ADR-0197 S5)
- Commit-list pagination, commit and cleanup scroll positions, graph offset, branch-group folds, and cleanup selections now stay with the tab session that owns them; inspector and cleanup column layout remain window-wide. Smart Commit generation status likewise lands only on its initiating session, drops after that session closes, shares the single modal slot, and probes repository-independent capabilities once per window. (#643, ADR-0197 S3b/S3c)

## [0.37.0] — 2026-09-08

### Fixed

- Stash push no longer rereads every unchanged tracked file while saving untracked files. It uses the hardened Git runner while retaining approval, preflight, stash/index verification and operation logging; an uncertain subprocess result requires reconciliation rather than retry. A two-file large-repository fixture improved from 12.5 seconds to 0.93 seconds. (#622, ADR-0176)
- Git subprocesses no longer inherit the repository-local Git environment. A `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE` or config redirect exported into Kagi used to override the repository each command names, so an operation planned against one repository could be executed against another; every git and `gh` child now starts with that environment cleared. (#623)
- A stash push identifies the entry it created instead of reading whichever stash is on top afterwards. An external `git stash push` racing Kagi's own could hand back a stranger's OID, which is what resolves the pop target for a dirty pull and what the operation log records as the recovery handle. When two entries are genuinely indistinguishable, the result is reported as unverified — the work is saved, and reconciliation is requested — rather than guessing. (#623, #618, #500)
- **Stash & Pull no longer reports a false restore failure for repositories containing unpopulated gitlinks.** Restore verification now compares only paths carried by the stash instead of scanning every tracked path, so unrelated unreadable gitlinks cannot turn a successful restore into a Partial result and large repositories avoid the unnecessary full-worktree comparison. The comparison follows the paths the restore actually writes, so a file the current HEAD renamed after the stash was taken is verified where its content lands rather than at a path nobody wrote. (#624)
- **Stash & Pull names the paths whose restore would conflict, before you confirm.** A dirty Pull now fetches first, then merges your edit with the incoming change in memory and lists the paths that genuinely fail to merge — a fast-forward pull cannot conflict commit-to-commit, which is why the existing merge prediction stayed silent for exactly this case and the collision only appeared when the stash failed to restore. Paths it cannot decide in advance (binary content, a mode change, a file added or removed on one side) are listed separately as *may* conflict, so the confirmation never asserts what it has not proven. On a diverged branch the prediction runs against the merge of your branch and the upstream — what the pull actually installs — not the raw upstream tree. The confirmation is delivered to the tab that asked for it even if you switch tabs while it fetches, never replaces a modal you opened in the meantime, and is refused rather than applied if the working tree changed after it was shown. A failed pre-Pull fetch opens no confirmation and reports itself in a dismissible modal and the operation log. (#625, ADR-0192)

### Internal

- Added ADR-0176 (application-layer stash boundary) and ADR-0192 (dirty-Pull conflict preview).
- The restore-conflict preview and the execute-time refusal read one calculation in `ops/pull_conflict.rs`, so the two cannot drift apart.
- The dirty-Pull confirmation's delivery states are enumerated in one place: fetch failed, tab on screen, tab absent, another modal open, tab closed.

## [0.36.0] — 2026-09-08

### Added

- **Pull no longer refuses a dirty working tree.** A dirty current-branch pull offers an explicit Stash & Pull confirmation: staged, unstaged and untracked changes are stashed through the planned Backend operation, the pull runs behind its usual preflight, and that exact stash is popped back by OID. A failed pull restores the stash before reporting, and a restore that conflicts keeps the stash and records the result as partial. The stash OID is written to the operation log, so the work stays findable if Kagi stops mid-pull. A pop restores file contents but not the original staged/unstaged split. (#618, ADR-0189)

### Fixed

- **A corrupt settings file no longer takes your session with it.** Settings read-modify-write moved into one store: an unparsable file is set aside under a unique name before anything is written, a failed rescue refuses the write entirely, and every save is a temp file renamed into place with the original's permissions. Repeated writes of one key (a column drag) coalesce, while separate settings, including the restored tab set, are written immediately. (#491, ADR-0191)
- **A subprocess whose wait was cut short is no longer reported as one that exited.** One runner owns every child, its pipes and its deadline; the stop type carries no exit code, so "we stopped waiting" cannot be expressed as "it finished". Output that never arrived is separate typed evidence, so a command that exits 0 with a truncated capture no longer reads as success. A child that cannot be reaped is handed to a janitor rather than abandoned. (#507, ADR-0188)
- **A failed pull keeps its explanation on screen.** A Pull modal holding an execution error survives the watcher's repository reload and is dismissed explicitly; ordinary confirmation modals still close on reload. (#618, ADR-0189)
- **Recovery handles are typed data, not prose.** Savepoint, stash, file-backup, branch-tip and history OIDs are recorded as structured fields on the operation log entry instead of being formatted into an English sentence, so anything that restores them no longer parses display text. Existing entries still read. (#500, ADR-0187)
- A remote repository with no commits yet reads as empty instead of failing: Remote Browse shows "(no commits yet)" for an unborn HEAD, while an unreachable host is still reported as an error. (#604, ADR-0089)

### Internal

- Operation-log reads take only the tail of `operations.jsonl` instead of parsing every historical line, so recording an operation no longer costs more as the log grows. Legacy id-less logs keep their existing index-based identity, and cross-process appends are covered by a two-process test. Numbering and append were already serialized under the sidecar lock; ADR-0181's text said otherwise and is corrected. (#499, ADR-0149)
- Added ADR-0187 (typed oplog recovery handles), ADR-0188 (subprocess runner ownership), ADR-0189 (auto-stash pull and error modal lifetime) and ADR-0191 (settings store).
- Operation-log reads take only the tail of `operations.jsonl` instead of parsing every historical line, so recording an operation no longer costs more as the log grows. Legacy id-less logs keep their existing index-based identity, and cross-process appends are covered by a two-process test. (#499, ADR-0149)
- **ADR numbers are unique again.** Two parallel merges each landed an ADR on a number another ADR already held; the newer ADR of each pair now lives at **ADR-0190** (NUL-framed `git log`) and **ADR-0191** (settings store), and every reference in `.rs`, `.md` and `AGENTS.md` points at the new number. A new `check-adr-unique-number` gate fails the build on any new duplicate 4-digit ADR number; the six numbers already duplicated are grandfathered by an allowlist that itself fails once an entry goes stale. (#620)

## [0.35.0] — 2026-09-08

### Added

- **Worktrees open straight from the commit graph.** A branch badge's tree glyph is its own click target that opens or switches to the worktree tab; main and detached worktrees are included, even for unreachable commits or several detached worktrees on one commit. (#591, #595, ADR-0185)
- **Remote branches can be dragged onto a local branch to plan a merge.** The remote-tracking ref is used directly without creating a local branch or fetching automatically, and confirmation remains required. (#590)

### Changed

- Dropping a branch onto a branch checked out in another worktree opens that worktree and plans its HEAD merge there, preserving the source worktree. (#605)

### Fixed

- **Busy notifications name the operation again.** The snackbar shown while an operation runs says what it is doing in English and Japanese instead of an internal writer tag, and an unknown label can no longer leak one. (#607)
- **The stash preflight refusal is a typed, localized note.** An approved stash that is no longer at its index reports which entry changed, in English and Japanese, instead of a raw English string. (#606)
- **A commit message can no longer forge graph or Remote Browse rows.** `git log` records are NUL-framed, a byte Git commit objects cannot contain. (#508, ADR-0190)
- **Recovery guidance and copied commands no longer recommend `git reset --hard`.** Amend keeps the working tree with a safe ref move, and pull undo uses revert. (#456)
- A save completing after a file switch no longer marks another buffer clean; saves remain bound to their originating buffer and written bytes. (#486)
- Stage and unstage failures now appear in the footer, notice, and operation log across their UI entry points. (#490)
- Failed pull-request fetches no longer look like an empty list, preserving cached results for authentication, network, and malformed-response failures. (#506, ADR-0186)
- Graph checkout strips display-only worktree markers and refuses a branch already occupied by another worktree before writing. (#603)

### Internal

- Added ADR-0185 (graph worktree navigation), ADR-0186 (PR fetch outcome contract) and ADR-0190 (NUL-framed git log).
- Added ADR-0185 (graph worktree navigation), ADR-0186 (PR fetch outcome contract) and ADR-0190 (NUL-framed `git log`).
- Codex GitHub reviews are requested in Japanese. (AGENTS.md)

## [0.34.0] — 2026-09-07

### Added

- **Flower Road gains balanced light and dark swimlane palettes** with stronger graph-row tinting while preserving the existing badge colours. (#529)
- **Deleting an unmerged branch now requires a deliberate two-step confirmation**, with its full tip retained through a ref-backed recovery handle. (#585)
- **Worktree WIP rows now connect directly to their checked-out commits** with lane-coloured dashed paths, making each worktree's next commit position visible in the graph. (#474)
- **Linked-worktree WIP rows now open an inline commit panel** where stage, unstage, commit, amend and discard target that worktree without loading another graph tab. (#477, #478, #479, #480)

### Changed

- Local stash push, apply, pop and drop now run through one application-owned planning, admission and receipt lifecycle. (#541)
- CLI and MCP operations now share one agent contract and return the receipt produced by their own confirmed run. (#571)
- Remote stash drop now runs as a typed application-layer SSH job with frozen connection identity and explicit recovery evidence. (#572)

### Fixed

- Release checks require the complete blocking CI aggregate from the target
  commit's newest workflow run and latest attempt, including the gate selftests. (#519)
- Branch cleanup retains remote recovery OIDs when subsequent local deletion
  fails, records partial completion, and opens the per-target operation details. (#519)
- Branch creation records partial completion when the branch exists but its requested checkout fails. (#524)
- WIP-to-HEAD dashed connectors remain visible across intervening stash rows. (#518)
- Worktree removal now preserves accurate receipts across partial failure and keeps production fault injection test-only. (#533)
- Rebase abort reconstructs its guard from replayed HEAD state, including later conflict stops. (#537)
- Worktree commands remain available when more than one Kagi process is open. (#538)
- Conflict mode re-detects repository state immediately after Continue or Skip. (#539)
- Stash drop follow-up waits for conflict reload instead of being consumed behind the conflict view. (#550)
- Expanding long oplog entries no longer applies a second, incorrect selection-position calculation. (#552)
- The footer consistently displays the first line of a multi-line status message. (#553)
- Closing a tab clears only that session's stash conflict and follow-up state. (#557)
- PR merge and SSH pull mutations are recorded at the transport execution boundary, including failed or unknown outcomes. (#558)
- Enter confirms only the active modal and no longer propagates to controls behind it. (#559)
- Editor history diff and snapshot loading are owned by request, preventing stale requests from leaving the editor stuck loading. (#560)
- Closing or reselecting background tabs preserves their live session, and successful worktree removal closes the removed worktree's tab. (#562)
- Backend execution policy is applied consistently and trust checks close previously reachable mutation bypasses. (#563)
- Rebase Skip that advances to the next conflict is classified from repository state instead of being reported as a failure. (#567)
- Discard and worktree-removal backups are anchored by refs so recovery bytes survive garbage collection. (#568)
- Plan and replan failures are explicit states instead of silent or stale confirmations. (#570)
- Branch-menu Enter no longer falls through to checkout behind the menu, and GUI tests use the same keymap setup as the app. (#579)
- Unknown conflict-process termination remains Unknown and retains its writer lease rather than permitting an unsafe retry. (#582)
- Branch deletion preserves partial progress and recovery handles after a post-backup failure, while worktree removal remains available on filesystems without creation timestamps. (#588)
- CLI and MCP confirmations preserve oplog receipts and backup recovery handles when execution fails or completes only partially. (#589)

### Changed (internal)

- Commit, compare and staging diffs share one patch decoder and inspect binary
  flags after libgit2 materializes content rather than guessing from empty hunks. (#519)
- The application-layer ownership and delivery model is specified before feature migration. (#521)
- CI runs the invariant checks for both dev pushes and pull requests. (#525)
- The first application-layer slice proves worktree removal across plan, admission, execution and delivery. (#526)
- Build guidance isolates Cargo targets per worktree to prevent cross-worktree artifact collisions. (#527)
- Writer admission now coordinates editor saves, staging, snapshots and fetch with worktree removal. (#530)
- The stash-family design defines local and remote operation ownership and evidence. (#532)
- A per-process GUI driver and opt-in startup activation make visual scenarios independently addressable. (#543)
- The canonical Kagi verification workflow is shared across Claude and Codex. (#545)
- GUI E2E scenarios unmount their windows and support focused scenario filtering. (#551)
- CI validates that canonical verification skill references remain resolvable. (#554)
- GUI E2E enforces a native-window budget and closes leaked windows before they can exhaust macOS. (#555)
- Verification documentation defines evidence tiers and the GUI hitbox API constraint. (#556)
- The remote-stash design fixes SSH identity, completion-token and reconciliation semantics. (#561)
- SessionId and frozen attachments own display identity, departure and delivery lifetime. (#574)
- Raw mutation executors are confined behind Backend boundaries. (#575)
- The conflict-family design defines request, evidence, lease and UI-adapter boundaries. (#577)
- Test fixtures isolate oplogs and reject fallback writes into the developer's HOME. (#578)
- Repository snapshots and reads are session-owned, replacing duplicated active-view and tab-cache state. (#580)
- Migration notes now reflect the implemented application-layer and session-ownership slices. (#581)
- Conflict Save and directory/file resolution now use finite application jobs and one recorded Backend boundary. (#583)
- ADR-0175 through ADR-0184 record the release's application boundaries, transport recording, execution policy, recovery refs, modal failure state, agent contract and session ownership decisions.

## [0.33.0] — 2026-09-06

### Added

- **Popup content is copyable.** Every plan card carries a hover-quiet copy
  button: one on the title row for the whole dialog (title, current →
  predicted, warnings, blockers, the row list, the recovery text and the
  structured recovery commands) and one per list panel for its rows. The list
  button copies the **full** paths, not the left-truncated form the rows show.
- **The confirmation cards say what is at stake.** A plan marked destructive
  carries a `Cannot be undone` chip next to its title, file rows carry the
  change-kind badge (`A`/`M`/`D`/`R`/`T`) in the same colours the file tree
  uses, and a list of ten or more rows is preceded by a per-kind tally
  (`M 115  A 2  D 5`).

### Fixed

- **Large file and commit lists are reachable again.** The amend card cut its
  staged-file list at ten rows with no "+N more" and no scroll; discard cut its
  skipped list at twenty; the push preview cut commits at ten. Every row is
  rendered now — the big lists virtualized, the bounded ones plain — and each
  list is a bordered panel that scrolls in place.
- **Cards no longer outgrow the window or overlap their own text.** List and
  prose panels are bounded relative to the window height with floors, so on a
  short window the list yields before the safety text and neither can paint
  over the pinned buttons. A long path or commit summary stays on one line
  (ellipsis at the start for paths, so the file name survives).
- **Popup surfaces match.** Plan cards, the remote browser and the trust prompt
  now paint the same surface as the Settings popup, and the inset panels are
  tinted from the theme's text colour — `surface` equals `modal` in Apple Dark,
  IBM PC, Monokai and One Light, where the panels used to be invisible.
- **Section disclosure works and does not stick.** The caret on a card's
  sections actually collapses them, and a collapse no longer carries into the
  next confirmation — collapsing "untracked files will be deleted" once used to
  hide it by default forever.
- **Checkout's overlap blocker lists its paths** instead of joining forty of
  them into one sentence.

### Changed (internal)

- Every modal is built from one shell (`modal_card` / `modal_body` /
  `modal_scroll_body` / `modal_list_panel` / `modal_section`), with one rule:
  a card has exactly one scroll region per panel and never a scroller inside a
  scroller. Section disclosure state is a `SectionOpen` type that only
  `section_open` can construct, so a literal cannot be passed by mistake.
- **The CI invariant gates are a uv project** (`ci/`, `kagi-checks`): one
  `check-<name>` command per gate, ruff + mypy clean, and
  `check-all --selftest` proving every rule still matches its own positive
  sample. They used to be inline `grep -rnE` plus `find | awk` shell scripts,
  where BSD grep's 255-repetition cap could silently make a gate check nothing.
  New gates: `check-shell-hygiene` (no grep/find/awk/`sed -i`, no bare
  interpreter in a workflow) and `check-klog-raw`.

## [0.13.7] — 2026-07-23

### Fixed

- **Push preview no longer over-counts commits for a branch with no upstream.**
  When a branch had never been pushed, the "commits to push" preview walked
  every commit reachable from `HEAD` back to the root instead of excluding
  history the remote already has via other branches (e.g. commits merged into
  `main` after the branch diverged) — a branch with one new commit could show
  a padded, capped-at-100 count. The preview now hides every
  `refs/remotes/<remote>/*` tip, matching what `git push` actually needs to
  transfer.
- **The plan confirmation modal (push/pull/merge/etc.) no longer overflows the
  window.** A plan with many warnings or preview commits could grow taller
  than the viewport, pushing the Confirm/Cancel buttons off-screen and out of
  reach. The card is now capped at 85% of the viewport height and scrolls.

## [0.13.6] — 2026-07-23

### Fixed

- **Japanese text no longer renders thin on Linux (Ubuntu).** The bundled Noto
  Sans JP fallback shipped as a variable font whose default weight axis is Thin
  (100); on Linux GPUI's text backend rendered every weight at that default, so
  Japanese UI text looked too light. Kagi now bundles static Regular (400) and
  Bold (700) faces, so text renders at the requested weight.
- **The window now binds to its launcher on Linux (Ubuntu).** The main window
  advertised no application id, so on GNOME/Wayland it appeared as a separate,
  unnamed taskbar entry with a generic ("gear") icon — clicking the dock icon
  spawned that stray entry, and quitting it closed Kagi. The window now sets its
  app id to `com.tomixrm.kagi`, matching the installed `.desktop` launcher, so
  the desktop environment groups it under the Kagi icon with the correct name.

## [0.6.0] — 2026-06-22

### Added

- **Activity tab.** A new repository Activity view shows commit/merge history
  as a compact chart plus contributor rankings. Granularity now covers fixed
  recent windows (Day / Week / Month / Year) and an **All** mode for whole-history
  analysis.
- **Instant chart inspection.** Hovering an activity bucket now updates the
  read-out immediately, with per-bucket tooltips, axes, and clearer commit vs
  merge colours.
- **Gitru-style graph lanes.** The commit graph gained stable lane colours,
  compact swimlane rendering, branch-lane tinting, and optional author avatar
  nodes. The setting is framed around avatar nodes; lane colours and compaction
  stay available independently.

### Security & safety

- **`Backend::run` scaffolds the enforced plan→preflight→execute pipeline**
  (ADR-0104). A new single entry point runs `preflight_check` (or
  `preflight_check_stash` for stash apply/pop) before dispatch, and the old
  `execute(op)` shortcut is `#[deprecated]`. NOTE: as of this sprint `run` has
  no callers yet — every real call path still uses `execute_*` directly. The
  caller migration is deferred to Phase 2 (blocked on ADR-0107 RepoSession).
  The concrete safety wins this sprint are the four wired-in changes below.
- **Merge is blocked on a dirty working tree** (ADR-0105), mirroring the
  cherry-pick / revert rule. Merge previously only warned, but it writes
  conflict markers into the user's uncommitted files when a real conflict
  occurs — `git merge --abort` would then discard both the merge AND the
  pre-merge edits.
- **`stage_conflict_resolution` is now atomic** (ADR-0106). A per-file write
  loop previously left the working tree half-resolved on a mid-loop disk
  failure (files 1..k overwritten, index never written, original markers
  gone). It now writes all resolutions to sibling temps first and renames
  them onto targets only once every write succeeded.
- **Stash pop verifies the stash count at preflight** (T-REARCH-015). A
  concurrent stash push between plan and execute previously shifted indices
  and popped the WRONG entry.
- **Discard now requires two-stage confirmation** (T-REARCH-014). The first
  click arms the red "Discard N file(s)" button; the second click on the
  relabeled "Permanently discard N file(s)" executes. Cancelling, reopening,
  or a failed execute all reset the armed state. The armed path also re-runs
  preflight before firing, so a repo change between the two clicks refuses
  rather than executing a stale plan.

### Performance

- **External-change refresh no longer blocks the UI** (T-REARCH-030).
  `reload_external` (triggered by a terminal `git commit`, a sibling worktree,
  or auto-fetch) now runs the git2 snapshot on a background thread and applies
  it on the UI thread. Previously a full status scan + topological walk froze
  the frame for an event the user didn't initiate.
- **Per-file diff content is cached** (T-REARCH-031). Clicking between two
  commits to compare the same file previously recomputed the full tree-diff +
  hunk extraction on every toggle; now the `FileDiff` is cached by
  `(row, file_index)` and invalidated together with the file-list cache.
- **Diff rendering is text-first with off-thread tree-sitter highlighting**
  (ADR-0109), so large file diffs become readable before syntax highlighting
  finishes.

### Fixed

- **CLI-argument tabs now open a real `RepoSession` on startup.** Passing a repo
  path on the command line no longer leaves the tab in a partially initialized
  state.
- **Operation errors now surface the real failure text** instead of the literal
  `"session unavailable"` placeholder.
- **The file-diff center pane no longer pushes the Inspector off-screen** on
  narrow or content-heavy layouts.
- **Worktree WIP row markers are clearer and complete.** Main/worktree WIP rows
  use distinct glyphs and count untracked files in the row status.
- **Swimlane graph polish.** Lane bands no longer protrude into the branch/tag
  column, avatar nodes are not clipped at the left edge, and label-to-node
  connectors align with the graph padding.

### Changed

- **`src/git/history.rs` renamed to `file_history.rs`** (ADR-0108). It
  collided with `kagi_domain::history` (the undo/redo operation history); the
  two describe different concepts and the collision made it unclear which
  `history::Foo` a caller meant.
- **Activity ranking now focuses on authors and windows** rather than per-window
  line-add/delete totals, keeping the UI compact and fast to scan.

### Changed (internal)

- **Extracted `kagi-git` as a workspace crate** (ADR-0115). The Git backend now
  lives under `crates/kagi-git`, owns the `git2` dependency, and is imported by
  callers/tests through `kagi_git::`.
- **Added `RepoSession` / `RepoWorker` infrastructure** (ADR-0073 / ADR-0107):
  per-tab backend ownership plus a dedicated repository worker thread for the
  next OperationController migration step.
- **Added pure domain activity aggregation** in `kagi-domain`, with unit coverage
  for bucket generation and contributor ranking.
- **Moved UI subsystems out of `KagiApp`**: `ToastStack`, `OpLogPanel`,
  `blocking_ops`, `render_helpers`, `modal_renderers`, and the Activity view now
  live in focused modules/entities.
- **Consolidated UI state** by grouping conflict fields into `ConflictState`,
  sidebar fields into `SidebarState`, and holding toast/oplog panels as GPUI
  entities.
- **Retired mutating `KAGI_*` headless hooks** in favour of direct git-layer
  integration tests; read-only harness hooks remain for UI-state smoke coverage.
- **Threaded GPUI context through operation logging/toasts**, removing deferred
  toast plumbing and making user-facing error paths more direct.
- **CI and docs now reflect the extracted backend boundary**, including updated
  grep-gate guidance, ADRs 0104–0108 / 0110 / 0115, the architecture cleanup
  roadmap, and the release/refactor handoff docs.

### Removed (internal)

- Dead code (Phase 0 sweep): `Backend::repo()` escape hatch (0 callers), the
  redundant `tempfile` under `[dev-dependencies]`, 23 dead `take_*` and 15
  dead `*_mut` modal accessors, the unused `CommandState::Hidden` variant,
  `render_status_footer`, and the obsolete `MAX_LANES` / `graph_width*`
  helpers (`modal_state.rs` 806→472 LOC).

## [0.4.0] — 2026-06-19

### Added

- **Branch Solo focus mode.** Right-click a branch badge in the graph and choose
  **Solo** to dim every commit that isn't in that branch's history; choose **Exit
  Solo** to restore. History is walked via first-parent ancestry. (#47)
- **Branch context menus on graph badges.** Right-clicking a local or remote
  branch badge in the commit graph now opens the branch action menu directly.
- **WIP-row diffstat.** The synthetic working-tree (WIP) row shows an aggregated
  staged + unstaged `+N / -M` count, refreshed from the backend on load/reload.

### Changed

- The WIP-row diffstat is rendered at the **right end** of the row, larger and
  bold, for better legibility.
- Per-file **Stage / Unstage** buttons in the commit panel are slightly smaller
  so they no longer exceed the file-row height.

## [0.3.22] — 2026-06-19

### Fixed

- **Smart Commit now finds the Claude Code / Codex CLIs in the macOS app bundle.**
  A `.app` launched from Finder/Dock doesn't inherit the login shell's `PATH`, so
  CLIs installed via mise / Homebrew / `~/.local/bin` showed as "not on PATH"
  (they worked from a terminal). kagi now resolves the login shell's PATH for
  both detection and execution. (#44)

### Docs

- Added a **File History** section to the README (English + Japanese).

## [0.3.21] — 2026-06-19

### Changed

- **Refreshed the app icon.** Regenerated the macOS `.icns` and Linux PNGs from
  an updated source image. Added `assets/README.md` documenting the icon
  pipeline (`xtask icon` / `scripts/make_icon.sh`).

## [0.3.20] — 2026-06-19

### Added

- **Smart Commit can use the Claude Code / Codex CLIs.** If you have the `claude`
  or `codex` CLI installed and logged in, you can pick it as the commit-message
  provider in Settings (in addition to local Ollama). kagi runs it
  non-interactively and **read-only** (it can never modify the repo) and shows a
  clear warning that your staged diff is sent to that external CLI and consumes
  your own account's usage/quota. Opt-in; off by default. (ADR-0099)
- **Connect to an SSH remote from the Welcome screen.** A *Connect to SSH remote…*
  button sits next to *Open Repository…* when no repo is open.
- **Recent repositories on the Welcome screen.** A list of recently-opened repos
  (name + path); click to reopen. Missing paths are dropped automatically.
- **New colour themes:** Pinky Boo, Catppuccin Latte, and Dracula.

### Fixed

- **In-app update now works for the AppImage build** (issue #29). The updater
  replaces the writable `.AppImage` file itself (download → verify → swap →
  relaunch) instead of bailing out. The tar.gz install path is unchanged.
- **Smart Commit LLM settings.** You can now enable Smart Commit's LLM and pick
  the Ollama model from Settings — the enable toggle was missing and the model
  picker didn't populate unless the commit panel had been opened first.
- **Update dialog is readable for long release notes** — wider (0.8× the window),
  the notes scroll, the markdown is sized down, and it follows the dark theme.

### Changed

- **The theme list is sorted alphabetically** (the default, Catppuccin Mocha,
  stays first) so the Settings picker stays tidy as themes are added.

### Docs

- Added a research note on GitHub pull-request integration (how GitButler/Fork
  do it; recommended approach for kagi) for a future feature.

## [0.3.19] — 2026-06-19

### Added

- **Switch branches without a forced stash.** Branch checkout no longer blocks on
  *any* uncommitted change — it only blocks when your local changes actually
  collide with the target branch (a path that differs between the two and is
  locally modified). Non-conflicting changes are carried over to the target
  branch with a heads-up warning, matching how commit checkout already behaved.

### Fixed

- **Stage/Unstage button colours.** The buttons used gpui-component's filled
  `success`/`warning` variants whose hover/foreground colours kagi never mapped,
  so the white label washed out (and gpui-component 0.5.1 hardcodes the hover
  text colour). They now use a translucent, theme-tinted style that reads like
  the branch-list rows.
- **The commit panel no longer closes when you stage/unstage a file.** Staging
  writes `.git/index`, which the file watcher treated as a graph change and
  triggered a full reload ~0.3 s after the click, closing the panel. Index-only
  changes now do a light in-place refresh that keeps the panel open.
- **Arrow keys now navigate the File History view.** In the per-file history
  view, up/down moved the (hidden) main commit list instead of the history
  entries, so the selection and diff never changed. They now move the history
  selection and update the diff.
- **File History selection highlight.** A hovered row used the selection colour,
  so the row the mouse was left on after a click looked "still selected" while
  the arrows moved the real selection — now hover uses a subtle tint and exactly
  one row reads as selected.

## [0.3.18] — 2026-06-18

### Added

- **Settings theme picker is now a real dropdown** (gpui-component `Select`) with
  keyboard navigation, replacing the hand-rolled inline option list. The On/Off
  toggles (Compact graph, Auto-fetch) are proper `Switch`es and the language
  choice is a `RadioGroup`.

### Fixed

- **Settings rows could overflow the panel.** Wide controls combined with
  unbreakable (CJK) labels pushed the control past the panel's clipped edge,
  hiding it; the label column now shrinks so the control stays inside.
- **Settings could not scroll when zoomed in.** Lower sections (Smart Commit /
  LLM) were clipped and unreachable; the content now scrolls, and the panel is
  sized to a fraction of the window so it always fits.
- **Diff text was hard to read on light themes.** Added/removed line text used a
  fixed light green/red that washed out on the light diff backgrounds; it now
  uses the per-theme colours, readable across all themes.

### Changed (internal)

- **Adopted gpui-component widgets across the UI.** Hand-rolled buttons throughout
  the modals, conflict views, inspector, commit panel, file-history/diff headers,
  and tab strip are now the shared `Button`; the conflict editor's icon button and
  the diff/settings controls follow suit. Reduces bespoke styling and keeps the UI
  consistent with the theme.
- **Unified the commit/branch/stash context menus** into one generic overlay
  renderer (they were three near-identical copies), removing ~260 lines.
- **Sped up debug builds.** The dev profile raises the GPUI rendering/text-shaping/
  layout crates to opt-level 3, so `cargo run` is no longer sluggish during
  development without slowing incremental rebuilds.

## [0.3.17] — 2026-06-17

### Fixed

- **Branch-picker dialog could swallow a row click.** The overlay's clickable
  rows were not occluded, so a mouse-down on a branch propagated to the
  full-screen dismiss scrim beneath it and closed the overlay before the row's
  click completed — selecting a branch silently did nothing. The panel now
  occludes, matching every other menu/modal.

### Changed (internal)

- **Tuned the release build profile** (`lto = "thin"`, `codegen-units = 1`,
  `strip = true`). Kagi's interactive cost is dominated by tree-sitter
  highlighting, git2 diffs and commit-graph layout, so this makes distributed
  release builds faster at runtime and noticeably smaller. (If Kagi ever feels
  sluggish during development, make sure you are running a `--release` build —
  debug builds are 10–50× slower on these paths. See `docs/linux-development.md`.)
- **Added a Linux/Ubuntu development & testing guide** (`docs/linux-development.md`):
  system dependencies, debug-vs-release performance, Wayland/XWayland, Blade/Vulkan
  device selection, the test suite, and bundling.

## [0.3.16] — 2026-06-17

### Added

- **Remote stash drop over SSH.** The stash context-menu **Drop** now works in
  the read-only remote view (ADR-0089 Phase 3): the same danger-confirm modal and
  oplog as local, executing `git stash drop` on the host over the system-`ssh`
  transport, then re-snapshotting (ADR-0097).
- **Remote pull over SSH.** The **Pull** button now works in the remote view —
  `git pull` runs on the host (its own credentials reach its `origin`), so
  fast-forward and clean-merge pulls complete; a conflict is surfaced for
  resolution on the host. Same confirm + oplog discipline as local pull (ADR-0098).

### Fixed

- **Commit detail panel no longer hidden on repos with long commit messages.**
  The center commit-list column had no flex `min-width`, so a long commit/merge
  message could push the right-hand Inspector off-screen (most visible on remote
  dev repos with long branch names): clicking a commit selected it but showed no
  detail. The column now shrinks and truncates so the Inspector keeps its width.
- **Stash graph connection lines were drawn off-screen on wide graphs** (many
  branches). Stash lanes are now packed from the lane count in use near the top
  of history instead of the global maximum, so the stash nodes and their
  connection lines stay visible (ADR-0088).

### Changed (internal)

- **Codebase structural refactor (issue #13).** Added `AGENTS.md`; split the
  `ui/mod.rs` god-file into `types.rs` / `render.rs` / `operations/` and
  `git/ops.rs` into per-op modules; extracted `settings.rs`; introduced an
  `ActiveModal` enum, a `view_models` layer, an `active_view` single source of
  truth, and a `klog!` log-contract macro (ADRs 0091–0096). Behaviour-preserving;
  no user-facing change.

## [0.3.15] — 2026-06-17

### Added

- **Remote repositories over SSH (read-only).** Connect to a host over SSH from
  **File → "Connect to Remote Host…"**, browse its directories, and open a repo
  to inspect its graph/branches/tags/commits and per-commit file diffs — all
  **read-only**. It is **agentless**: nothing is installed on the remote; Kagi
  runs short read-only `git`/`ls` commands over the system `ssh`, so
  `~/.ssh/config`, keys, ssh-agent, and `known_hosts` just work (set new or
  password-only hosts up in a terminal first). Remote views are structurally
  read-only — every write operation and the fs-watcher disable themselves
  (ADR-0089).
- **Per-file commit history.** A new view lists every commit that touched a
  given file, with a resizable list/diff split (ADR-0089).
- **Smart-commit: body generation in template mode** — the model now fills the
  commit body field, plus a model picker in Settings and an OpenCommit-style
  prompt (`think:false` for reasoning models); the Style toggle was dropped
  (ADR-0090).

## [0.3.14] — 2026-06-16

### Added

- **Stashes in the commit graph.** Each stash now appears as a row directly
  below the WIP row, in yellow with a stash (inbox) icon, and draws a branch
  line down to the commit it was created on — so you can see where each stash
  sprouted from, even when its base is an older commit (ADR-0088). Left-click a
  stash row to Pop, right-click for the Pop/Apply/Drop menu.

### Fixed

- The branch/tag (and stash) **label→node connector line now extends into the
  BRANCH/TAG pane** instead of stopping at the column boundary, and runs level
  across the divider (previously a ~1px step).

## [0.3.13] — 2026-06-16

### Changed

- **Stash actions in the sidebar.** Left-clicking a stash now **pops** it
  (apply + remove) instead of applying-and-keeping — so a stash you act on
  actually goes away. Right-click opens a menu with **Pop**, **Apply** (keep),
  and **Drop** (ADR-0087).

### Added

- **Drop a stash directly.** A new Drop action deletes a stash entry without
  touching the working tree, behind a danger-confirm modal that shows how to
  recover it (`git stash store <oid>`). The dropped commit is recorded in the
  operation log (ADR-0087).

## [0.3.12] — 2026-06-16

### Added

- **Background progress is a single, unified snackbar.** Every slow operation
  (merge, pull, push, stash, checkout, commit, …) runs off the UI thread and
  shows one busy snackbar with a large spinning sync icon + label
  ("Merging…", "Pulling…"). The old per-operation "X: started" toasts are gone
  (ADR-0086).

### Changed

- **No-op Push / Pull no longer pops a dialog.** When there's nothing to push
  or pull (already up to date), Kagi shows a quick "Already up to date"
  snackbar with the same big sync icon instead of opening a confirmation modal.
  Real push/pull operations still show the confirm modal (ADR-0086).
- **Merge no longer freezes the window.** Merge planning and execution run on a
  background thread; the sync icon spins while busy. The merge confirm button
  is now just "Merge" (it could overflow the window with long branch names).

### Fixed

- **Add/add text conflicts show in the conflict editor.** Files added on both
  sides (no common ancestor) — e.g. `.h` headers — were misdetected as binary
  and hidden; they now materialize as a 3-way text conflict.
- **Terminal loads your shell config and scrolls.** The embedded terminal now
  starts a login + interactive shell (so `~/.zshrc`/PATH apply — `python` etc.
  resolve) and vertical scrollback works.
- **Discard handles untracked files.** "Discard all" now includes untracked
  files (deletes them, backed up to the oplog first) and prunes now-empty
  folders — equivalent to `git clean -fd` but recoverable (ADR-0083).

### Performance

- **Branch / tag / remote sidebar is virtualized** (uniform_list), so scrolling
  and terminal typing stay smooth on large repositories.

## [0.3.11] — 2026-06-15

### Fixed

- **Fonts render consistently on Linux.** Kagi now bundles **Inter** (UI) and
  **JetBrains Mono** (terminal / conflict editor / code) and loads them at
  startup, instead of relying on the platform default and the macOS-only "Menlo"
  fallback (which rendered broken on Ubuntu). The look is now identical on every
  OS; CJK still falls back to a system font.
- **Window no longer opens off-screen.** The initial size is the preferred
  1440×920 but clamped to the active display (≤92% width / 90% height, 900×600
  floor), so it always fits on small / scaled displays.

### Changed

- Polished theme colors for secondary controls and the title bar.

### Docs / internal

- Documented the Linux build dependencies (apt packages) for building from
  source. Silenced macOS dead-code warnings for the Linux-only in-app menu.

## [0.3.10] — 2026-06-15

### Fixed

- **Linux AppImage installer now works with no arguments.** The zip nested the
  install script under `scripts/` while the AppImage and icon sat at the root, so
  `install_linux_desktop.sh` couldn't find them and only printed its usage
  message. The zip is now **flat** (script next to the AppImage + icon, per
  ADR-0047), and the script's auto-detect also searches the unzip root — so
  `unzip … && bash install_linux_desktop.sh` registers Kagi under `~/.local`.

## [0.3.9] — 2026-06-15

### Added

- **Checkout a remote-only branch from the commit graph.** Right-clicking a
  commit that carries a remote-only badge (e.g. `origin/feature` with no local
  branch) now offers **"Checkout '<remote>' as local branch…"** — it creates a
  local tracking branch and switches to it (the same flow as the sidebar). It is
  hidden when a local branch of that name already exists.

### Changed

- **Enter approves / Esc cancels the active modal.** When any confirmation/plan
  modal is open, Enter confirms it and Esc cancels it.
- **Taller title bar** for a bit more padding around the tabs and traffic lights.

## [0.3.8] — 2026-06-15

### Added

- **Cmd+Z / Cmd+Shift+Z for Undo / Redo** of git operations (ADR-0084). Bound so
  they never shadow text-input undo (in the commit message box) or the
  integrated terminal's Cmd+Z — they only act on the commit graph.
- **Undo works on a freshly-opened repository.** The undo/redo history is now
  seeded from the current branch's **reflog** on open, so you can undo the last
  operation(s) even in a repo you just opened (not only ones done this session).
  Switching tabs re-seeds from the new repo's reflog.

### Changed

- **Undo of a commit now uses `git reset --soft` semantics** — the undone
  commit's changes come back **staged** (index untouched, working tree
  preserved), instead of unstaged. Still a safe ref-only move: no `reset --hard`,
  no `clean`, and the commit stays in the object store + reflog.

## [0.3.7] — 2026-06-15

### Added

- **Drag-and-drop merge of upstream-only branches.** A remote-tracking branch
  with no local counterpart (e.g. `origin/feature`) can now be dragged — from a
  commit-graph remote badge or the sidebar remotes list — onto the current branch
  to merge it directly via its remote ref (no local branch is created).
- **Background auto-fetch.** Kagi now periodically fetches the remote (every few
  minutes, while a repo is open) so the commit graph and ahead/behind counts stay
  current without manual fetches. New **Settings ▸ Appearance ▸ Auto-fetch**
  toggle (on by default).

### Changed

- **The 🔁 Refresh button now also fetches** the remote in the background (so a
  merge done on GitHub shows up). It re-reads local state instantly and pulls the
  remote quietly — failures (offline / no remote) are silent.
- **Pull and Push are no longer grayed out** when there's "nothing" to do. Pull is
  enabled whenever the branch has an upstream; Push whenever a remote exists. The
  old ahead/behind gating used possibly-stale counts and caused a "can't pull
  after a remote merge" dead-end. A no-op pull/push is harmless.

### Fixed

- **"Discard all" now removes newly-added (untracked) files** too, instead of
  leaving them. Untracked files are deleted from disk after their content is backed
  up to the ODB (recorded in the oplog) — recoverable with `git cat-file -p <sha>`,
  exactly like a tracked discard. This is not `git clean` (ADR-0083). Per-file
  Discard is also offered on untracked rows now.

## [0.3.6] — 2026-06-15

### Added

- **Two new themes: Tokyo Night and IBM PC.** Tokyo Night is a navy/blue-green
  dark theme; IBM PC is a black-background CGA 16-colour theme.
- **Smart commit message: the Suggest button now uses the local LLM when one is
  available.** When Ollama is enabled, *Suggest* sends the staged diff to the
  LLM and uses its output (button turns green); otherwise it falls back to the
  rule-based suggestion (blue). The separate "Generate with Local LLM" button is
  gone — folded into Suggest.
- **Snackbar slide animation.** Toasts slide in from the left (fade in) when they
  appear and slide back out (fade out) when they expire or are dismissed.

### Changed

- **Themed window title bar.** The title bar is no longer the default OS gray —
  it is transparent so kagi's themed top bar (the repo tab strip) fills the
  title-bar area and follows the active theme. The strip is draggable and, on
  macOS, leaves room for the traffic lights.
- **Ctrl+A selects all in text inputs** (e.g. the commit message), instead of
  jumping to line start. Double-click word-select and ⌘A already worked.

### Fixed

- **Line-level conflict merge interleaves by position.** When taking individual
  lines from both sides of a hunk, the result now keeps each line in its
  original position instead of grouping all of one side first — so "base on the
  left, pull in just line 10 from the right" lands line 10 in place.
- Commit panel: hid the scrollbar on the stage/unstage lists (they still scroll),
  and added a left margin before the per-row Stage/Unstage buttons.

## [0.3.5] — 2026-06-15

### Performance

- **Commit panel no longer janks the whole UI.** It used to run a full
  `working_tree_status` every render frame (for the staged preview) and read every
  untracked file for a diffstat — so opening it on a large repo dropped the app to
  ~6fps and a bulk untracked drop (e.g. 300 images) froze it. Now the preview is
  cached, untracked files are not diffstatted, and the file lists are **virtualized**
  (`uniform_list`, O(visible) per frame) — scrolling stays smooth with hundreds of
  changes.

### Added

- **WIP auto-refreshes on working-tree changes**, not only on git operations: the
  watcher now watches the working tree and refreshes the WIP / commit panel when
  files change on disk (background status check; a no-op when nothing the repo
  cares about changed, so a busy nested worktree doesn't cause reload storms).
- **Persisted commit-list column widths** (BRANCH/TAG, GRAPH) — your resize sticks
  across restarts.

### Fixed

- Watcher no longer reloads this view on **sibling worktree / submodule** git
  activity (`.git/worktrees/…`, `.git/modules/…`) — fixes the reload storm from an
  active Claude Code worktree.
- Nested git worktrees/repos are no longer listed as a giant "untracked" entry in
  the commit panel.
- Header: a long repo/branch label no longer overlaps the Pull/Push/Branch
  buttons — the repo name now sits above a smaller current-branch line, each
  truncating with an ellipsis.
- Commit panel: the per-file Stage button is right-aligned again.

## [0.3.4] — 2026-06-14

### Added

- **In-app auto-update** (ADR-0082). On startup Kagi checks GitHub Releases in the
  background (best-effort, silent on failure, opt-out via Settings) and shows an
  **"↑ Update vX.Y.Z"** chip in the header when a newer release exists. Clicking it
  opens a modal with the current → latest versions, the platform asset, and the
  **release notes rendered as Markdown**. "Update now" downloads the asset,
  **verifies its SHA-256** against the release checksums, swaps it into the running
  install atomically, and relaunches — or "Skip this version" / "Release page" /
  "Later". Checking is opt-in and silent; installing is always confirmed and
  checksum-verified, writes atomically, and runs no destructive command.
  - Linux installs cleanly; **macOS/Windows are unsigned**, so the OS still warns
    on the relaunched build until code signing lands (ADR-0038 Phase 2). The macOS
    path is verified end-to-end; Linux/Windows install paths are implemented but not
    yet runtime-verified by the maintainers.

## [0.3.3] — 2026-06-14

### Added

- **Windows build** (x86_64), experimental / best-effort. Releases now ship
  `kagi-<version>-x86_64-windows.zip` (a self-contained `kagi.exe` — assets are
  embedded). The terminal uses `cmd.exe` and settings/avatars/oplog resolve under
  `%USERPROFILE%`. Built and packaged by CI; not yet runtime-verified by the
  maintainers, and unsigned (SmartScreen warns on first launch).

### Fixed

- **Conflict editor, mismatched-length sides.** Scrolling the longer of the two
  panes was clamped to the shorter side's line count (the panes share one scroll
  handle but had unequal row counts); each hunk now blank-pads the shorter side so
  both panes have equal height.
- **Conflict editor, missing context.** The A/B panes skipped non-conflicting
  context lines, so the Merged Result Preview contained lines that were invisible
  in the editor (reading as code at "unexpected positions"). Context lines now
  render on both panes (muted, with real per-side line numbers) and stay aligned,
  so each pane shows the full file and the preview is traceable to what's on screen.

## [0.3.1] — 2026-06-14

### Fixed

- **Could not commit after resolving a merge conflict.** After resolving all
  conflicts and clicking **Continue**, Kagi advanced to the commit panel but the
  commit could not be completed: the resolutions were never staged (the per-file
  Save is optional), so the index kept its unmerged entries — the Commit button
  stayed disabled and the merge commit was refused. Continue now stages the
  resolutions before opening the commit panel, and a resolved merge (MERGE_HEAD
  present, no remaining unmerged entries) is treated as "ready to commit" rather
  than re-entering an empty Conflict Mode, so the commit panel stays put across
  the filesystem-watcher reload that staging triggers. GUI-verified end to end.

## [0.3.0] — 2026-06-14

This release ships new user-facing features on top of the start of the v1.0
internal re-architecture. See `docs/rearch/` for the architecture work and
`docs/adr/0072`–`0081` for the decisions behind it.

### Added

- **Drag-and-drop branch merge** (ADR-0079, T-DNDMERGE-001). Drag a local-branch
  label — from the commit-graph **BRANCH / TAG** badges *or* the sidebar branch list —
  and drop it onto the current branch to **start** a merge. The dropped label follows
  the cursor; each badge is independently draggable (a commit may carry several
  branches). The drop only opens the merge **preview** (`Merge <source> into <current>`
  with current→predicted state, fast-forward vs merge-commit, conflict prediction) —
  nothing is merged until you confirm. Cancel leaves the repository untouched; on
  conflict it enters the existing Conflict Mode.
- **Settings button + window** (ADR-0080, T-SETTINGS-001). A gear button in the
  window's top-right (also ⌘, / menu bar) opens a settings view (sections for
  Appearance — theme, UI zoom, compact graph — and Language: English / 日本語),
  applied live and persisted to `~/.kagi/settings.json`.
- **Undo / Redo of operations** (ADR-0081, T-UNDOREDO-001). GitKraken-style
  Undo/Redo toolbar buttons that work after commit and merge, implemented as safe,
  reflog-backed branch-ref moves through the plan→confirm→preflight→execute→verify
  pipeline — every move shows a preview first, no commit is ever destroyed, and
  `reset --hard` is never used (undone commits stay recoverable via the reflog).
- **`kagi <repo>` CLI** — `cargo install --path .` puts a self-contained `kagi`
  binary on your `PATH`; `kagi <repo-dir>` opens that repo (no arg → Welcome).
- **Smooth commit** — the Commit button commits immediately (no confirmation
  popup) when the pre-commit checklist finds no blockers; blockers (conflict
  markers / secrets / large binaries) still show the safety modal.

### Fixed

- Integrated **terminal arrow keys** (shell history) and **Escape** (vim/less) now
  work — they were being consumed by global diff/close key bindings.
- Settings window: the top-right gear icon now renders (missing bundled SVG), the
  layout/contrast is correct (rebuilt as a native view), the theme selector is a
  dropdown, and opening Settings no longer panics.
- Header toolbar button cluster is now centered (was right-shifted).

### Changed (internal — v1.0 re-architecture groundwork)

- Extracted a pure **`kagi-domain`** crate (commit/graph/diff/conflict model, rules,
  plan types — zero `git2`/`gpui`) (ADR-0072).
- Introduced a **`Backend` façade** + unified **`Operation`** pipeline; the **UI no
  longer calls `git2` directly** (enforced by a CI grep gate) (ADR-0073/0078).
- Began decomposing the 16.7k-line `ui/mod.rs` god-file (modals, diff view extracted)
  and slimming `main.rs` (ADR-0076/0077).
- Added a **test CI** workflow (`cargo test --workspace` + the UI-git2-free gate);
  the suite stays green at every commit.

## [0.2.0]

- Conflict Mode (line-level 3-pane editor, merge-into-conflict), commit suite,
  repo tabs, themes, EN/JA UI, uniform zoom, integrated terminal, GitHub avatars,
  cross-platform distribution. (See the v0.2.0 release notes / git history.)

## [0.1.0]

- Initial release: commit-graph UX, branch/tag/stash/worktree management, staging +
  commit, cherry-pick / revert / amend / discard with dry-run safety.
