# Changelog

All notable changes to Kagi are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.

## [Unreleased]

### Changed

- Rebuild Settings as eight stable sidebar panes: Appearance, Graph, Editor, Terminal, Network & Sync, Smart Commit, Analyze and General. Retain every existing preference and native effect; use fixed category tiles, palette-driven theme cards, separate keyboard focus/selection/ON states, and wrapping EN/JA controls. Preserve native window chrome, the 715-point fixed width and content-driven height, and Analyze drafts across pane changes and reopening. (#1814, #1816)

### Internal

- Share the actual Settings renderer and typed input events between native Settings and the deterministic WASM verification host. Native storage/discovery stays outside the portable API; existing persistence remains authoritative. Browser coverage drives production controls, delayed/error Analyze outcomes, all panes, EN/JA, light/dark, resize and 167% zoom rather than a copied gallery. (#1816)

### Fixed

- Restore one bottom-left notification stack in the owning surface's centre content, including Conflict Mode and Settings' content pane. Persistent, transient and busy feedback share the same bounded anchor without shifting content or covering the sidebar, Inspector, toolbar or status bar. Remove conflict-footer routing while preserving typed notification ownership, audience-scoped Escape, actions, lifetimes and Operation Log detail. (Owner ruling, #1590 restored 2026-10-27.)
  - Keep busy cards from shrinking their Skip/Hide controls when durable notifications are expanded; bound caption/advice paint while retaining full text in accessibility names and hover help.
  - Keep busy feedback, disclosure and durable cards in one scroll viewport, with independent window/audience offsets. Forward and backward keyboard focus reveal the existing controls, including Skip and clone progress, without pinning manual scrolling. Narrow, enlarged Conflict views retain access to the oldest error and its real Operation Log detail while work is still running.

## [0.48.0] - 2026-10-27

### Added

- Share image, PDF, binary, symlink and submodule side-pair presentation between diff and conflicts. Raw conflicts use the text resolver's per-side Use / Used / Not used / Change vocabulary; PDF conflicts start with synchronized scrolling, and Save keeps staging the selected frozen OID/mode rather than preview bytes. Apply the shared 8 MiB image budget and magic-first PNG/JPEG/GIF/WebP/BMP recognition, with real decode outcomes and aligned image geometry. (#1742)
  - Route an absent side through the shared frozen deletion confirmation, retaining cancel/no-write behavior and actual index/worktree removal after approval; keep ancestor-only removal as one file-wide action. (#1734, #1742)
  - Keep ordinary `BM`-prefixed text in the text resolver, reject oversized frozen sources before probing their payloads, bound selectable text summaries, and release retired decoded images from the GPU atlas.
  - Group non-image metadata directly below padded source bodies, retain the visible symlink-target heading and accurate failure reasons, and show the PDF synchronization label when the file header has room. (#1773)
- Preserve conflict lifecycle and pure-predicate formal models as an advisory CI regression guard, with pinned TLC, explicit issue-mapped expected outcomes, red broken-variant checks, and runtime-reconciled ledgers. (#1731–#1733, #1740, #1743–#1746 (model-external runtime findings: #1734–#1738))
  - Replay 34 saved native-drill projections, reject a forged unarmed Skip, and retain explicit evidence gaps. Refine refreshed-Skip and unreadable-metadata guards; run the 84 lifecycle checks in bounded batches and validate 40 pure properties against 37 pure mutants.

- Present conflict resolution as a document workspace with borderless whole-file sources, one-click left/right hunk arrows and a More menu for both block orders and resets, previous/next hunk controls in the left commit footer, and directly editable Result with origin stripes. Remove detached adoption rows and Preview/Edit controls. Keep a collapsible 232px source list with top-down In progress/Saved groups, state letters, fill-only selection and a plain inline pending count when hidden. The file header owns one Save/Next file action; the unified toolbar retains its right icon group but no conflict-session controls (placement updated by #1764 below). (#1729)
  - Preserve choices and manual text across navigation and hide/show, protect divergent manual Results until explicit Reset, and retain accepted saved selections for locked, read-only inspection without auto-advancing. Saved membership comes from the current operation's accepted conflict-path/index read, never unrelated staged files or a UI success ledger; stash has no saved entries. Navigation does not write Git, and Save/Continue keep the existing revision, lease and guarded CommitPanel handoff.
  - Replace the choice pill with `←` / `…` / `→`, showing whole-side and mixed adoption state. Source conflict lines have checkboxes with same-pane drag painting; Space toggles the focused line and Shift+Up/Down paints the neighbouring conflict line across hunks. More offers Use both, left first / Use both, right first, Reset this hunk and the existing confirmed Reset all, with localized accessible control names.
  - Keep whole-side block order after Use both when individual source lines are unchecked; selections built from scratch still assemble by position.
  - Replace the gutter arrows/More menu with per-pane Use/Both action rows and resolved-state Change controls (FRD A+C): action rows share the 24px uniform-list height, the gutter is a control-free 12px band, Result owns focused-hunk Reset and confirmed Reset all, and the file header shows total conflicts and resolved count.
  - Follow the revised owner/HIG placement: group Skip and Abort with the operation name and step in one fixed-height conflict sidebar header; reserve the measured full current identity width (with `cherry-pick 10/10` as the fitting benchmark), ellipsize action labels first, and retain full-label tooltips. Short operation names leave room for recognizable Skip captions rather than unused longest-label padding. Use a framed neutral Abort button with hue-preserving red ink adjusted to keep at least 4.5:1 contrast against the actual sidebar/strip fill in rest, hover and pressed states; mute it when disabled. Keep only Continue after the file actions/Save, separated by a line and gap. When the sidebar is hidden, show the #704 strip under the main toolbar with Abort and the sequencer's visible Skip/Confirm Skip state; retain the strip when `ConflictView` is unmounted too. Repository menus use operation-specific, title-case Abort labels, Skip This Step… / Confirm Skip, and Review to Continue… in EN/JA; Abort remains available for admitted operations without an editor. Session controls are a separate accessibility Group outside the file ListBox, and hiding a focused session control returns focus to the sidebar toggle. The main toolbar has no conflict-session controls; admission, confirmations, revision/lease guards and execution safety are unchanged. (#1764)
  - Keep the armed sidebar Skip caption fully readable as Confirm / 確定, without ellipsis; retain Confirm skip / スキップを確定 in its tooltip and accessibility name, and the native menu's full Confirm Skip title. After #1755, startup shows the fallback Abort until the accepted-read detector mounts the resolved-merge overview. Continue retires that overview for the real CommitPanel, where the fallback strip and native Repository menu retain Abort; regression coverage exercises that transition and the guarded real abort write.
  - Keep Abort, Skip and Continue visible in the command palette and resolve their live enabled state and localized disabled reason through the same context-aware admission as the Repository menu. Enter, pointer and accessibility activation retain the existing guarded command paths.
  - Match stateful Repository menu labels in the palette: reopening after the first Skip visibly offers Confirm Skip / スキップを確定, without an ellipsis, before the existing second-stage action can execute.

### Internal

- Record the shared side-pair preparation and ownership contract in ADR-0228: bounded frozen-side backend reads, one off-thread preparation entry and host-owned accepted previews, with rendering and conflict decisions kept separate. Step ① preserves existing image caps/classification and visible controls; the decided shared 8 MiB image policy, renderer cutover, conflict PDF/actions and deletion acceptance belong to later steps. No later-step completion or verification is claimed. (#1742)
- Harden the preparation cutover with header-only loading, distinct read failures, one bounded retry per accepted observation and generation-checked retirement. Remove obsolete conflict byte-read facades; cover first-render Backend I/O, late selection/revision/retirement completions and failure recovery with deterministic native tests. (#1742)

### Fixed

- Make per-conflict Use / 採用 actions neutral-tinted like Both, preserving their shared geometry. Only available Save is prominent while resolving; unresolved files have no prominent action. Quiet adoption controls retain at least 4.5:1 text and 3:1 outline contrast against their actual conflict band and hover backgrounds. / 「採用」「両方」「変更」は中立な見た目を保ち、実際の背景・ホバー背景に対して文字 4.5:1、枠線 3:1 以上を確保する。 (#1766)
- Give danger confirmation sheets no default button: root Return leaves the sheet and any armed confirmation unchanged; Escape / ⌘. cancel, and only a focused button accepts Space / Return. Use caution symbols only for unexpected data-loss notes, localize Abort as 中止, and guarantee readable destructive filled-button captions in both appearances. (#1767)
- Keep persistent repository notices bounded outside the sidebar/Inspector. In every conflict layout, reserve their in-flow lane and summary below the column/side or saved header, before choices and panes. In Conflict Mode, show only the newest routine feedback in the existing fixed-height footer line for four seconds, with Status arrival, the busy spinner and trailing Skip/Hide; never overlay conflict actions (owner ruling, #1768). Keep Home/Settings/Graph placement unchanged. Assign each notice's typed subject before insertion or focus repair: Settings keeps its own notices, repository drafts/results/refresh stay in their repository window (including its same-window Home tab), and standalone Home keeps app-level notices (owner, #1806). Draft-rescue cards identify the frozen repository and explain that damaged saved edits were not restored and their copy was preserved. Normal previews point to Operation Log without internal filenames; when recording fails, the required acknowledgment instead retains the rescued-file detail without claiming a log entry. Preserve captured-owner Refresh Repository recovery and explicit persistent dismissal. / 永続通知と件数表示は見出しと本文の間に確保し、conflict の一時通知・処理中表示は既存の固定高フッターに一行で表示する。通知の対象は挿入・フォーカス修復より前に設定し、別 window の通知到着で focus を奪わない。救済カードは元の repository を明記し、破損した編集の未復元とコピーの保持を伝える。通常の内部名は操作ログだけに残し、記録失敗時の確認には保存先の詳細を残すが存在しないログへ案内しない。 (#1768, #1806)
  - Keep compact Skip/Hide/Show controls inside the footer's content box, including its dark-chrome border at fractional UI zoom. / Skip・非表示・表示の小型操作を、拡大率と Dark の境界線を含めてフッターの内側に収める。
  - Contrast-adjust Success footer text, deliver palette/startup theme notices in their owning window, and make Escape ignore invisible foreign notices. State that damaged saved edits were not restored while retaining their copy and log detail. / 成功フッターの文字を AA 補正し、テーマ通知を呼出元に表示。Escape は他の window の通知を消さず、破損した編集の未復元とコピーの保持を明示する。
  - Keep native regression probes aligned with the shared raw per-side Use actions, check absent-log guidance separately from the recording diagnostic, and bind direct runner evidence to the current Cargo artifact and its SHA-256. / native の確認対象を現在の各 side の採用操作に合わせ、ログ案内と記録エラーを分けて検証する。実行バイナリは現在の Cargo 出力から選び、SHA-256 を記録する。
  - Assemble each new notification's subject, actions and recovery ownership before classifying its lane or repairing focus, so an action-bearing Info result cannot steal a collapsed transient's focus. / 操作付き Info を一瞬だけ一時通知として扱わず、通知の全情報を挿入前に組み立てて既存の focus を保つ。
- Distinguish Commit Panel conflict status from Stage/Unstage with an outline-only label after the aligned file name, outside the trailing action column. Keep convention Rule labels in their warning hue, retain shared height/radius and row selection, and guarantee readable label ink/borders in light/dark and selected rows without changing Git. Shared selection-ink policy remains tracked separately in #1791. / conflict と規約の状態表示を塗りなしのラベルにし、操作ボタンと区別する。 (#1772)
- Release repository, conflict-draft and oplog sidecar locks synchronously when their final owner drops, even while an unrelated fork temporarily inherits the descriptor before exec. Share one explicit-unlock guard, preserve repository quarantine until every unconfirmed writer stops, and cover each owner with held-fork regressions without sleeps or acquisition retries. (#1805)
- Pin the advisory conflict formal runner to stable TLC v1.7.4 with a verified SHA-256 and version-specific cache, replacing the mutable v1.8.0 prerelease asset without weakening model expectations or hash-before-execution checks.
- Keep pristine branch-name fields quiet in Create branch and Create worktree; editing or Return reveals the shared localized instruction beside the focused field without relaxing any plan blocker. Wrap invalid-name reasons instead of hiding their repair guidance, and keep error ink readable on the actual modal surfaces. Replace checkout-off branch creation's duplicated AFTER tally with one HEAD/index/working-tree consequence shared by the visible row, accessibility and Copy all. / branch 作成・worktree 作成では入力前のエラーを抑え、編集・Return 後に入力欄の隣へ案内を表示する。実行後の重複表示を整理し、安全確認は維持する。 (#1774)
  - Treat Return on an empty branch name as inline form feedback only, without an operation toast or log entry; nonempty refusals retain their normal receipts. / branch 名が空のまま Return を押しても操作失敗の通知・履歴は追加せず、入力欄の案内だけを表示する。
  - Retain the previous preview while typing so the centered card and focused field do not jump during debounce; stale plans cannot enable Create. Only clearing the name bypasses debounce, not worktree-path edits. / 入力中も直前のプレビューを保って欄の位置を安定させ、古い計画では作成を有効にしない。worktree のパス入力は遅延再計画を維持する。
  - Preserve an already-visible name-error row's layout while its text is stale; hide the old text and accessibility alert until replanning completes so correcting an error does not move the focused field. / エラー修正中は古い文言を非表示にして行の高さだけを保ち、入力欄の跳ねを防ぐ。
- Complete both-deleted/renamed conflicts with an explicit confirmed deletion, and offer deleted-side versus modified-side choices for one-sided conflicts. Preserve renamed destinations instead of attempting to select nonexistent blobs. (#1734)
- Resolve Git-started directory/file conflicts at their original namespace: keep the directory or restore the file, remove the parked `~HEAD` artifact, and show the exact affected paths in the standard confirmation sheet. Verify both index and worktree outcomes; preserve recovery and refuse stale plans. (#1735)
  - Add deletion/parked-namespace formal obligations and independently red missing-route, empty-blob, parked-path, suffix-attribution and leftover-artifact controls.
  - Reserve the confirmation path viewport's capped row-count height so flex compression cannot hide the deletion target; check actual filename containment in both languages.
  - Model admitted recovery followed by a second preflight before namespace writes; retain a red mutant that overwrites a foreign change made during snapshot creation.
  - Use a neutral Remove from Index… action with a separate DD explanation; name the action/path in each sheet, reserve destructive confirmation for working-file removal, and show parked restoration as one move with directory `/` markers. / DD の状況説明をボタンから分離し、操作と対象を確認シートに明示。作業ファイル削除時だけ破壊的な確定ボタンを使い、退避ファイルの復元は移動1行で示す。
  - Preserve every unrelated index entry under Git's actual index lock, verify checked-out directory children through Git filters, and record measured Partial or Unknown outcomes instead of a predicted successful deletion. Cover nested directories, symlink boundaries and post-mutation measurement faults.
  - Retain the `index.lock` contention cause and smudge racy-clean entries before private-index publication, so unrelated same-size edits stay visible. Freeze the actual next-step label, distinguish working-file Delete from index-only Remove, and use sentence-style confirmation titles. / index.lock の競合原因と同時刻編集の検出を維持。次の操作名を確認時に固定し、作業ファイル削除と index からの削除を区別する。
  - Match conflict-plan path and group-label line heights to their dense rows, preventing fractional-zoom clipping in Japanese without relaxing filename-containment checks.
  - Keep a restored D/F file visible in Saved at its original path even when Git's conflict message names only the removed parking path; retain the directory-side entries without duplicates. / 退避名だけが conflict message に残る場合も、採用したファイルを元の名前で「保存済み」に表示する。
  - Retain deletion and D/F snapshot references in durable recovery receipts even when a later index write or verification fails, so removed bytes remain recoverable from Partial and Unknown outcomes. / 削除後に処理が失敗した場合も、復元用 snapshot を操作ログに保持する。
  - Recognize parked Git labels containing `~` without confusing them with the original path, and make real-CLI conflict fixtures independent of global Git identity. / `~` を含む Git の退避名を正しく識別し、fixture の Git identity をローカル設定に固定する。
  - Bound Saved-path parking attribution by source-tree entries, not by the number of tildes in an untrusted merge message. / 不正に長い merge message でも、退避名の識別処理を source tree の項目数に制限する。
- Serialize conflict writes across Kagi processes using a repository advisory lock. Save, directory/file resolution, Continue, Abort, Skip and merge finalization refuse busy repositories without blocking reads; process death releases admission automatically. (#1746)
  - Keep repository admission held when a CLI returns `TerminationUnknown` with live descendants, including Continue/Skip and nested commands. Transfer same-fd lock ownership to the existing process janitor and remove the originating thread's reusable admission; only whole-group stop proof releases it, never pipe closure alone. Probe without retiring an executing supervisor job, so later commands remain supervised. Known-success hooks with closed stdio do not quarantine admission; their helpers remain tracked by the existing group sweeper. Per-Sessions reconciliation remains separate; no second monitor is introduced. (#1780)
  - Keep repository admission probes non-recording so Continue, Skip and staging retain exactly one operation-specific refusal. Quietly skip contended auto-fetch until its next tick; retain queued successors and retry a repository-busy head instead of reporting an identity change.
  - Preserve Pull's specific repository-busy footer, show a short localized cause and retry-later guidance within the bounded refusal preview, and link recorded notifications directly to their Operation Log receipt. Clear only stale repository-busy Idle/Failed footers on later successful write/stage admission, including reworded or previous-language previews; preserve unrelated statuses. Keep staging's single path-bearing refusal and quiet auto-fetch unchanged.
  - Keep contended queued writes responsive with zero-wait background probes and capped retry backoff. Show one error notification per waiting intent, replan only after admission is available, and cancel retry tasks when their owner closes.
  - Re-arm the waiting head if another window wins admission immediately after a successful probe. Route non-contention probe failures through ordinary admission instead of leaving a misleading repository-busy wait.
  - Record user-initiated admission refusals under the requested operation's name, including Pull before its preparatory Fetch and Commit before execution. Keep one contention refusal per queued intent rather than per retry; silent auto-fetch remains receipt-free.
  - Preserve operation-named Abort/merge planning receipts through failed appends and show their required acknowledgment once, without duplicate log rows. Keep automatic PR-ref refresh quiet under contention and retry on its next tick; localize admission states and all error variants in EN/JA while retaining raw details.
  - Retain frozen, clean-filtered conflict staging inside the repository guard from revision preflight through index publication, including the private interleaving-hook path. Keep Abort's restored-path and raw stash index/worktree verification under the same guard; draft preservation still precedes admission, while directory/file snapshot refs follow locked preflight. (#1746, #1775)
- Recognize CRLF conflict delimiters, including zdiff3 base markers, without removing source-line CR bytes. Per-hunk Current, Incoming, both orders and manual Results preserve original CRLF/mixed endings through Save. / CRLF の conflict marker を正しく認識し、採用・両方・手動編集から保存まで元の改行を保持する。 (#1756)
  - Block pasted CRLF marker residue in manual Results and recognize CRLF separators in the staged checklist without changing source provenance or the checklist's seven-wide policy. / 手動 Result の CRLF marker 残留を保存前に拒否し、staged checklist でも CRLF 区切りを検出する。

- Make Graph, PR and Editor navigation leave the conflict workspace without discarding its live Result, selected file/hunk or scroll. Keep the in-progress operation and existing Abort/Skip actions visible below the toolbar, with one secondary Back to conflicts / conflict に戻る button that restores the same pane. Refresh retained conflicts in place while away; keep existing write-blocker explanations and admission guards. (#1748)
  - Retire every hidden conflict focus owner (Result, navigation, reset and sidebar controls) to the workspace root on keyboard/menu/swipe departure, and return focus to the same Result when Back restores it. If the operation ends externally while its draft is still pending or failed, reveal the retained pane instead of stranding it after the operation strip disappears.

- Restore native menu/queue regression coverage: explicitly edit the Markdown fixtures for code-input menus, the accessible-field inventory and PR Peek's real-input dirty guard, preserve the selected glyph during right-click, await reopened Commit Panel rows before capturing menu ownership, use a genuinely warned detached checkout for pending confirmation, and assert bounded single-surface checkout and palette-push refusals while retaining their exact durable blockers. Product behavior and safety gates are unchanged. (#1777)
- Record conflict Continue, Skip and merge-to-Commit-Panel handoffs in the backend worker before releasing the write lease. Closing the host no longer discards their receipts; recording failures retain the attempted outcome. (#1740)
  - Preserve the next-conflict stop in the recording-failure acknowledgment when Continue succeeds at one replay but its receipt cannot be appended. Classify from the execution outcome, not the display row's recording-failure conversion. (#1775 review)
- Freeze the approved merge commit plan through execution, refusing changed staging and re-presenting the new plan rather than silently committing the live index. Conflict staging writes the approved payload rather than rereading mutable worktree bytes, preserving index executable modes when `core.fileMode=false`. (#1743)
  - Share the planned and executed executable-mode policy so recorded Save verifies successfully, and apply path-specific Git clean filters to frozen Result bytes without rereading the worktree. Preserve CRLF when no normalization is configured; verify the filtered staged blob separately from the original worktree bytes. (#1775 review)
  - Integrate atomic-draft restoration and handoff receipts with the completion presentation boundary; retain failed-read admission while recovered Save still verifies its frozen clean-filtered blob and configured executable mode. (#1755, #1775)
  - Apply clean filters after materializing all approved Results, so a `.gitattributes` resolved in the same Continue controls its dependent files. Filters still receive frozen Result bytes rather than rereading those files. (#1775 review)
  - Apply the same materialize-then-filter ordering to per-file Save when `.gitattributes` changes its own clean policy. Stage filtered frozen bytes and verify their OID independently from the original worktree Result; ordinary file Save keeps its existing policy. (#1775 review)
- Invalidate sibling worktree reads when a conflict Abort plan restores a shared branch ref; detached and stash-only restores remain worktree-local. (#1744)
- Restore non-UTF-8 stash-conflict paths without lossy path conversion, and verify both restored index entries and worktree content instead of only HEAD. (#1745)
- Surface failed conflict-draft autosaves in the Operation Log and an error notification, retain live decisions, and block close/quit until retry succeeds or a second explicit discard is confirmed. Refuse Abort, Skip and directory/file replacement before repository writes when their recovery copy cannot be saved. (#1731)
  - Protect repository-tab close (including Cmd+W on the last tab) with the same retry/two-confirmation discard sheet. Require Abort plan and confirm to durably preserve the current live draft, never an older disk-loaded copy; localize shared Abort/Skip/D-F refusal guidance in English and Japanese.
  - Queue ordinary Result edits and source selections through the background writer instead of waiting for draft locks/fsync on the UI thread. Await durable flush off-thread at close/quit/tab close and Save/Continue/Abort/Skip/D-F boundaries, reject failed or superseded generations before approval, and retain Retry/two-step Discard protection while any draft remains unflushed. Flush precedes repository admission; queued acceptance still has an unacknowledged crash window. (#1760)
  - Integrate draft-flush barriers with mutation-owned conflict receipts: retain pending/error state on handoff failure, finalize pre-admission refusal receipts explicitly, and preserve typed refusal outcomes through preflight wrappers without reconstructing Skip receipts in the UI. (#1775, #1784)
  - Distinguish healthy draft-flush progress from storage failure, and disclose scoped ordinary-editor file counts and both loss consequences before mixed-state two-step Discard. Failure Retry flushes only conflict decisions; ordinary file changes still require a separate explicit Save All.
    - Use a non-question saving title during healthy flushes, and retain one exact autosave-failure log line per failed writer generation for formal runtime trace replay; editor-capture errors keep their separate update-failure contract.
  - Retry the current draft at explicit operation boundaries after storage recovers. Dock Quit/logout/restart capture every live conflict pane before window destruction and request urgent background flushes within GPUI's shared 200 ms shutdown budget. Slow/failing storage or abrupt exit can still lose an unacknowledged edit; no durability acknowledgment is claimed in that window.
  - Await real draft-flush admission in stash and sequencer recovery regressions; keep dismissal, tab departure and close races after write admission and before completion instead of assuming synchronous plan creation.
- Preserve the staged-merge handoff to Commit Panel when an accepted reload revalidates conflicts; do not recreate the retired Saved workspace behind commit review. Update conflict detector and co-author picker regressions for accepted startup detection and retained-pane failure gates without weakening stale-result rejection or hidden-menu keyboard isolation. (#1776)
  - Check native View workspace exclusivity only among Graph, Pull Requests and Editor, independently of the Editor Details checkmark.

- Preserve concurrent conflict-draft decisions across Kagi processes with an interprocess lock and content-based three-way hunk merge. Atomically sync and replace drafts, retain colliding/manual bodies for inspection, and recover torn documents from the last good `.prev` without treating corruption as an empty draft. (#1732)
  - Expose an ordered background draft writer with latest-snapshot coalescing (up to 250 ms before starting), generation-aware completion and explicit durable flush barriers; keep blocking `autosave()` durable for preservation callers. Queue acceptance is not durability: an unacknowledged captured edit can still be lost on abrupt exit.
  - Keep newer in-memory edits when preserving a clone after failed autosave. On upgrade, quarantine a damaged draft without a usable backup under an exclusively reserved `.corrupt[.N]` name, record the failure, and show an EN/JA recovery notice while loading the editable index-backed buffer.
  - Preserve worktree provenance through queued saves, surface unreadable saved-edit errors with EN/JA recovery guidance without retrying every frame, and share one standard-library advisory-lock implementation across drafts, repository admission, and Operation Log retention.
  - Reject superseded restore failures before they can remove a newer conflict pane or its unsaved Result. Bind asynchronous restore failures to their owner, read revision and detector generation even when newer reads observe the same Git state.
  - Record unreadable-draft failures with their complete cause in the Operation Log and show one bounded EN/JA preview instead of an error modal. Never replace an unreadable primary with its older backup; reserve backup recovery for absent or verified-corrupt documents.
  - Keep an existing conflict pane, unsaved Result and editor input when an ordinary Reload encounters a fresh draft-restore failure for the same operation. Leave its checked-read mutation gate closed until successful re-detection instead of discarding the live edit, and do not replace its failure preview with Refreshed feedback.
  - Keep invalidated hunk recoveries inspection-only across queued saves and reloads, including when their original source materialization later returns.
  - Preserve the earlier writer's exact recovery body and identity when its stale editor later autosaves another file. Deduplicate against the post-merge record actually written, including assembled-hunk overrides, rather than the editor's superseded in-memory Result.
  - Retain failed-restore editors across unrelated index observation changes, revoke already-open Continue approvals and recheck pane admission at Confirm. Reject superseded successful detections as well as failures before they can reopen the checked-read gate, while still announcing completed draft rescues.
  - Re-detect conflicts under the first accepted repository read when Open or a new tab supersedes speculative startup detection; fresh Open, second-tab Open and CLI launch show the conflict workspace without a corrective Reload.
  - Revoke Abort approvals on failed checked reads and enforce pane admission before confirmation or shared writer planning. When an external abort/completion removes the operation, retain an unsaved Result until the existing ordered writer durably hands it off; failed or stale completions cannot erase the editor.
- Keep Continue confirmations free of command disclosures, consistent with Abort, and distinguish a resolved rebase step from completion when more steps remain. (#1736)
- Preview the same pre-apply restore target used by stash-conflict Abort, and require explicit confirmation before Continue stages its resolution. (#1737)
- Honor the effective repository comment character, including automatic selection, when preparing and submitting merge commit messages; strip comments from the body while preserving typed subjects even when they begin with the comment character. (#1738)
- Reset create-branch editor ownership on reopen and initialize its visible value from the current modal model, so cancelled names cannot leak into later typing. (#1749)
- Use shared, counted conflict-resolution advice to explain why Discard all is unavailable instead of claiming that no files exist, including when other changed files are present. (#1750)
- Share extra-small control height and radius across Commit Panel Stage, Unstage and inert status badges; keep Conflict and convention badges fully opaque, contrast-checked and accessible as status text rather than disabled actions. Preserve normal row selection through convention badges while keeping Conflict chips inert. (#1752)
- Preserve the focused stash row after keyboard Peek in the compact navigator, including Enter, Space and context-menu focus return after stash indices shift. Align the unborn-HEAD stash regression with the exact English/Japanese bounded result preview while retaining the full plan, disabled-action and durable refusal reasons. (#1761)
- Animate the conflict file sidebar with the main sidebar's shared width reveal, clipping toward its toggle while keeping focus and button state immediate; Reduce Motion skips both transitions. (#1739)
  - Return focus to the toggle if a closing sidebar row regains focus before the final exit frame, preserving keyboard activation after unmount.
- Keep adopted empty conflict sides selected when materializing line choices; reject invalid line edits without resolving untouched hunks and safely ignore stale selection bits. Treat literal marker-looking source lines as content in readiness, Save and Continue, while unresolved and hand-typed markers still block. (#1746)
  - Materialize conflict markers wider than every source marker run and parse that exact width, preserving literal marker lines without blocking per-hunk choices after clean auto-merges. Keep incompatible older hunk drafts in preserved read-only recovery instead of authorizing them against the new materialization, including when saving another file triggers re-detection. (#1751)
- Bind Skip confirmation to the approved operation step and full conflict revision; refuse a CLI-advanced or replaced step immediately before Git runs. Report a zero-exit Skip without observed movement as an uncertain warning, never success. (#1746)
  - Keep execution-time stale-step refusals localized in the EN/JA footer and unattended notification, while retaining the English durable blocker and log contract.
- Refuse conflict Save and Continue after an external worktree edit, preserving the file and index. Show an explanatory header, disable Save, and offer index re-detection without discarding the matching draft or granting it fresh write authority. (#1733)
  - Preserve the original source fingerprint across watcher refreshes before the first choice, and allow an interrupted Save/Continue to retry when the worktree already equals its frozen text exactly.
- Show a saved submodule conflict's commit OID instead of trying to preview it as a blob. (4363857802)
- Restore conflict hunk choices, selected lines and both ordering modes after restarting a partial resolution; older drafts remain readable. (4363162008)
- Disable per-hunk Reset and Change for manually edited Results; only confirmed Reset all may discard those edits. (4363161991)
- Autosave direct Result edits immediately so quitting or restarting retains the latest text. (4361355968)
- Keep non-UTF-8 conflict paths byte-faithful when recovering the Saved list on Unix. (4362562421)
- Honor the repository comment character, including Git's automatic choice, when listing saved conflicts. (4363857789)
- Preserve leading and trailing spaces in conflict paths when identifying saved resolutions. (4361355977)
- Use Finder's white-preferred 3:1 accent rule for checkbox/radio indicators and shared switch/slider thumbs, rather than the 4.5:1 text-label rule.
- Keep untouched conflict hunks unresolved when changing line order. Reorder only already adopted content, preserve partial/manual/empty choices, and avoid adding no-op undo entries. Keep binary, symlink and submodule conflicts out of text hunk materialization. (#1728)
- Bind conflict Result inputs and saved drafts to their owning operation and file-stage identity. A later rebase conflict at the same path starts unresolved instead of inheriting the previous Result; unchanged reads preserve live editing, selection and undo. Keep older and identity-unknown drafts available through read-only Operation Log inspection, even without a recorded operation, without changing ordinary receipt Copy. (#1721)
  - Preserve the newest actual edit, including undo to unresolved, without letting an older live buffer or a decoded/cloned draft outrank a later saved result. Keep recovery history in the existing draft document.
  - Restore the shared Operation Log, Terminal and Activity panel slot during Conflict Mode. Reload the real index for Continue planning and both execution/staging routes; unreadable indexes fail before working-tree or index writes, while a separately saved resolution can continue on the same long-lived backend.
  - Revalidate the Result's exact operation/file identity before direct Save, Continue and merge staging; an old Result cannot overwrite a later same-path conflict or silently drop its replayed commit. Bind recorded Save payloads to that identity rather than granting them a newer observation's authority.
  - Freeze the original full Continue approval through confirmation and queued execution, including fully staged sessions. Refuse a replacement operation or changed staged content before any write; require a new explicit plan rather than rebinding the old approval to a refreshed editor.
    Include semantic extended index flags in that approval: staging a previously intent-to-add empty file cannot reuse an older confirmation even when its path, blob, mode and short flags stay identical.
    Bind persisted merge/sequencer/apply metadata and the replay/autostash operands actually used by the operation as well. Changed message, authorship, options or operand targets invalidate the old approval without discarding still-authorized file input; require a new explicit plan and fail closed on unreadable or unknown inputs. This operation-state check does not freeze global configuration, hooks or arbitrary external dependencies.
  - Retain durable drafts through Continue, Skip and merge completion even if the foreground callback never runs; reserve current-draft clearing for explicit discard. Keep already-open read-only inspection and its dedicated Copy synchronized with language changes.
- Present each requested Git result once: keep ordinary results in the footer, use bounded translated notifications only when away or after long operations, replace repeated results, and keep Pull's fetch/stash steps quiet. Keep hunk staging success quiet, preserve full failure details in the Operation Log without auto-opening it, and coalesce Busy/Quit feedback until writing completes. (#1698)
  - Preserve Pull's unknown/no-retry safety preview, and include the requesting visit's freshness-fetch time in the final no-op or write result without announcing a child Fetch.
  - Keep failure-detail actions reachable from Home, another tab, or a closed repository owner; dismiss the notification only after its receipt is revealed. Do not mistake permission/network errors mentioning `index.lock` for a competing Git operation, and clear the current owner's completed Stash Busy footer.
  - Keep receipt inspection and copying available without a repository tab, but disable history restoration with an explanation until that receipt's local repository is displayed; recheck its owner before planning.
  - Preserve the selected Pull target and useful successful result when moving it from the footer to a notification, reuse the same result card across executed and no-op requests, and deliver failed manual Fetch details even when its owner has moved to Home or another tab.
  - Give conflict Save/resolve/Abort, worktree maintenance and history the same result owner. Keep required recording-failure acknowledgment ahead of ordinary success/failure notifications without replacing the original outcome or retrying its append.
  - Retire only the matching consumed Continue confirmation on terminal failure, including unknown outcomes, so existing inspection can arrive naturally instead of leaving a stale retry surface.
  - Bind Clone's Open Repository action only to its own successful recorded result, preserve older failure-detail actions, and treat local/remote Stash completions from a previous tab visit as background results.
  - Keep normal remote refresh from shortening the usual lifetime of explicit SSH Pull/Stash result notifications, without overwriting a foreign footer or dropping its receipt. Bound successful Pull headlines instead of replaying raw Git stdout, while preserving structured targets and the distinct meaning of known up-to-date results.
- Finish Pull without waiting for recursive status reads in unrelated linked worktrees. Keep strict fast-forward refusal cards readable across unchanged refreshes, and discard them when the checked-out state, exact upstream mapping or tab visit changes; never promote a refused request into automatic execution. (#1720)
- Hide a running progress card with its accessible close button without stopping reads, Pull, planning or clone work. Keep it hidden across redraws, tab visits and Pull's fetch/write handoff; show genuinely new requests and still deliver final failures and Operation Log details. Keep Skip and writer/Quit safety independent. (#1719)
- Keep branch deletion labels neutral until the existing merged/squash judgment is known, and identify branch-cleanup targets in English and Japanese. Report merge/rebase conflicts as stopped rather than completed, distinguish failed/successful/unknown operations when their log cannot be saved, and explain up-to-date current-branch fast-forward Pulls. (#1699)
  - Preserve the immediate Operation Log undo/restore warning for successful or partially applied operations whose recording failed, without showing it for unchanged failures or refusals.
  - Propagate unreadable ancestry and squash evidence, so missing Git objects leave deletion menus neutral instead of falsely warning that a branch is unmerged.
  - Keep unrelated orphan branches deletable through the normal unmerged two-confirmation path, while preserving actual missing-object failures.
  - Surface failed Operation Log persistence in one required acknowledgment that retains stopped context and the original diagnostic. Do not call an initial merge/rebase or a rebase Continue paused at the next conflict fully completed; keep commit-level Skip success unchanged.
### Changed

- Give Graph, PRs and Issues one shell-owned sidebar: the same View → Hide Sidebar / ⌘B now hides it in every mode with the same slide, including Issues, which previously ignored it, and PRs, which hid it without motion. One sidebar toggle with a new `sidebar.left`-style icon sits in the titlebar right after the window controls in every mode (disabled on Home), replacing the PR-only toggle that pushed the toolbar's repository identity aside; the PR navigator drawer is removed. The traffic lights move to (9,11) so they, the toggle and the tabs share one centre line. (#1790)
- Reuse existing Git helpers for plan status summaries, cherry-pick/revert conflict paths, and push remote selection without changing displayed text or operation safety ordering. (#1705)
- Consolidate Git/app integration fixtures, low-level working-tree snapshots and oplog collection in `tests/support`, including Pull freshness/conflict scenarios, preserving hermetic setup, environment/drop lifetimes and independent safety assertions. No product behavior changes. (#1704)
- Share native GUI test repository witnesses, presentation matrices, squash evidence projections, writer waits and Pull/checkout fixtures while retaining scenario-owned safety assertions and teardown. No product behavior changes. (#1706)

### Fixed

- Reserve generic-operation cautions for actual fetched-path overlap or nonempty rebase replay. Show retained checkout changes, ref-only Pull, sidebar publish safety, tag publication and linked-worktree creation as neutral English/Japanese previews instead of amber warnings; preserve execution guards and approved identities. (#1696)
  - Give strict ff-only overlap warnings their own commit/stash-and-retry guidance instead of advertising unavailable ordinary-Pull buttons. Keep untracked paths that conflict with the checkout target blocked before admission, while unrelated untracked files stay quiet.
  - Route idle and queued checkout through an explicit typed, fail-closed admission policy instead of treating an empty warning list as approval. Predict untracked collisions from an unborn HEAD against an empty tree before admission.
  - Integrate the shared Pull/Push entry admission after #1697 without retaining obsolete informational caution variants; keep unknown warnings explicit and execute safe current/ref-only fast-forwards without a redundant card.
  - Detect staged file/directory obstructions and both ends of staged renames before checkout admission. Keep concrete-path blockers read-only and unrelated staged renames quiet in idle and queued checkout.
- Use the same risk-based admission for Pull, Pull ff-only, Push and publish from the toolbar, sidebar menu, command palette and shortcuts. Run safe sidebar publish and clean non-current branch fast-forwards without a redundant confirmation, retaining the target branch in progress and result footers. Keep risk choices and execution-time safety checks. (#1697)
  - Keep explicit destination review when publishing without an upstream to multiple configured remotes and no `origin`; show typed candidate names instead of silently publishing to the first remote.
  - Freeze the approved Push branch, upstream, remote name and effective push URLs; refuse destination/config drift before any write instead of publishing to a newly added `origin` or changed URL.
- Confirm recoverable ref-only Reset, unpublished Amend, backed-up Discard and local branch deletion losing no commits once. Keep two-stage approval for irreversible deletion and shared-history changes; show the force-with-lease remote target and exact lease instead of claiming it cannot be undone. Align English/Japanese consequences with recorded recovery. (#1695)
- Keep recovery-only refs out of zero-loss local branch deletion, recheck approved commit reachability before execution, and recognize published Amend history through remote-tracking refs even without a configured upstream. (#1695)
- Keep ordinary branch-retention refs locked through deletion, freeze existing Amend publication refs through Git's real commit boundary, and give CLI/MCP zero-loss deletion plans the same one-confirmation explanation as the GUI. Document that new remote-ref namespaces cannot be locked globally. (#1695)

## [0.47.0] - 2026-10-21

### Added

- Choose whether newly opened Markdown files start in Preview or Source in Settings. Preview is the default; already-open buffers retain their chosen mode and unsaved edits.
- View working-tree PDFs read-only in Editor as continuously scrollable pages, with page navigation, zoom and display-density-aware rendering. Rasterize visible pages on demand with bounded image memory and background work. Malformed and password-protected documents show localized errors; password entry and universal font fidelity are not supported.
- Preview PDFs from Git Graph commit diffs side by side: previous version on the left, selected version on the right, with independent continuous scrolling and no computed diff highlighting. Preserve historical source identity, renamed paths and explicit added/deleted side states instead of substituting working-tree content.
- Fit PDF pages to the actual reading-pane width with an accessible English/Japanese action shared by Editor and historical previews. Fit the widest page in mixed-size documents, including below 25%; preserve manual zoom and reading position, and explicitly reapply after pane or window changes. (#1686)
- Switch historical PDF comparisons between independent and synchronized scrolling. Synchronize page-relative positions in either direction without changing either zoom; retain independent reading positions when synchronization is turned off. (#1691)
- Copy the selected commit's local branch name with Cmd+Shift+C while Graph has focus, or through the Branch menu and command palette. Preserve Cmd+C behavior and text/terminal input ownership; without a selected local branch, leave the clipboard unchanged instead of copying the checked-out branch or a hash.

### Fixed

- Pull without an extra confirmation or stash when the fetched update can preserve local changes, including unrelated staged and unstaged edits. Show affected paths and explicit protection choices only for real overlap; route committed-history conflicts to Merge review instead of presenting stash as a cure. Recheck the approved checkout and upstream before writing. (#1690)
- Close Pull confirmation immediately when Enter starts the background operation, and prevent repeated Return from reactivating the toolbar. Run ordinary safe Push directly while keeping refusal, failure and recovery details in the Operation Log. (#1690)
- Reduce PDF scrolling stalls with preview-first rendering, neighboring-page prefetch and a bounded reusable raster cache. Keep usable images during refinement, skip obsolete queued work and release evicted GPU images explicitly. (#1691)
- Bound historical and working-tree PDF reads before allocating preview bytes, without copying complete ordinary text files merely to classify them. Keep oversized/read-failed sides distinct from absent versions.
- Preserve PDF comparison sources for symlinks and unresolved index conflicts: do not preview a working-tree symlink's target, and use conflict ours/ancestor rather than substituting HEAD for the unstaged base. Preserve a missing conflict base as an added side instead of a read failure.
- Identify retained historical PDF viewers by commit and file identity rather than display titles, while preserving same-file reading state.
- Preserve the previous HEAD PDF when previewing a staged rename, while keeping the current side on the index. Refresh rename identity from current status so a later addition cannot retain an unrelated old version.
- Restore Graph keyboard navigation after a focused historical PDF is closed or replaced, without taking focus from another input or modal.
- Keep Editor Save/Undo/Redo from reaching unrelated Git operations while file classification is pending or a size-limit placeholder is visible, including PDFs recognized by signature without a `.pdf` extension.
- Preserve keyboard Save for active and cached unsaved text buffers renamed to `.pdf`; a filename change alone no longer reclassifies retained text as a read-only PDF. Keep keyboard navigation working when a later read replaces the focused text Input with a PDF surface.
- Keep keyboard Save available while a text buffer refreshes in the background, and clear the current read's loading state when typing makes its result stale without clearing a newer request.
- Keep PDF error, loading and unsupported-diff explanations readable with the existing secondary body-text color; do not dim essential status like disabled controls. (#1665)
- Preserve the PDF reading position when zooming in, out or resetting to 100%. Keep useful page images visible during asynchronous rerasterization; page buttons navigate within the continuous document. Keep narrower pages on screen when resetting after fitting a mixed-width document. (#1678, #1686)
- Keep PDF reading keys inside the focused document: arrows, Page Up/Down, Space and Home/End scroll the document instead of selecting another file. Preserve File Tree and toolbar keyboard controls, and return focus safely when the PDF is hidden, replaced or closed. (#1680)
- Show shared vertical and horizontal PDF scrollbars when content overflows, with the existing visibility policy and draggable position indicators. Preserve the viewport, reading position, keyboard controls and read-only document state. (#1682)
- Hide or restore Editor's supplementary Diff/History pane from its persistent header button, View menu, command palette or Cmd+Option+E. Recover document space without losing unsaved text, Markdown preview, PDF reading state or the selected historical view; preserve preferred pane widths and respect Reduce Motion. (#1684)
- Keep Editor's header and Details control at the workspace edge when a PDF-only zoom redraw changes the document's intrinsic width.
- Keep Editor size-limit, binary and decoding explanations readable, and wrap placeholder messages inside their pane at narrow widths and enlarged UI sizes. (#1666)
- Keep Markdown Preview Undo/Redo from opening unrelated Git history dialogs, while retaining keyboard Save for unsaved Markdown edits.
- Render unsaved Markdown edits in Preview immediately, including images and Mermaid source, without an implicit save or loss of text Undo history when switching modes or tabs. (#1668)
- Make the Editor Markdown Preview / Back to editor action reachable by Tab and Shift-Tab, with Enter, Space and native accessibility activation. Keep keyboard focus across label changes without replacing the draft or its Undo history. (#1674)
- Show Mermaid's leading failure explanation instead of a trailing blank line or stack frame. Provide an English/Japanese fallback when the renderer supplies no details, and keep diagnostic text readable and wrapped inside the Editor pane. (#1670)
- Render Mermaid PNGs at their logical diagram size with raster density matched to UI zoom and the window's display scale. Preserve aspect ratio within narrow panes, reuse density-specific renders, and coalesce zoom changes without launching overlapping diagram conversions. (#1672)
- Open Staged and Unstaged diffs directly from compact Changes on a fresh session, without first opening the WIP composer. Preserve the shared file selection and reject delayed reads after a different file, commit, Compare source, session or panel owner takes over. (#1662)
- Keep the reading position when switching the same diff between Unified and Split, including wrapped rows, open review threads and a settled end-of-list position. Preserve selection, code-only Copy and focus; a different file or source still starts at the beginning. (#1660)
- Reuse review-thread overlay row mappings during unchanged PR diff redraws instead of rebuilding them for every scroll or selection update. Preserve thread anchors, split/unified numbering, copy selection and per-tab expansion state. (#1072)
- Preserve normal merge-conflict results on Git 2.39 when disabling repository-selected merge drivers. Use a fixed fail-closed shell builtin instead of attempting an empty executable; untrusted driver commands remain blocked. (#1657)
- Route squash-merge ghost connectors through nearby free graph lanes instead of detouring to the far-right width of unrelated older history. Keep overlapping connectors separate, reuse lanes for disjoint intervals, and preserve real parent lines, commit colors and WIP anchors. (#1627)
- Keep neutral confirmation-dialog target and section counts readable over their composed pill backgrounds in Nagi and Apple Light/Dark. Preserve the existing tint, geometry, semantic warning/danger chips and operation routes. (#1638)
- Keep long confirmation warning/blocker collections reachable in bounded scroll panels while preserving pinned actions and target lists. Structured note paths share their note's scroll owner instead of creating nested scrollers. (#1633)
  - Show the single affected branch count on Reset, local branch deletion and force-with-lease confirmation buttons in both confirmation stages, preserving named targets and existing safety checks.
- Include the saved-tip checkout-alignment step in Sync-to-remote recovery instructions, so recorded commands restore the original index and working tree before applying retained changes. Cover dirty and clean checked-out branches while leaving ref-only recovery's active checkout untouched; clarify manual recovery conditions in English and Japanese. (#1630)
- Keep Cancel visible when a long confirmation action cannot fit beside it: shared dialog footers wrap, and full action captions can grow onto multiple lines instead of overflowing. Preserve native button semantics, the safe cancellation path and two-stage confirmation. (#1646)
- Remove obsolete inputless-root replacement and extra teardown frame pumping from native UI inventory captures. Use the shared window teardown with leak detection enabled; retain native presentation needed to produce screenshots. (#1632)
- Preserve the compact Commit popup and input focus when an older commit finishes while a newer draft is being edited. Close only a consumed matching draft, keeping queued operations waiting for typing until the user dismisses the composer. (#1649)
- Restore PR sidebar clicks after a horizontal gesture ends outside the sidebar, including when the sidebar was hidden mid-gesture. Give repeated PR cards distinct section/repository identities. Rename the visible companion pane to Graph, use an outlined toggle with action-specific help and native accessibility names, and move sidebar visibility to a persistent leading-chrome control whose measured width participates in toolbar overflow; compact windows retain their existing PR rail launcher. (#1652)
- Keep the operation queue and its 32-entry cancellation history in a bounded scroll area instead of pushing Graph and the status bar out of a compact window. Pin counts, Cancel all and Clear; preserve individual removal, session ownership and the prohibition on cancelling a running write. (#1650)

## [0.46.0] - 2026-10-17

### Added

- Expand and collapse individual directories in the commit changed-file Tree, in both the wide Inspector and compact Changes view. Nested disclosure survives parent toggles and Path/Tree switches; file clicks still open their original diff. Shared accordion motion respects Reduce Motion, while keyboard navigation and accessibility immediately exclude hidden descendants. (#1628)
- Add Nagi Light / Nagi Dark: Apple-derived neutral chrome with a limited semantic color palette, grayscale generated avatars, readable text and fixed blue accents. Existing themes and defaults remain unchanged. Document universal-color-design guidance and color-vision limitations. (#1635)
- Make the compact-layout threshold configurable in Appearance, from 40 to 160rem with a 70rem default. Show its current logical-pixel equivalent, apply changes live and preserve them across restart. Compact layout now follows UI text zoom without double-counting OS display scaling. (#1640)

### Fixed

- Correct Nagi's washed-out colour hierarchy by limiting chromatic roles rather than globally reducing saturation: three blue/green/amber graph lane colours, neutral remote/tag metadata and ordinary syntax, and clear green/red diff cues. Brighten Dark selection, lane bands and ref chips; keep selected labels readable through row fades and inactive selection neutral. Expanded Dark operation-log details stay neutral. Preserve text and essential-stroke contrast, native neutral/disabled button paint, existing themes and defaults. (#1637)
  - Keep stronger opaque chip paint specific to Graph and its drag ghost; preserve semantic-label contrast in confirmation and operation-plan badges.
- Fit the compact Commit popup to its content instead of leaving empty space below the Commit button for short drafts. Keep long drafts bounded by the viewport and preserve the existing inputs, draft and anchor placement.
- Reveal Settings steppers and switches when reached by keyboard at enlarged UI text sizes, without pinning subsequent manual scrolling to the focused control. Smart Commit consent retains its existing explicit confirmation and privacy boundaries. (#1640)
- Reveal PR Peek's Compare files in compact Changes even when the fetched PR head is outside the loaded Graph. Hand off the compact PR navigator to its context menu so the popup cannot intercept menu clicks. Preserve retained commit drafts and the read-only Peek contract. (#1640)

## [0.45.0] - 2026-10-16

### Changed

- Retire the transitional platform-less Linux tar aliases after their v0.44.0 compatibility release. Linux archives keep the `-linux.tar.gz` names; macOS installer tar, DMG, Windows zip, AppImage and Debian packages remain available. Correct the release runbook to merge before tagging and verify the exact merged SHA's blocking CI. (ADR-0047)
- Establish an internal shared motion foundation for selection, list insertion and accordion transitions, extending existing pane timing and Reduce Motion / deterministic GUI-test policy. User-visible behavior is unchanged; dependency pins are unchanged. (#1617)
  - Review: unify batched fixed-height row insertion and removal in `RowSplice`, including retained ghost placement and continuous overlapping/reversed splices. No UI surface is added.
  - Bound active row-splice motion to 64 keys; larger changes settle immediately, including prior active motion, so large fetches and directory collapses cannot cause unbounded per-frame scans.
  - Apply insertion/removal diffs atomically through one `RowSplice::splice` call, checking the combined key limit before starting either side so oversized replacements cannot partially animate.
- Animate file-row selection in Changes and Inspector, small Changes status insertions/removals and Generated / Agent-artifacts folds, and supporting Git-operation disclosures. Fixed-height list slots, selection, keyboard and accessibility state remain immediate; Reduce Motion settles presentation without waiting. Destructive confirmation targets remain non-collapsible. (#1617)
  - Preserve readable filename, status and diffstat text throughout active selection fades: transient ink follows the existing on-accent contrast policy against the actual wash, rather than interpolating through low-contrast gray. Settled styles are unchanged.
  - Keep the Inspector viewport filled during long Generated-folder transitions by realizing displaced visible rows beyond the final-slot range, using measured row pitch and their original hit targets.
  - Keep overlapping animated file, fold, directory and overflow rows on their visible click target, without swallowing repository-folder drops, pointer focus or wheel input. Leaving the rendered Graph/Diff workspace retires retained file-list motion, so rapid return paints current semantic endpoints.
  - Prevent visible Changes removal ghosts from opening a covered file's context menu, without making exits focusable/selectable or blocking repository-folder drops.
  - Preserve the file row's keyboard origin when its inline Stage/Unstage button is pressed, while keeping covered animated rows from taking focus.
- Animate the sidebar's existing keyboard focus indicator and Editor file-tree selection wash, directory/ref-group disclosure and sidebar pane disclosure with the shared motion policy. Fixed-height virtual rows keep immediate keyboard/accessibility state while keyed content slides and fades; Reduce Motion remains immediate. Sidebar refresh arrivals animate without animating initial loads, repository switches or large replacements. (#1617)
- Animate Graph selection washes and accent bars by logical row identity, and slide/fade small commit and WIP arrivals in the existing fixed-height list. WIP removals retain fading presentation-only rows; scrolled reading anchors, Reduce Motion and default-instant GUI tests remain stable. (#1617)
  - Keep mid-insertion clicks on the visible commit/WIP/stash row instead of an overlapping predecessor. Real rows consume their click without suppressing ancestor folder drops or keyboard-focus handoff; ghosts remain inert, and viewport fringe content keeps logical paint order.
  - Settle retained Graph presentation before returning from another workspace center, including commit arrivals accepted while hidden, without consuming motion work on hidden frames.
  - Keep right-clicks on a closing WIP ghost from opening the covered commit's menu, without blocking ancestor folder drops or changing real-row context menus.
- Remove the temporary cosmic-text locale fork and use crates.io 0.19.0. Explicit bundled Noto Sans JP fallback preserves Japanese regular/bold glyphs across terminal, diff/code, commit Inputs, TextView and UI/status text in the Linux JA/EN × minimal/system-Noto × X11/Wayland acceptance matrix. Also apply the fallback explicitly to detached hover tooltips, interactive worktree hover cards and branch-drag ghosts, which do not inherit the window Root's font; their rendered inherited fonts have independent JP face-identity regressions. (#1479-S1b)
- Prepare the upstream dependency migration with native single-post selection coverage: exact giant-post rendered Unicode Copy, freshly painted mounted-post witnesses after reorder/prepend, and mounted partial → Select All → full Copy before the existing offscreen regression. This test-only preparation leaves the current selection implementation and offscreen/cross-post guarantees unchanged; their official-upstream cutover belongs to #1479-S6. (#1479-S3)
- Document the in-tree terminal as Kagi-owned frontend, selection/copy, scrollback and PTY integration rather than a temporary fork. Retain gpui-terminal's upstream origin, MIT/Apache notices and official alacritty_terminal emulator; terminal behavior and dependency pins are unchanged. (#1479-S2)
  - Remove stale registry-import metadata and the obsolete duplicate manifest so the owned package can be packaged; preserve its upstream source SHA in the README and retain both license files and attribution.

### Fixed

- Keep pointer actions on the visibly moving Editor and Navigator row during insertion and disclosure motion: covered siblings cannot replace the clicked file/ref/stash/worktree or open a subgroup's context menu. Compact leaf selection and nested localhost links retain their existing behavior; disabled targets, presentation-only ghosts and viewport fringes do not acquire row actions. (#1617, #1621)
- Retire retained Editor tree motion when another workspace hides it, so rapid returns from Pull Requests or Issues show the current selection and collapsed rows immediately instead of replaying an outgoing wash or ghost. Other workspace takeovers and Home share the same presentation boundary; ordinary new motion still runs after returning. (#1617, #1621)
- Consume context-menu clicks on presentation-only Editor tree ghosts and displaced edge rows, instead of opening the blank-tree New File / New Folder menu behind collapsing content. Real file, directory and blank-space menus retain their existing targets. (#1617, #1621)
- Measure the Navigator pane viewport when reopening from an entirely collapsed stack, so the body expands into unused flex space instead of remaining header-only and jumping at the animation endpoint. Closed-window resizing and Reduce Motion preserve the measured endpoint. (#1617, #1621)
- Clear the Navigator's retained keyboard-ring motion when its panes leave the front workspace or are replaced inside the compact rail, so hiding the sidebar or visiting another pane, Pull Requests, Issues or Editor cannot replay an unfocused ring on return. (#1617, #1621)
- Correct the existing right-selection motion GUI oracle to allow native pixel snapping when comparing full pane width; the same 327.5px/327.6px mismatch reproduced on unchanged dev. Production motion is unchanged. (#1617)
- Use native menu checkmarks for Theme, Language, Toolbar and the active workspace, with plain Graph / Pull Requests / Editor names (グラフ / Pull Request / エディター in Japanese). Add File > Open Recent and matching Dock entries from Home history, with history-only Clear Menu and a clearly separated, aligned portable menu group. Expose Find… only for an available target and disable Clear Menu when history is empty using native action availability. Preserve the existing sidebar shortcut priority; further Find navigation awaits public upstream actions. (#1604, #1608; Refs #1607)
  - Keep Clear Menu in File > Open Recent only; the Dock lists repositories that remain actionable after the last window closes.
  - Opening a recent repository from Settings or the Dock returns to the most recently used repository window and brings it forward, creating a window when necessary. Missing repositories also bring the error message forward instead of leaving it behind Settings or another app.
- Context menus in text fields drawn by Kagi follow the selected English/Japanese UI language and contain only Cut, Copy, Paste and Select All, including Editor, Conflict and Issue composition fields. Find/Replace bar inputs retain gpui-kit’s default English context menu because the pinned library exposes no customization API for those internal fields. (#1606)
- Command-Period follows Escape's child-first cancellation route: focused menus, dropdowns and Find bars close before the dialog or workspace behind them, including when a text field owns focus. Escape and Command-Period inside the compact commit composer close its popup without dismissing background notifications or losing the draft. (#1609)
- Open repository folders dropped from Finder onto a Kagi window or its Dock icon through the existing repository-tab opening flow. Window drops highlight the target; files and non-repository folders retain existing failure feedback. The macOS bundle registers as an alternate folder viewer without taking over the default folder handler. (#1605)
  - Clear folder-drop feedback immediately when a drag leaves or is cancelled, and show the highlight only where the repository target can actually receive the drop—not over dialogs or other blocking overlays.
- Quit (including from Settings), Close Window, and the window close button now protect unsaved Editor changes in every repository and file tab. One confirmation counts unsaved files and offers Save All, Discard, or Cancel; saving errors keep the window open, and running operations still prevent closure. (#1603)
- Protect unsaved Editor changes before restarting after an update. Save All explains when another operation prevents saving, interrupted saves leave the confirmation usable, and Quit upgrades an already-open Close Window confirmation. Dock Quit, logout and shutdown still require an upstream GPUI termination hook and remain tracked in #1614.
  - Review: Unknown operation results direct Cancel and recovery review rather than waiting for running work. Cancel retains installed updates as restart-pending, so Update offers Restart without reinstalling; a clean blocked restart uses only Restart / Cancel. Restart is explicitly retried after work or recovery, never automatic.
  - Preserve an open dialog when installation finishes in the background; the installed update stays ready for an explicit restart. Waiting advice follows the operation actually preventing closure, not an unrelated recovery reminder.
- Serialize user-text tests that implicitly read the shared UI language with the existing i18n test lock, preventing parallel language-switching tests from changing diagnostic expectations mid-assertion. Production behavior is unchanged.
- Replace internal operation names and implementation diagnostics in notifications, preview cards and Operation Log with English/Japanese action-oriented text. Existing receipts and stderr contracts stay unchanged; branch names, paths, commit IDs and raw Git diagnostics remain available. (#1588, shared presentation)
  - Preserve original blocked-operation log arguments, retain complete recorded failure causes and backup identifiers in Details/Copy, and keep recovery and shortcut guidance tied to actions users can actually perform.
  - Use display-only errors in worktree planning notices, lock-reason dialogs, checkout preparation and filesystem prompts; keep recording-failure checks independent of translated wording.
- English and Japanese interface guidance describes results and next steps instead of internal operation stages, identifiers and implementation details. Plan previews, queue statuses, recovery guidance, Settings help and accessibility descriptions retain Git terminology; the i18n CI gate prevents internal jargon from returning. Persisted operation records and diagnostic log contracts remain unchanged. (#1588)
- Explanatory hover help now disappears when its control is removed, moves away from the pointer, or its window becomes inactive—including during the display delay. Existing tooltip text and themed content remain, but formerly Kit-managed tooltips now use cursor-relative placement and GPUI timing rather than Kit overlay animation and adjacent-button grace, closer to standard macOS tooltip behavior. Control sizing and interactive worktree cards are unchanged. Shared registration and a CI rule prevent the old lifecycle from returning; no dependency fork or pin changes. (#1591)
- Inspector file lists and Restore inspection panes retain stable keyboard focus while scrolling through fixed-height rows, including generated files and overflow notes. Nested Restore panes keep boundary-wheel chaining and do not pull pointer scrolling back to the focused row; horizontal wheel gestures no longer turn into vertical row movement. (#1479-S5B)
  - Review: retain the minimum 940×660 Restore-card keyboard and nested-wheel regressions at 167% zoom alongside the added 1000×720 desktop coverage.
- Repair dev-red PR accessibility and branch-drag oracles after the approved safety/layout cutovers: deleting PR merges assert alert-owned focus, desktop drag coverage mounts 1000×720 and 1392×883, and a 940×660 compact leg drags real Local rail Popover rows. Self/alias no-ops, detached-HEAD refusal, native feedback and unchanged-repository assertions remain; production behavior is unchanged.
- Align the PR dashboard responsive GUI regression with the approved compact navigator below 1000px: Cmd-B still updates the shared Sidebar preference, compact keeps its rail, and desktop toggles its inline Sidebar. The 940/1440px table, EN/JA, 100%/167%, scrollbar-policy and two-axis reachability coverage is unchanged; production behavior is unchanged.
- Prepare sidebar Clear and repository/Settings IME Escape boundaries for official upstream Input APIs, without changing dependency pins. Clear retains its localized name, focus, query owner and compact appearance; Input naming regressions now inspect native AX nodes rather than fork render recorders. (#1479-S4)
  - Give workspace Escape its own positive key context so Input composition cancellation, toolbar PopupMenu dismissal and Settings theme-picker dismissal run before the unchanged workspace fallback. (#1554)
  - Review: give the co-author ListBox and pinned sidebar group reveal scoped Escape bindings, and route PR context-menu focus/dismissal through the shared MenuKeys helpers so a containing Popover cannot consume dismissal first. Audit all Popover child routes and cover compact context-menu layering.
  - Preserve tree Left and row-list navigation at the revealed row's key-context depth; regression coverage includes Left/Right/Up/Down/Home/End with a pinned group tooltip.
- Issue lists and conversations share Kagi's variable-height scrolling, preserving the reading position as posts change or text reflows and keeping the complete Reply preview reachable. Appended pages do not pull the reader to the bottom, and filtered lists retain explicit pagination. Existing text selection and Copy behavior remain unchanged. (#1479-S5D)
- Notifications stay fixed at the bottom-left above the status bar in repository and Settings windows, expanding upward instead of appearing below the toolbar on the right. The newest-preview/count disclosure, 30% window-height budget and 45% window-width cap from #1551 remain unchanged. (#1590)
  - Compact repository windows with an open bottom panel further fit notifications to its body minus the toast inset, preserving the 30% upper bound and fixed anchor while leaving the panel tab strip's primary controls exposed. Settings shares the busy-snackbar/toast renderer.
  - Review: compact Editor uses its actual capped panel height; Settings does not expose the clone-progress action targeting its repository window.
- Japanese terminal text keeps the bundled Japanese typeface when changing fonts, zooming, switching sessions or restarting a terminal. Status-bar text fitting uses the same Japanese fallback as drawing. (#1479-S1)
- Update the toolbar count scaling GUI regression to expect the reachable 170% preset after seven public zoom steps, rather than the former 167% cap. Direct legacy 1667-permille rendering coverage remains unchanged.
- Diff geometry is now Kagi-owned across Main Diff, File History, PR diff/conflicts and Editor WIP/history, without depending on the patched GPUI list APIs. The shared virtual list retains actual distant measurements across unchanged reloads/recolor, reflows reading anchors, and resolves giant wheel and scrollbar-end requests against the true tail without eager `measure_all`. Native extent contracts retain all seven scenarios and add physical thumb and offscreen review-card update coverage. (#1479-S5C; ADR-0222)
  - Review: near-tail conflict jumps, shrinking reflow and review-card collapse refill the viewport from the real tail; same-count review-card reloads retain the reading slot and fractional position.
  - Re-review: explicit near-tail jumps keep their requested header visible when subsequent rows wrap beyond the estimated height; bottom alignment is decided only after measuring the requested region.
- PR swimlane geometry uses GPUI's public uniform-list API for its fixed-height commit and elision rows. Shared list keyboard navigation accepts the existing row-scroll adapter directly; Branch Picker, Home Recent, Issue feed and restore inspection retain their current list geometry. This prepares the official-upstream migration without changing dependency pins. (#1479-S5A)
  - Review: horizontal wheel gestures retain the lane rail's horizontal scrolling without moving the vertical row list.
- WIP opens its desktop panel or compact Changes/composer without waiting for status, diffstats, file classification or the first Smart Commit login-shell PATH probe. File lists show a loading placeholder, retain their session/input owner, and reject opening reads superseded by a newer refresh. (#1570)
  - Review: invalidated opening reads retry even when no reload follows; Smart provider discovery starts independently of status publication; Amend waits for authoritative staged status, with English/Japanese unavailable help.
- Home Recent items offer a localized “Remove from Recently Opened” context-menu action through right-click and Shift+F10/Menu. Removal persists only the history entry; repository/worktree files, Git state and open tabs remain unchanged. Empty history retains Open Folder. (#1567)
  - Review: removal returns focus to the same row position (or its predecessor at the tail); removing the last row focuses Open Folder rather than the window root.
- Inspector detects explicit HTTP(S) URLs in literal commit messages, with named Open Link and Copy Link actions for pointer, keyboard and accessibility in EN/JA. Authored HTML/Markdown stays literal; arbitrary schemes remain inert. (#1566)
- Fetch authentication failures identify the remote and advise checking credentials in English/Japanese, while Operation Log Copy retains the complete Git diagnostic. Non-interactive Git no longer launches a nonexistent askpass helper on macOS. No automatic authentication retry is added. (#1561)
  - Review: remote-branch menu Fetch uses the same localized summary/full-detail split, and malformed additive receipt detail no longer discards the whole row.
- Branch-menu Create worktree here starts with an empty new-branch field and a matching provisional path instead of proposing the already-existing source branch. Empty fields no longer show an inline required-name error; typed collisions retain normal EN/JA validation. Open worktree from branch, lock and removal safety are unchanged. (#1564)
  - Review: Return on an empty new-branch request is neutral, matching disabled Create; it does not append a Refused receipt or change the toast/footer.
- Help → Keyboard Shortcuts formats effective bindings with the same platform-aware shortcut glyphs as the command palette and menus, including remapped modifiers and named keys; EN/JA labels and unassigned-command omission stay unchanged. (#1563)
  - Review: Linux/Windows text notation formats each stroke of a custom chord independently (for example, `Ctrl+K Ctrl+S`), with the pure text formatter covered on macOS too.
- Main repository windows enforce a native 940×660 logical-point minimum, shared by saved-frame restore and runtime resizing, preserving supported compact chrome instead of allowing an unusable 200×150 window. (#1550)
  - Restore persistence coverage waits for each requested native resize before advancing its debounce clock, rather than comparing unchanged saved geometry with the initial clamped frame.
- Inspector changed-file options now fill the available row width, so filename, diffstat and trailing-space clicks open the same diff. Selection, hover and the existing keyboard focus ring share that full-width frame in Path and Tree views; indentation and arrow navigation are unchanged. (#1559)
- Inspector density and full-message desktop regressions now use the exact 1000×720 desktop breakpoint instead of expecting desktop-only frames in the 940px compact layout. Existing compact Inspector scenarios retain compact coverage; the small desktop density oracle caps downward movement without rejecting unrelated chrome improvements. (dev-red oracle repair, #1559)
- Stage all and Unstage all now publish the Commit Panel, session status counts and WIP diffstat from one post-write read, instead of leaving status feedback to the filesystem watcher's debounce and repeating the panel refresh. Linked-worktree actions retain their own repository scope; index mutation and receipt behavior are unchanged. (#1569)
  - Integration with nonblocking WIP opening invalidates older panel reads before publishing the batch status, without adding another repository scan. (#1570)
- Push tag confirmation shows the frozen tag object OID, peeled target OID, lightweight/annotated kind and every actual push destination instead of unrelated active-branch state. EN/JA presentation and Copy all redact URL credentials, query and fragment; receipt and klog summaries remain unchanged. (#1560)
- Dev-red tag navigation oracles now exercise inline-sidebar desktop behavior at the exact 1000×720 breakpoint instead of compact 940px; all tooltip, keyboard, pagination, Solo and unchanged-repository assertions remain, and 1392px variants are unchanged. (#1537 regression audit)
- Operation Log summaries reserve named EN/JA outcome labels and Copy ahead of supplementary actor/worktree metadata at enlarged zoom. Recorded operations with no ref movement disable Revert with a reason, while Restore to point retains its independent cumulative-history planning. (#1562, #1565)
- Branch Rename opens with a neutral current-name prefill and disabled Rename action; unchanged Return is inert. Edited invalid names retain localized inline validation without repeating the same cause in the plan. (#1552)
  - Review: unchanged-name tooltip and accessibility description say “Enter a new name” / 「新しい名前を入力」, not a permanently running “Checking plan…” state.
- Notification stacks show the newest preview with an EN/JA counted disclosure for older notifications; expanded cards scroll within 30% of the window height, preserving foreground workspace access and all Operation Log receipts. Errors still require explicit dismissal. (#1551)
- Settings fits its entire native frame inside its own display's visible area after opening, content-driven pane resizing, zoom changes and saved-frame restoration. Oversized pages keep the existing scroller; repository window sizing is unchanged. (#1549)
  - Page Up/Down scroll the native Settings page without changing the focused control, so the final setting remains keyboard-reachable when large content is clipped to the display.
  - Review: ordinary dragging and redraws preserve the user's partly offscreen or cross-display placement; fitting runs only for actual layout or display-configuration changes.
- Sidebar search now clears a nonempty query on the first Escape without losing input focus; a second, empty Escape returns to the list. Its Clear button exposes an independent localized EN/JA accessibility action and keeps search focused after native AXPress. IME cancellation is consumed before search cancellation. (#1547, #1548)
  - Neighbor disclosure coverage now mounts its inline Sidebar at the approved 1000px desktop breakpoint instead of looking for desktop rows inside the 940px compact rail; all filtering/disclosure assertions remain.
- Changes keyboard arrows, Home/End and PageUp/PageDown select the focused file through the existing lazy diff path, keeping its fill, accessibility selection and file-menu target together in desktop and compact layouts. (#1536)
- Graph's final Escape now closes an open bottom pane only after existing overlays, diff and commit-selection dismissal. Returning from PRs/Issues keeps the pane hidden until a fresh compact commit click or Up/Down selection reveals Changes detail/files through the existing selection hook; same-row toggles do not reopen it. Desktop keeps its right Inspector, and Editor/Conflict Escape semantics remain unchanged. (#1543)
  - Review: focused text Inputs keep their Escape semantics, including desktop Commit Panel title/body. Closing from a bottom-pane descendant hands focus to the Graph root before hiding controls, keeping the next Escape and ⌘J reachable.
- Rebase confirmation discloses the oldest-first commit range to replay, its count and commit identities instead of only repeating the unchanged branch/clean summary. The scoped list is localized in EN/JA; planning remains read-only. (#1530)
  - Review: planning is session-owned and off-thread, keeps 100 preview rows with the true count and EN/JA remainder, and formats Copy only on activation. Linear ancestor/no-merge ranges are already up to date. CURRENT freezes the current full OID; AFTER identifies the approved target OID and explicitly leaves the result unknown. Preflight refuses target drift before writes; execution uses the frozen OID.
  - Second review: refuse duplicate/in-flight planning before changing its latch and register the shared busy label. Frozen identities and uncertainty are display-only projections; semantic branch/clean summaries and successful durable receipts remain unchanged.
- UI Zoom now reaches 200% through Settings and the existing zoom shortcuts, persists across reopening, and describes the 70%–200% range in English and Japanese. Existing 100%/167% and migrated zoom values remain supported. (#1553)
  - Native Settings pane tabs wrap at high zoom rather than clipping the first/last pane outside the existing window.
  - Compact Sidebar search defers navigator opening until the shortcut handler releases its app update, avoiding a reentrant-owner read panic.
- Compact Changes reconciles file-row focus only from the surface drawing its lists, so the pinned composer no longer moves End/arrow focus back to an earlier file before scrolling finishes. Keyboard Discard coverage compares its menu and planned path with the actual focused row's accessibility name.
- Revert confirmation uses English generated AFTER prose in English and Japanese presentation in Japanese; commit subjects and branch names remain verbatim, including Copy all. (#1534)
- Stash Apply and Pop approval cards show the selected stash's frozen file count, paths and change badges, including index-only and untracked entries, using the existing EN/JA scrollable plan preview. (#1525)
  - Review: saved index-only content is explicitly labeled “not restored” (EN/JA), outside the restoration count; successful Pop loses that content because execution does not use `--index`. Apply/Pop Copy all includes the count, change kinds and full paths, and file/symlink changes retain their TypeChange badge.
  - Manifest read failures now remain explicit localized “unavailable” warnings in the card and Copy all, preserving the existing fail-closed Pop prediction blocker instead of failing plan creation. Unknown contents are not reported as zero files.
- Compact plan confirmations no longer cycle Tab back to the workspace root before reaching the card. Covered navigator/composer projections are dismissed when suspension begins, preserving subsequent keyboard focus and the existing confirmation safety policy. Covered rail triggers reject pointer, Enter and Space activation without opening a popover or switching workspace mode.
- PR merge confirmations classify branch deletion as destructive, reusing safe Return=Cancel and the alert focus trap. The card freezes the GitHub target/source identity, same-head L3 file and line totals, and a same-head file preview when available; unavailable totals/paths are explicit instead of L1 default zeros or an empty “no changes” list. PR Copy all includes the frozen paths without changing other cards' established rows. Local branch cleanup retains its separate guarded/recoverable scope. (#1529, #1531)
- Graph is a single keyboard Tab stop between Sidebar and Inspector. Its inset accent ring is visible for keyboard focus; existing commit arrows and selection actions work there, and Shift-Tab returns without clicking. (#1532)
  - Review: keyboard focus belongs to the named Commits ListBox in the native accessibility tree; switching to a cached unborn repository releases the missing list's focus to root so Escape and ⌘J remain reachable.
- Compact Graph commit selection now activates the Changes bottom tab and opens its existing Inspector detail/files projection for pointer and keyboard navigation, even after manually opening Terminal or Operation Log. Same-row reselection and refresh-retained selection leave the chosen tab alone; desktop selection remains unchanged. WIP keeps the existing working-tree Changes path. (#1528)
- Diff and File History header natural-width measurement copies no longer appear as duplicate accessibility controls. The real named actions, button sizing and icon-only fitting remain unchanged. (#1523)
- Remote/group header native coverage now mounts the exact 1000px desktop breakpoint instead of expecting inline navigator rows in the approved 940px compact rail. All eight appearances retain 120/240/340px sidebar widths, full-identity reveals, count-column/ellipsis geometry, accessibility hierarchy, targeted disclosure and unchanged-repository assertions; production rendering is unchanged. (#1481 regression audit)
- Commit Panel file labels preserve both ends with native middle ellipsis, keeping long alpha/beta names distinguishable in List and Tree. Full-path hover text uses the existing viewport-bounded wrapping tooltip; accessibility retains the complete path. (#1533)
- File → Clone Repository and its existing shortcut now open Home's Repositories picker instead of remaining disabled as an unimplemented placeholder. Selecting a repository uses the existing clone confirmation card and safety lifecycle. (#1522)
  - Review: another open modal or visible confirmation blocks navigation without stealing its focus; portable menu dismissal cannot restore the covered workspace's focus over the picker.
- Editor Blame uses a named, keyboard-focusable toggle Button with a visible focus ring and toggled (pressed) accessibility state. Enter, Space and pointer activation share the existing on/off path in EN/JA. (#1524)
- Activity chart hover and keyboard inspection targets now align with the plotted bucket positions, including narrow plots at 167% zoom; neighboring marks share midpoint hit boundaries without changing aggregation or series encoding. (#1510)
- Smart Commit's first Settings enable now requires the same four localized privacy statements as Generate, including unstaged exclusion and the staged-secret warning. Approval only enables the preference; cancelling, Escape or closing Settings sends nothing and leaves it disabled. Consent survives OFF/re-enable independently from the enable flag, and older enabled settings without recorded consent must ask before generation. (#1509)
- Editor History read failures retain their cause and show a localized Retry action instead of claiming that the file has no history. Retry uses the existing file/request owner; successful empty reads and stale responses remain distinct. (#1515)
  - Review: display-control bytes in backend causes are escaped, previews are bounded to three lines/480 characters, and Retry stays pinned outside the cause scroller. History and Snapshot share one named Button adapter.
- Conflict Editor Reset all confirmation is bound to the edited file and disarms on navigation, selection changes and blur. Result Preview/Edit uses explicit shared TabList selection, so pressing an already-selected segment preserves mode and manual edits; writer-busy controls remain inert. (#1511, #1512)
  - Review: keyboard arming keeps focus and its ring on the Reset Button, so a second Return or Space confirms without re-tabbing; pointer arming still enters the same blur-disarming focus scope.
- Command palette result rows expose a standard accessibility Click action for commands, themes and languages, sharing the pointer/keyboard enabled gate without double pointer activation. Unavailable results remain selectable but do not execute. (#1513)
- Update installation disables Skip this version, and replaces Later with “Hide — update continues” (EN/JA). Hiding retains the update offer and window-owned installer; reopening cannot start another installer, and failures restore the pre-install choices. (#1514)
- Compact ⌘F opens the existing navigator Popover and focuses its shared filter; short-window rails scroll and reveal keyboard-focused controls. Rail triggers expose expanded state and Enter/Space toggle the same Popover as pointer activation. PR/Issue section headings support keyboard disclosure in both layouts, and compact PR lists use the existing system-policy scrollbar with a session-owned scroll handle. (#1491–#1494, #1502)
  - Review: focused-trigger reveal runs only on focus transitions, preserving subsequent wheel scrolling to PRs/Issues while a different rail control retains keyboard focus.
- Operation Log summary time, operation and outcome now share the 13px dense-list identity role while retaining the 22px summary floor and variable-height details. Issues navigator age, comment count and number use the readable 13px secondary role without changing card height or selected foregrounds. (#1498, #1499)
  - Review: narrow Issues activity labels shrink and truncate so long ages and comment counts cannot clip the trailing Issue number, including 120px/160px sidebar widths.
- Apple Light/Dark unselected Graph WIP hover uses a lighter 15% accent wash over its lane tint instead of opaque accent. Selection retains its 30% wash and counts/diffstat retain their normal colours. (#1497)
- Historical Inspector Path/Tree controls share one keyboard-accessible roving TabList in wide and compact Changes, with explicit mode activation and selected accessibility semantics. Stage all, Unstage all and Discard all keep their semantic captions on neutral hover rather than painting an unmatched opaque accent. (#1495, #1496)
- Document titlebars keep a fixed 36-logical-pixel height at every UI zoom, with capped tab-label typography; native macOS traffic lights retain their centred (9,9) placement. The second-row toolbar continues scaling normally. (#1500)
  - Review: titlebar » keeps a capped 24px hit height at high zoom; Editor bottom-panel fitting subtracts the fixed logical titlebar separately from scaled status/toolbar chrome.
- Compact Changes now follows Graph selection: historical commits show the existing Inspector metadata on the left and changed files with Path/Tree controls on the right; file selection opens the central diff. WIP retains Unstaged/Staged columns. The entire compact bottom panel defaults to one-third of the logical window height and remains divider-resizable without writing compact heights to settings; widening restores the preferred desktop height and right Inspector. (#1484)
  - Compact metadata scrolls as one viewport so the unchanged Inspector actions remain reachable at high zoom. Native Graph pointer-focus coverage now opens the actual compact navigator filter, retains desktop filter coverage at 1040px and 1392px, and counts copy receipts within the bounded log's local observation window.
  - Review: compact Changes uses the same Compare-before-Inspector source and title as desktop, so Compare file clicks open the matching comparison diff. Widening restores the desktop tab and open/closed state suspended on compact entry. The original selection-only content behavior is superseded by #1528's explicit-user-selection activation.
- Custom-theme native reload coverage identifies fresh success/error toast IDs instead of requiring identical success labels to accumulate beyond the intentional four-card cap. Each winning reload still reports its result and rejected files; superseded reads still produce no feedback. Production notification policy is unchanged. (#1487)
- Apple Light/Dark Graph commit and WIP row selection overlays a translucent 30% accent wash on their normal stripe/lane tint, retaining ordinary text, badges and ref-chip colours. Inactive selection stays grey; other presets retain their existing selected fill. Sidebar, file, PR, menu and primary-button opaque accent presentation is unchanged. (#1485)
- Diff selections belong to their pane owner and content revision: replacing same-path/same-size content, closing a pane or changing repositories retires old selections, while syntax/theme highlighting preserves them. Copy reads only the focused diff's valid selection. Split cells retain the same localized unsafe-Unicode badge/help as Unified, and LF/CRLF-only replacements show localized per-side line-ending annotations without changing code-only copy or raw hunk approvals. (#1444, #1445, #1446)
- Editor ⌘S remains registered while another writer owns the repository and visibly refuses with `OpInProgress`, rather than silently doing nothing. Native Save menu items remain disabled while busy; hidden/historical buffers and ordinary active modals remain protected. Panic recovery retries only after acknowledging and dismissing its notice.
- Editor bottom-panel sizing reserves the compound header/tab/snapshot/source minimum after titlebar and toolbar chrome, keeping historical code readable at small-window/high-zoom sizes even when a larger desktop panel height is retained. Fitting preserves the preferred panel size. PR lane selection coverage scrolls virtualized context rows into view, and diff reflow coverage accounts for the hunk header when checking an oversized first code row.
- Editor pane fitting and divider coordinates use the actual workspace body rather than the full window, excluding Compact's icon rail. Long selected tabs stay fully revealed at small-window/high-zoom sizes without shrinking tab/close targets or replacing preferred side-pane widths.
- Graph windows below 1000 logical px replace the inline navigator with a scaled 44px icon rail. Its existing Local branches, Remote, Worktrees, Tags, Stashes, PRs and Issues sections and filters open in anchored Popovers; selection, Escape and outside click dismiss them. Toolbar overflow, graph geometry and the wide layout remain unchanged. (#1225, rail slice)
  - Compact Changes uses the original Commit Panel's Unstaged and Staged state in equally sized, independently scrolling columns. Each header exposes List/Tree and Stage all/Unstage all; selecting Changes and resizing retain the same draft and file selection. Operation Log, Terminal and Activity are unchanged. (#1225, Changes slice)
  - WIP hover reveals Commit…; clicking it or pressing ⌘⏎ opens a pinned Popover using the existing Summary/body, Template, AI, co-author and Commit controls. Pointer leave and outside click keep it open; Escape, ×, successful commit or scrolling the WIP row out close it without dropping an unsent draft. Placement prefers below, flips above and clamps to the window. (#1225, composer slice)
  - Compact file selection is kept in the original Commit Panel state. A pinned composer moves to the wide right pane and back even after Graph selection closes the desktop-only visibility flag, retaining both input entities, draft text, selected file and real Git staging. Retired Popovers cannot clear a newly reopened session's pin. (#1225, shared-state slice)
  - Review: rail SVGs explicitly carry their primary-label colour instead of remaining invisible; native pixels check all seven icons. Commit… sits inline before the WIP diffstat, with non-intersection bounds. Wide-first shrinking pins the same draft owner, absent-anchor ⌘⏎ leaves focus/tabs alone, visible Changes attaches on tab changes, co-author Escape closes only its picker, and wide Operation Log keeps its status toggle in sync. Restored exact-1000px desktop/high-zoom coverage and moved presentation sizing out of workspace.rs. (#1225 review)
  - Feature-branch integration reuses the updated Commit Panel keyboard/AX Buttons, List/Tree TabLists and co-author ListBox in compact surfaces. Escape preserves modal/menu and diff/PR priority, then dismisses notifications before the rail Popover and pinned composer; shared draft/focus ownership remains unchanged. (#1225, #1251 integration)
  - Warned/blocked compact Commit plans hand keyboard focus to the existing modal root before suspending the footer; Amend uses the shared alert-dialog focus trap. Escape cancels the modal and returns the same draft/input owner; another Escape closes the composer without a Git write. Co-author focus reconciles only in its actual render owner, preserving End navigation to virtualized tail candidates. (#1225, keyboard integration)
  - Compact empty-column staging actions retain keyboard focus and stay inert with localized no-changes reasons; pointer completion returns focus to diff navigation. Wide disappearing-action fallback is unchanged. Shared native bulk traversal verifies both compact column headers and the restored file-row Stage Button's native focus ring instead of skipping its existing Tab stop. (#1225, keyboard integration)
  - Development-branch sync retains the shared UI-core TabList and all compact geometry cells. Geometry coverage runs in four viewport/zoom partitions, each keeping EN/JA × Apple Light/Dark and the unchanged 120s native deadline. (#1225 integration)
  - Development sync preserves responsive Graph-column and wide-pane budgets, accounting for the compact rail without persisting fitted widths. The shared Changes lists retain the updated roving file navigation and accessibility semantics; compact AI controls receive the same generation snapshot as the desktop panel. (#1225, #1317, #1355 integration)
  - Rail Escape retains Popover dismissal from its focused filter instead of applying the desktop filter-clear fallback. Native traversal covers the new roving file-row stop between its Stage Button and List/Tree selector, and Amend suspension checks the existing alert focus owner rather than an undrawn composer input. (#1225 development integration)
  - Approved development cutover retains the separate document titlebar, Finder-density 22px file slots, on-accent selection foregrounds and English Git terms in Japanese. Compact Changes and the pinned composer share the updated list/action/composer helpers, including draft-save failure feedback and minimum composer height. (#1225, #1465, #1467, #1468, #1472–#1474 integration)
- Remaining Finder-inspired lists share 13-reference-pixel typography: sidebar branch/remote/tag/stash/worktree identities, File History rows and Editor file/directory tree rows. File History and Editor use 20-reference-pixel rows, 16px badge columns and descender-safe 1.2em leading. Sidebar group headings and 20px ref rows stay unchanged; worktree rows retain their 24px path/port wrapping contract. Inspector and Commit Panel retain their already-adopted sizing. (#1467)
- Apple active row selections now paint the opaque chrome accent with Finder-style white labels (automatic black fallback for light accents); inactive selection remains grey with normal text. Selected row glyphs, badges and secondary labels switch foreground together, and accent-filled primary buttons use the same contrast policy. Segmented controls, modes, filter chips and tabs instead retain a subtle neutral selected fill and ordinary foregrounds; other theme identities, semantic chip fills and selection/input state remain unchanged. (#1468)
  - Review: one selected/key/Apple guard limits foreground replacement, preserving ordinary semantic and muted colors elsewhere. PR/Issue sidebar card children and translucent WIP chips follow the guard; enabled menu hover and keyboard highlights now pair accent fill with main/secondary labels. Drag-over remains neutral, toolbar hover differs from current mode, and Home/Conflict pane toggles retain segmented-control styling.
- Japanese UI keeps Git terms in English throughout menus, tooltips, accessibility names, plan cards and notifications. Stash/Branch/Pop retain their operation names; `working tree` describes files/index state and `worktree` identifies a checkout. A CI invariant prevents translated Git terms from returning; English strings and repository-owned text are unchanged. (#1465)
  - Review: conflict notifications and PR reasons also use `conflict`; already-English operation/sidebar labels keep their existing case and number. The CI gate distinguishes ordinary compounds such as margin, Stage Manager, pull-down and Remote Desktop from Git terms.
  - Native Japanese Branch/Commit headings retain English Git vocabulary; the invariant scans every application/crate source and recognizes JA bodies after an English early return. Ordinary File/Edit/View headings remain Japanese. (#1501)
- Commit paging's native viewport check uses fresh rendered selection-row bounds rather than accumulating nominal zoom-scaled heights, preserving full-row Home/End/Page reveal and wheel-scroll assertions at fractional zoom. Production selection and scrolling are unchanged. (#1458)
- Commit consumption invalidates pending Smart generation so an old result cannot replace the next draft. Draft autosave failures keep edits unsaved, show English/Japanese failure feedback and offer Save Draft after storage recovers; saved logs are success-only. Smart replacements preserve standard Input Undo/Redo and earlier handwritten edits, including template comments. (#1434, #1435, #1436)
  - Review: full Input replacement has an isolated Undo group even immediately after typing; discarded generation panels release their busy state and generating status only when their dispatch token still matches, never settling a newer panel's request. Pending consumption cannot retain an empty panel during reload.
- Selected PR dashboard rows retain active/inactive selection fill on hover. Hidden clones keep a source-named running status and keyboard-reachable Show Progress action without starting another clone. Home's successful empty repository sections name their owner and host instead of claiming the whole account has no repositories; retrieval failures remain distinct. (#1451, #1452, #1453)
  - Review: the user's empty section uses a natural host-only sentence instead of interpolating the Yours/自分 heading; organization names remain explicit with idiomatic Japanese wording.
- Restore always-visible repository/Home document tabs in a separate titlebar above the unchanged 52/60-reference-px toolbar, with trailing + when opening a document is available; only blank spacers handle window drag and double-click. Owner decision supersedes unified single-document chrome. (#1462, supersedes #1179)
  - Review: narrow-to-wide resizing retires tab overflow and restores natural widths without reopening documents; main-window native lights return to (9,9), while Settings retains (9,16). Home-only windows omit inert +/×, and chrome hairline coverage follows the restored row order.
  - Exact-fit review: each titlebar reservation follows GPUI's device-pixel rounding; the measured natural item span has no extra pixel/ceiling, so contents that fit their budget do not spuriously show ».
  - Compact-list integration retains a full scaled 22px file slot (20px logical minimum) plus focus/border allowance in each Commit Panel viewport, alongside the restored titlebar's composer minimum; native desktop height remains 408px and small-window composer/actions stay contained. (#1463, #1467)
- Repository restoration retains the active repository identity after excluded SSH tabs or failed opens, falling back to the first surviving local tab only when that identity is gone. Cmd-W closes the current repository/Home tab, or the guarded window on document-free Home. Keyboard Shortcuts keeps its title and dismiss action visible while its independent body scrolls by wheel and keyboard in English/Japanese at small-window and high-zoom sizes. (#1441, #1442, #1443)
  - Review: a retained Home tab becomes document-free window content when the last repository closes, so one subsequent Cmd-W closes the guarded host rather than silently deleting the invisible Home descriptor. Covered-menu regression now opens help through its production command and verifies scroll-body focus; obsolete no-tabs-disabled translations are removed.
- Commit Panel Staged/Unstaged file lists use Finder-inspired 22-reference-pixel slots (at least 20 logical pixels) and shared 13-reference-pixel file typography in both List and Tree. Directory labels are vertically centered instead of leaving uneven space below them, section-header padding is reduced, and Stage/Unstage retains its minimum target and existing keyboard/accessibility behavior. (#1463, #1467)
  - Review: Stage/Unstage draws its keyboard-focus border inside the unchanged click target so adjacent dense rows cannot cover its lower edge. Shared diffstat typography also applies to Inspector.
- Inspector changed-file rows use the sidebar's dense 20-reference-pixel height with Finder-inspired 13px labels and 16px status-badge columns, including directories, Generated and overflow notes. A shared UI-core file-tree font token replaces the initially requested extra-small text. Reduced top padding and metadata/Comparing spacing bring files upward without changing title line height, keyboard reveal, accessibility names or preferred pane widths. (#1464, Inspector portion of #1467)
  - Review: reuse the sidebar row-height constant and use 1.2em filename leading so Inter descenders remain inside truncation's content mask; native glyph containment now covers `package-000.json`.
- All five Graph sidebar panes collapse from their roving keyboard row with Left, returning focus to the header; Enter/Space now toggle empty headers like pointer clicks without changing saved weights. Filtered group disclosures explain in English/Japanese why matching groups stay expanded and never silently change the normal collapse choices. (#1449, #1450)
  - Review: outline Left collapses an expanded group first, otherwise reveals its parent; only top-level rows collapse the pane. Worktree inspection-card controls are excluded and retain their card/focus.
- Native menus follow the English/Japanese display language, including standard Edit items. Window's AppKit role remains independent of its translated title. Edit Undo/Redo route to focused text input or the same available repository-history plan as their shortcuts, never from Terminal or historical snapshots. File Save exposes the existing ⌘S action only for a saveable dirty live Editor buffer. (#1438, #1439, #1440)
  - Review: Japanese Undo uses macOS-standard 「取り消す」; history target labels remain stable during modal, busy and Home availability changes, avoiding unnecessary native-menu rebuilds.
- Restore `filter_menu_over_modal` coverage to the existing covered-dismissal policy: an arriving modal dismisses the filter menu, and Escape cancels the modal without writing. (#1457)
- Clicking or right-clicking a Graph commit returns keyboard ownership to the Graph; arrows and Copy no longer stay in the previous filter, sidebar row or toolbar control, and Escape from the context menu returns to the Graph. Graph Copy now uses the same visible-center/front-layer guard as commit navigation, so takeovers and modals cannot copy a hidden commit; foreground TextView and diff selections retain their copy paths. (#1447, #1448)
- Relative ages follow the EN/JA UI language, including fully spoken accessibility units, without reloading repository models. Branch Cleanup merge dates now share Detail's local, per-instant DST-aware calendar; stored timestamps, staleness and deletion eligibility are unchanged. (#1454, #1455)
  - Review: Japanese Graph ages use short unit forms that fit the fixed column; visible and AX ages share one reference-clock read per list render.
- Terminal plain clipboard paste now requires explicit approval for ASCII control bytes (except Tab) even without a newline. English/Japanese warnings name invisible control characters; Cancel sends zero bytes, while approved contents, ordinary Japanese text and clipboard Tab are delivered exactly once. Tab does not replace the newline warning. Bracketed-paste filtering and raw completion keys remain unchanged. (#1437)
- Analyze Reset asks before discarding an unsaved draft and writing defaults; Cancel/Return preserve both draft and disk. Save is disabled for unchanged patterns, with localized saved/unsaved status. Settings render important help under controls as secondary text while retaining long-form tooltips and switch accessibility descriptions. (#1404, #1415, #1416)
  - Review: Reset approval is owned by the Settings form, preserving the main window's existing/pending plan and remaining visible in the portable overlay. Escape cancels only the alert; switch help is announced once rather than duplicated by its Group.
- Clone folder-picker answers belong to the initiating card and repository visit; dismissed, replaced and reopened cards reject late answers. Hidden/background clones no longer switch repositories on completion: the existing notification offers an explicit Open Repository action, while a user waiting on the current card retains automatic opening. Native picker errors leave the card unchanged and show bounded feedback (with durable detail owned by the repository screen present at launch, never Home's background tab); cancellation stays silent. (#1402, #1405; #1420 Clone side)
  - Review: persistent Open Repository feedback stops the expiry ticker and, like errors, is retained ahead of routine notifications when the toast cap is exceeded.
- Operation Log restore cards expose named, read-only ref and graph-preview lists with single-stop keyboard inspection, arrow/Home/End/PageUp/PageDown reveal and system scrollbars. Long warnings/blockers share a named keyboard-scrollable body; focused list rows also reveal their parent position. Copy all and the two-stage destructive approval/execution path are unchanged. (#1382)
  - Review: dedicated body scroll actions outrank global navigation; parent reveal runs only for a changed focused row, preserving later pointer scrolling. Ref/preview wheel events stay within panes that can move in that direction, but fitting panes and outward wheel input at their boundaries scroll the surrounding safety prose.
- PR dashboard rows now share stable-identity keyboard navigation and offscreen reveal; Enter/Space open the focused PR. Shared filter/sort/Clear/Refresh chips and dashboard All PRs/Refresh are named, focus-visible keyboard buttons in English and Japanese. (#1408, #1413, #1414)
- Open Repository's native picker approval button follows the EN/JA UI language. Picker failures produce a bounded error toast and durable Operation Log detail for an active repository (a process diagnostic on Home); cancellation stays silent. (#1419, #1420)
  - Review: Home foreground never writes a receipt against retained background tabs, and a pending native picker retains its launch repository context across tab switches.
- PR swimlane commits share retained keyboard row navigation and complete subject/SHA/PR accessibility names. Selected commits retain the shared active/inactive selection fill, including on hover; inert context rows remain read-only. Headings use an authoritative PR commit total when supplied, otherwise explicitly say how many commits are loaded in English and Japanese. (#1409, #1410, #1412)
- Branch drag-over highlights only distinct resolved destinations. Self and remote-target alias drops show a prohibited cursor and do nothing instead of opening a merge plan or reporting a late refusal; sidebar rows and graph chips share the same read-only identity rule as merge-into planning. (#1421)
  - Review: drag predicates reuse the session's local-tip map and snapshot-built remote-name set for allocation-free constant-time lookups. Unknown destinations retain their existing planning path, including the detached HEAD chip's explanatory blocker.
- Terminal clipboard paste respects the shell's bracketed-paste mode. Without it, multi-line text requires explicit approval and Cancel sends nothing. Control-Shift-Tab / Command-Shift-J return to the bottom-panel tabs without disrupting shell completion, and hiding Terminal returns focus to the visible graph while retaining its PTY. (#1403, #1406, #1407)
  - Review: bracketed payloads remove ESC and ETX without changing CR/LF, preventing an embedded end marker from escaping paste protection. A real PTY respawn regression confirms approval for the exited shell cannot reach its replacement; Terminal's accessibility name uses the localized toolbar label.
- macOS Reduce Motion is read at startup and refreshed on display-accessibility notifications; system or app opt-in skips decorative animations. Reduce Transparency selects an opaque equivalent for dark PR tiles without changing their normal appearance. (#1417, #1418; refs #354)
- Stash Apply, Pop and Drop confirmations show the approved entry's sanitized message beside its index, with middle ellipsis and full tooltip/accessibility text. Existing full-OID/order preflight guards reject a replaced stash even when stack depth is unchanged. Apply also shows its plan warnings outside the recovery disclosure. (#1384, #652)
  - Review: Apply warnings reuse the shared `Note` row and visible ⚠ cue; a missing stash index displays only the index rather than inventing a repeated message.
- Create Worktree ignores late folder-picker answers after its requesting form is replaced or its repository tab is left, including departure and return. The surviving form still plans `<chosen parent>/<branch>` exactly once; cancelling or choosing a folder creates no directory or branch. (#1383)
- Branch and Tag creation plans block existing names and parent/child ref namespace collisions, including packed refs, before confirmation. Existing invalid-name validation remains in place. (#1379)
- Set Upstream blocks nonexistent remote-tracking branches and explains in English/Japanese that they must be fetched before setting upstream, rather than offering confirmation for an operation that cannot execute. (#1380)
- Merge, Cherry-pick and Revert confirmation cards show the computed files to change with status glyphs, a localized count and bounded preview. Long paths use middle ellipsis with their full sanitized path in tooltips/accessibility names; the shared scrolling body keeps actions pinned at small-window/high-zoom sizes. Plan computations and confirmation policy are unchanged. (#1381)
  - Review: omit empty/uncomputed previews on Merge-into and blocked plans; reaching Merge's existing 200-entry producer cap uses a lower-bound `200+` heading and nonnumeric overflow. Reuse Changes' status badges and explicitly allowlist the 25-row preview as a #454 exception.
- Restore native cherry-pick abort-sequence coverage: valid plans still require Tab-focused confirmation, while blocked plans assert their disabled reason and exercise the guarded application boundary for refusal receipts without expecting an inert button in the Tab order. Production focus and destructive Return behavior are unchanged. (#1398)
- Create Branch shows existing ruleset and Checkout After warnings before approval, with localized Note names and the shared warning icon/style inside the scrolling plan body. Create Worktree uses the same warning list. Advisory warnings keep Create enabled; blockers and the pinned footer retain their existing behavior. (#1385)
- Inspector changed files are a stable-path keyboard list even before a diff is open: arrows/Home/End reveal rows, Enter/Space open the exact file, and the Generated disclosure expands from the keyboard. Localized accessibility names retain status and complete paths. (#1375)
  - Review: opening/closing Generated splices only its children, retaining the disclosure's scroll position and focus. Reaching the final row also reveals the unfocusable overflow notice, including after its rows have been measured.
- File History remembers its focused entry when visiting header actions. Shift+Tab from Back and Tab from Follow Renames reveal and return to that row; list arrows/Home/End update the existing entry and diff without a pointer. (#1376)
- Inline Editor blame maps the live buffer back to HEAD using the existing domain diff: inserted and modified rows show a muted EN/JA Uncommitted label, while unchanged rows retain their original attribution after insertions or deletions. Blame and HEAD text still come through the backend read path. (#1374)
  - Review: line mapping runs off-thread with revision-tagged results only while Blame is visible. Edited regions exceeding one million diff cells remain conservatively Uncommitted; unchanged trimmed edges keep attribution. LF/CRLF checkout differences do not erase authors.
- Inline blame retains complete sanitized author, relative date and commit subject in its tooltip and accessibility name; middle ellipsis affects layout only. (#1378)
- Split diff keyboard navigation follows aligned visual rows rather than revisiting the top of replacement blocks; Home/End and viewport-sized PageUp/PageDown use the same order. Unified navigation and inclusive selection/copy semantics remain unchanged. (#1373)
  - Review: retain monotonic source ranges when a right-column caret crosses left-only fillers, count measured visible Row bounds for wrapped/expanded pages, and reuse the pane's captured split projection.
  - Second review: Up reverses the same affinity-side replacement route as Down, preserving one-sided additions/deletions; page counting visits only visible list items.
- Status-bar chips expose localized explanatory tooltips and accessibility names, including ahead/behind counts, unavailable counts, upstream identity and full fetch ages. Stale fetches add a visible `!` cue rather than relying on warning colour alone; informational chips remain outside the Tab order and existing rounded-up text fitting is retained. (#1377)
  - Review: sanitize upstream identity once per footer render rather than once per chip; the fresh-fetch regression uses a midpoint within its minute to avoid a wall-clock boundary flake.
- Bottom-panel divider resizing starts from the pressed body height, including the rendered tab strip without a start jump; scaled pointer deltas clamp without drift and the final height is saved through the settings store on release. The existing divider has no focus handle, so its pointer-only keyboard scope is unchanged. (#1372)
  - Review: temporary viewport/zoom compression fits only the presented height, preserving the larger preference for restoration. A capped divider starts from its actual painted height, and returning to the press origin retains the original preference.
- The main repository window remembers its display-relative position, size and display UUID across launches. Reachable saved frames restore on the matching display, fitting its usable area; disconnected displays or inaccessible titlebars fall back to the main display centre. Drag/resize saves coalesce at rest, secondary windows remain independent, and `KAGI_NO_RESTORE=1` skips geometry persistence. (#1367)
- Remote SSH Connect/Browse and repository Trust confirmations cap their cards to the viewport and keep action buttons pinned while long diagnostics scroll. Long directory/repository identities use middle ellipsis with their full values in tooltips and accessibility labels; directory keyboard ownership and Trust's Return policy are unchanged. (#1368, #1369)
- Clean repositories without a first commit show an icon, localized “No commits yet” guidance and a primary Changes button opening the Commit Panel. Dirty unborn repositories retain their WIP row; opening guidance performs no Git write. (#1371)
  - Review: read-only SSH remote views retain their empty list rather than offering an unavailable local Changes action.
  - Visual review: the shared Graph/Terminal empty-state block centers its vertical icon/title/hint/action stack in the pane, with the Button below the hint rather than at the banner's trailing edge.
- Escape dismisses actionable operation notices instead of immediately re-presenting them on every render, and restores focus to the opener. Dismissal discards any armed release approval but keeps the reconcile requirement, write restrictions and durable Operation Log detail; a new explicit write refusal can offer Inspect again. Unread queued notices and notices displaced by another dialog remain available. (#1366)
  - Review: queued Checkout/Commit/Merge requests restore the blocking Inspect notice when their session is already waiting for reconciliation, without duplicating an active or queued notice for the same requirement or recreating it during queue ticks.
- Empty-query command palettes separate Actions, Appearance and Language with localized headings and named accessibility groups. Headings are not keyboard or activation targets; fuzzy-search ranking, disabled reasons, single-row highlighting and the native shortcut column are unchanged. (#1370)
  - Review: returning to the first result restores Actions fully; upward reveal of a section's first row includes its heading. Japanese section/availability descriptions use localized sentence punctuation.
- Repository document tabs shrink to a zoom-scaled minimum with middle ellipsis, then scroll horizontally; active and keyboard-focused tabs are revealed. A named trailing » menu lists every open repository and Home using the existing popup-menu keyboard/focus contract. Close × buttons expose localized target-specific accessibility names without changing Cmd+W or dirty-close guards. (#1339, #1340)
  - Review: distinguish same-name repositories with full-path descriptions on close controls and chooser items, retain cloud/worktree markers with duplicate-name parent qualifiers, localize Home’s close target, and leave vertical wheel input outside horizontal tab scrolling.
  - Visual review: retain natural tab widths and the previous two-tab appearance when there is room; only a constrained strip shrinks labels and exposes ». Native Apple Light / Japanese / 167% pixel pins protect the baseline repository/Home positions.
  - Integration: repository/Home close controls remain enabled when using the shared button-description helper's explicit accessibility-disabled state.
- Restore neutral repository-tab pixel coverage by comparing the gap with the strip’s chrome-surface token rather than the older panel fill; selected/inactive tab contrast assertions remain intact. (#1339; stale oracle after the #1295 surface ladder)
- Find/Replace controls in the Editor, conflict Result editor and Issue composer expose localized Button names and hover help, with case/replacement toggled state. Keyboard traversal reaches the actions after the field; Escape and Close return focus to the same editable buffer. Find Buttons consume inherited editing actions, and clicking bar padding focuses its query rather than allowing hidden code/draft edits. Query, undo history and guarded Save remain owned by the existing inputs. (#1348)
- Settings' theme picker exposes a localized ComboBox name, current theme value and open/closed state, with named options. UI zoom controls announce localized decrease/increase actions with the current percentage; the value is a named Status and bound buttons are disabled. (#1349, #1350)
  - Review: theme semantics belong to the actual focused Select trigger, not an unfocused wrapper. Reaching a zoom bound keeps keyboard focus on the opposite stepper, and the next Tab continues beside the zoom row; Japanese announcements use the visible UI ズーム term.
- Sidebar and Inspector / Compare widths fit the window and zoom without replacing preferred divider widths: the Inspector shrinks first, then the sidebar, reserving a scaled 320px centre whenever their presentation minima fit. Capped drags preserve preferences and widening restores the original geometry; diff-header controls remain inside the narrow/high-zoom centre. (#1355)
  - Review: worktree keyboard cards, group identity reveals and Branch Cleanup column drag origins follow the fitted sidebar edge rather than its larger preferred width.
- Draft PR dashboard rows retain full-contrast titles and readable number/author/age metadata in Apple Light and Dark. The existing localized Draft state uses `text_muted` rather than fading the whole interactive row; hover and open behavior remain unchanged. (#1352)
  - Review: mute STATE only when its attention reason is Draft; conflicting, changes-requested and other actionable draft states retain their attention colour rather than losing the Needs You signal.
- Smart Commit footer tooltips and accessibility descriptions name the selected Ollama model, Claude Code or Codex in EN/JA, including provider-specific generation wording; CLI generation no longer claims to use a local LLM. (#1342)
  - Review: freeze dispatch provider/model in the initiating session until completion, so settings changes cannot rename an in-flight generation. Rule-based suggestion omits a redundant AX description; loading text and provider display names each have one source.
- Issues rows and Home's recent repositories use stable-keyed, single-stop keyboard lists with arrow/Home/End navigation, focused-row reveal and Enter/Space activation. Localized row names retain complete Issue and repository identities; removed or hidden rows release focus. (#1346, #1347)
- Branch picker candidates share the existing roving list keyboard and focus-visible ring inside a bounded scrolling viewport. Checkout/Delete/Rename retain their frozen exact branch names and existing approval paths; Escape returns focus to the opener and repository departure discards the picker. (#1351)
  - Review: Rename keeps the new-name input's initial focus instead of scheduling a root-focus override; typing works immediately after Enter/Space selection.
- Changes file rows use stable path-and-section keyboard focus, one roving stop per section, arrows/Home/End and Enter/Space diff selection. Shift+F10/Menu opens the same file-action menu as right-click; keyboard menu navigation preserves the frozen owner/path and Discard's plan/confirmation with root Return cancelling. All three menu items fit inside the viewport, including at 167% zoom. (#1333–#1335)
  - Both section scrollers reserve one complete row before the composer shrinks; high-zoom keyboard focus no longer lands in a shorter-than-row viewport. Tree metadata and flat accessibility names share the localized status/full-path label.
  - Review: virtual-list measurement no longer replaces a remembered non-first-row Tab stop; expanded Generated/Agent artifact files retain TreeItem roles. File names are lazily cached by status/layout revision and language, and only the row containing focus (including its Stage/Unstage button) measures the section's reusable context-menu anchor, keeping menu placement current after resize or scrolling.
  - Only the remembered row's Stage/Unstage button participates in Tab, preserving its keyboard action without preceding rows intercepting Shift+Tab out of the section. A surviving focused path is revealed when refresh reorders it.
- Diff headers retain the full sanitized path in their tooltip and accessibility description without enlarging the title or displacing header controls. (#1336)
- Toast feedback exposes a stable Status node named by the newest arrival; animation, hover and timer paints do not replace its arrival identity. The pinned GPUI API has no live-region/priority setter, so automatic VoiceOver delivery is not claimed. (#1337)
- Running Clone exposes an operation-named EN/JA Status label with elapsed seconds; Hide remains non-cancelling and the existing completion/error feedback is unchanged. (#1341)
- Worktree inspection Refresh and local URL actions are named keyboard Buttons; Tab reaches them without closing the card, and Escape returns to the owning worktree row. (#1338)
  - Review: pointer-only hover-card actions stay outside native Tab traversal; only explicitly keyboard-opened cards register action stops. The local URL chip retains its original extra-small text sizing.
- Japanese Rebase confirmation correctly names the source branch (`{} を rebase`), not the destination. (#1343)
- Rebase and unarmed Replay confirmations bound their branch-bearing label to a zoom-scaled 160px middle-ellipsis region while retaining complete source/destination identities in the plan, tooltip and accessibility description. Cancel and confirmation stay inside the card; Replay's armed warning remains fully visible without ellipsis and its two-stage safety gate is unchanged. (#1344)
- Push plans retain complete sanitized commit subjects up to the shared 4 KiB UTF-8 display bound; only row layout ellipsizes. Accessibility labels and viewport-wrapped tooltips reveal complete subjects, while Copy all and Copy commits retain the full model subjects for checked-out and named-branch push plans. (#1345)
- Conflict resolver adoption controls are named EN/JA Buttons with keyboard-only focus rings. One roving hunk list uses Up/Down between hunks and Left/Right between side, order and line controls; Enter/Space share pointer activation. Selection remains buffer-only and the conflict FSM and Save boundary are unchanged. (#1311)
  - Review: rebuild roving ownership when a retained path's hunk/line structure changes, including consecutive operation steps. AX names use sanitized operation-dependent roles/identities; order controls announce selected output order, never adoption.
- Graph columns compress before commit subjects at narrow/high-zoom widths: subjects retain a 120-reference-pixel minimum, authors shrink with middle ellipsis and full-name tooltips, and compact times remain visible inside the row. Header, commit, WIP and stash columns share fitted widths without overwriting saved divider preferences; wide-window positions stay unchanged. (#1317)
  - Review: expanding an already capped divider no longer replaces its larger saved preference with a compressed width. Below the combined column/metadata minima, the subject floor is limited by remaining room instead of displacing author/time; native coverage includes compressed drags and a 1440px-wide window with a 360px Inspector at 167% zoom.
- Every gpui-component Input exposes its localized EN/JA name on the actual focused TextInput through the fork's `Input::aria_label` API. Naming-only Group wrappers are removed; genuine container semantics remain. This supersedes #1309's ancestor-name workaround without adding a second TextInput or changing input behavior. (#1331)
  - Review: search/replace fields also name their focusable Inputs directly; empty or name-identical placeholders omit redundant accessibility announcements. Stash messages and each PR field filter have dedicated localized names without render-time string composition.
- Sidebar filtering is a real localized Input from the first frame, reachable by Tab or Cmd+F without a preliminary click. Escape clears the filter and returns focus to the local list; the clear affordance and existing window-owned state remain intact. (#1309)
  - Review: retain one TextInput role on the actual input; language changes update the retained field's placeholder before drawing. The initial named-Group workaround is superseded by #1331's direct focused-input naming.
- Busy status-bar feedback keeps its unchanged spinner beside a single-line truncated label, centered within the fixed-height footer instead of stacking vertically. Branch and feedback minimum-width fitting remain unchanged in EN/JA at 100%/167% zoom. (#1321)
  - Review: reserve spinner and gap width in addition to Busy's preferred text budget, capped by available room; the text fills its remaining slot and the native minimum-width oracle measures that child directly.
- Settings provider and saved-model chips expose named selected tabs with one roving Tab stop, arrow navigation and Enter/Space activation; unavailable CLI providers remain inert and explain why. Provider selection does not generate a commit message. (#1298)
- Analyze ignore Save/Reset report OS write failures in EN/JA with bounded toast feedback and a visible unsaved marker. Failed Reset preserves the editor draft; successful writes quietly confirm and clear the marker. The settings owner uses atomic replacement to retain saved bytes on failure. Shared toast dismissal repairs focus independently in Settings and repository windows, preserving pane arrows and Cmd+W; closing Settings releases its toast focus metadata without dismissing shared feedback. (#1299)
- Main windows publish the active repository and branch as their native title, returning to `Kagi` on Home. View menus offer localized Show/Hide Sidebar and Enter/Exit Full Screen labels; Window adds previous/next repository-tab navigation and Bring All to Front. About uses the single named dialog slot with a Close button, trapped focus and selectable version/copyright text. (#1312–#1315)
  - Review: match macOS's normalized brace-key tab shortcuts, preserve native window stacking and the invoking frontmost window when bringing main windows forward, and retain menu ownership while the application is inactive.
- Restore native Settings live-apply coverage by comparing the repository toolbar's blank drag-region pixels with the chrome token rather than the distinct Apple Dark panel fill; theme changes still repaint both repository windows in EN/JA at 100%/167% zoom. (#1323)
- Long status-bar branch names shrink to a zoom-scaled readable minimum and end ellipsis, with the complete name retained in the tooltip and accessibility label. Status chips and refresh time keep their natural widths; feedback truncates before the nonshrinking Terminal / Operation Log cluster in both toolbar presentations. (#1310)
  - Review: short branch names keep their measured natural widths, nonempty feedback reserves up to 80 reference pixels before branch identity within the actual remaining footer budget, and labels/tooltips/AX share the existing control-byte sanitizer without changing raw refs or klog.
  - Pixel-rounding review: round each natural text width up like GPUI layout, preserving unellipsized short branch names and the right footer padding even with every status chip at 167% zoom.
- File History's Open File opens the selected entry's exact path in the Editor workspace and closes History only after the worktree file is available. Binary or missing paths show the existing EN/JA Editor reason and keep History open; followed pre-rename paths never silently fall back to the current name. Existing Editor tabs and dirty buffers are retained, and Back remains dismiss-only. (#1308)
  - Review: supported raster images pass the same allocation-free magic-byte recognition used by the Editor, rather than being refused as generic binary files. Missing pre-rename paths explain the rename and name the current path in EN/JA without opening it automatically.
- Clone confirmation cards use the shared viewport-capped modal frame with one scrolling body and a pinned Cancel/Clone/Hide footer, keeping long plans reachable at high zoom. (#1296)
- Clone shows the resolved destination, with middle truncation, full-path tooltip/accessibility text, localized Change and Copy Location actions, and the same destination in its plan summary. The system picker and approved clone request remain unchanged. (#1297)
  - Review: the scrolling body reserves a fixed 3px focus-band inset, so Change and Copy Location retain all four edges of the pinned Button's outside ring at both zoom levels. Native pixel coverage compares focused/unfocused edges in EN/JA and Apple Light/Dark.
- Sidebar stash accessibility names include the visible `stash@{N}: message` identity while focus remains tied to the stash OID. (#1303)
- Unloaded tag targets show a muted row and localized accessibility/tooltip explanation; click and Enter report the reason and request the existing read-only history page only while more history is available. Loaded targets hidden by Branch Solo retain the filter and show a localized hint instead of paging. (#1304)
- Remote/prefix group headers reserve separate disclosure and count columns, ellipsize only the name, and reveal the complete identity through bounded tooltips, Shift+F10/Menu keyboard cards and accessibility metadata. Escape closes the card without moving row focus; leaving the row or sidebar retires it. (#1305)
- Graph commit rows retain complete sanitized subjects for accessibility and hover tooltips; only their one-line layout ellipsizes. Tooltips constrain their width and wrap long subjects inside the viewport. An explicit 4 KiB UTF-8-safe display bound replaces the old 72-character cut. (#1306)
- Inspector retains and copies the complete sanitized commit message, including late co-authors and trailers. Metadata is prepared once per commit, and the existing independent message scroller keeps long bodies and trailer tables reachable without displacing changed files or the footer. (#1307)
- Activity period selection uses named EN/JA keyboard tabs with one Tab stop, arrow/Home/End navigation and Enter/Space activation; the selected fill remains visible on hover in active and inactive windows. (#1301)
- Activity chart buckets expose localized UTC dates, commits, merges and distinct-author counts, with arrow-key inspection sharing the existing instant hover readout. Solid commit and dashed merge strokes use the same encoding in the legend, including equal/zero series and single buckets. (#1302)
  - Review: merge dash gaps knock out underlying commit/baseline strokes with the Activity body's explicit panel fill, preserving a luminance-visible pattern at equal/zero values without contrasting gap rectangles. A single merge bucket paints two genuine dashes below its separate commit mark; native gap pixels match nearby unpainted background, and probes count actual painted dash/knockout segments.
- Operation Log summaries are named EN/JA disclosure buttons with expanded state, one roving list Tab stop and arrow/Home/End navigation. Enter/Space opens or closes the focused entry; the named Copy button copies its complete entry by identity even when new receipts shift the list. Existing variable row heights and long-detail selection limits are unchanged. (#1300)
  - Review: receipt insertion/trimming splices the variable-height list instead of resetting its reading anchor, preserving a scrolled entry’s summary/Copy focus. At the very top, new receipts stay visible even after wheel/keyboard navigation. Quiet Copy reuses the existing accent focus-visible control pattern and becomes fully opaque on hover or keyboard focus.
- Narrow toolbars move Update, Settings and inactive workspace modes into » before Git commands. Pull/Push remain visible longest among Git actions, with the selected mode collapsing last; widening restores the reverse order. Existing overflow menu order, accessibility names/roles, shortcuts and hidden-mode keyboard/palette routes are unchanged. (#1316)
  - Tiny-window review: icon-only fitting includes the controls' fixed focus borders, the workspace group retains its measured minimum, and repository identity explicitly shrinks to its readable minimum. Native group probes no longer insert a test-only layout wrapper; 700×500 long-name coverage checks outer control/group nonintersection.
- Alert-style confirmations, including the Editor dirty guard, Delete, Discard, Abort, Amend and Trust, reuse the named-dialog focus trap so Tab and Shift+Tab stay inside the dialog instead of reaching the Editor's Code Input. Root Return and Escape cancellation, explicit danger approval and modal accessors are unchanged. (#1292)
  - Review: alert dismissal restores the still-drawn opener (or root fallback), with an independent screen-scoped return target. Escape/Return resumes Editor typing, and Delete cancellation preserves root shortcut dispatch without stealing a newer modal's focus.
  - Review: retain the actual alert trap across reload's modal-focus reset; when a queued non-input notice replaces an alert, return keyboard ownership to the root instead of leaving focus on the unmounted card. Named input frames retain their own focus, with exhaustive modal classification requiring an explicit ownership decision for new variants.
- Apple Dark uses one lighter elevated fill for palettes, dialogs, menus, popovers, toasts and tooltips, with a neutral identifying boundary. Window chrome sits above panels and content, with literal 1px separators; readable secondary text, line numbers, scrollbar thumbs and the yellow accent retain at least 4.5:1 contrast. Apple Light and other/custom themes retain their existing fills. (#1274)
  - Review: elevated-card separators and outlined chips remain visible, Remote Browse selection/hover uses a distinct neutral, and Trust Repo, the co-author menu and full-hash tooltip share the floating fill. Generic component borders keep their existing stronger value; the Settings 1px oracle distinguishes its separator from the fill outside both RGB tolerance bands.
- Restore native cross-worktree merge coverage after the destructive-dialog Return policy changed: root Return cancels the dirty-editor guard and preserves its buffer; explicitly focused Discard closes the editor before graph dragging resumes. Worktree navigation still retains unsaved editor state, and merge safety/receipt assertions are unchanged. (#1290)
- Analyze Ownership displays author names, falling back to the email local part for unnamed authors; full identities remain available through tooltips and accessibility labels. Email-based aggregation, stable ranking and EN/JA captions are unchanged. (#1052)
  - Review: untrusted author names and emails escape display-control bytes before reaching visible labels, tooltips or accessibility names; raw aggregation/export identities remain unchanged.
- Worktree creation from a branch context menu preserves the branch name in its text Input and derives the suggested path from that name, in EN/JA. (#1045)
- Conflict image previews use the same zoom-scaled geometry for columns, boxes, gaps and captions; equal-width previews shrink to fit the pane while preserving image aspect ratio. Current/Incoming headers and size/dimension captions support EN/JA. (#1269)
  - Review: image boxes prefer a square aspect and only shrink; binary previews retain an external comparison button immediately below their captions. A view-owned scroll handle preserves scrolling across redraws, and native image coverage requires positive bounded height and zero horizontal/vertical extent.
- Terminal not-started, exited and failed states now show an icon, localized EN/JA title and concise hint with a keyboard-reachable Primary Restart button. Exit status comes from the owned shell's wait result; failed starts retain the OS reason and the existing Operation Log receipt. (#1273)
  - Reopening Terminal through its tab, menu, toolbar, status bar or conflict action preserves an exited/failed explanation instead of silently respawning. Failed starts still auto-open Operation Log; returning to Terminal shows Failed without another attempt, and Restart alone retries.
  - Review: wait errors also show Failed with the complete reason and Restart, while retaining Unknown process provenance and never offering automatic unlock or claiming a proven exit. JA uses the existing `shell` spelling and localized `（終了コード N）`.
- Review hunks retain their last twelve lines with an EN/JA Show previous lines / Collapse button, preserve full-hunk copying, and expose long lines through a horizontal system scrollbar without replacing the virtualized PR feed. Copy is outside the code, and vertical wheels only move the feed. (#1258)
  - Integrates PR detail accessibility: the header Copy icon retains its localized name, explicit Tab stop, 24-reference-pixel target and selection-safe mouse-down handling.
- Issue composer body heights scale with the text line height, preserving the ordinary visible-line budget at 100% and 167% zoom. Focused composition fits the remaining pane around nonshrinking fields/footer with a two-line minimum and an internally scrolling Input. (#1261)
  - Focused composition keeps its actual pane as the scroll viewport, without an auto-height content wrapper; flow-neutral probes check the window viewport and a long draft's usable height.
- Analyze mode, period, view and export selectors expose selected keyboard tabs; Copy, Help and Close are named buttons, and coupling disclosures report expanded state and support Enter/Space. (#1265)
  - Review: Hotspots and Coupling keep their original dense row height and content inset; a separate neutral separator remains visible before and after keyboard focus. Analyze Help follows the typed modal getter/setter/clear convention.
- Hotspots shows numeric risk alongside relative bars and explains the 0–1 scale; Map tooltips and accessibility text include path, risk, LOC and commit count. (#1266)
- Analyze explicitly announces truncated Map/List coverage in EN/JA while retaining the 150-tile/300-row rendering bounds. (#1267)
- Analyze Help uses the application's single named modal slot, traps keyboard focus, closes on Escape and restores its opener. (#1268)
- Restore native conflict/stash regression coverage after keyboard-policy changes: conflict selection targets the actual Next button instead of the window's last Tab stop (#1222), while stash Pop/Drop uses explicitly focused confirmation and verifies Return cancellation (#1277 / #1270). Selection pixels, stash OID ownership, duplicate suppression and close/reopen lifetime checks remain intact. (#1283)
- PR detail Overview, Review, Files and Commits use one named keyboard TabList, including Conflicts when available. Arrow/Home/End move focus; Enter/Space activate the existing view path without replacing the composer or its draft. (#1255)
- PR description, Merge and GitHub header actions expose localized names and keyboard activation. Description exposes its toggle state; conflicting PRs retain access to the guarded merge approval rather than looking disabled. Accepted plans take modal-root focus; Escape closes without reactivating the opener. Draft and held merges remain unavailable. (#1256)
- Review Copy hunk and Apply suggestion use named keyboard-reachable Buttons with larger hit targets. Full-hunk copying and mouse-down selection suppression remain; Apply still opens the existing destructive plan and never writes before confirmation. Plans take modal-root focus so Return cancels without the opener's key-up reopening them. (#1257)
- PR and Issues create/reply composer preview and focus icons expose explicit EN/JA accessibility names and state. Preview retains its toggle name while pressed reflects preview mode; the tooltip offers the next edit/preview action. Focus icons name the action to expand or restore the editor. Tooltips are no longer the only names; drafts and editor ownership are unchanged. (#1259)
- Ordinary comment and Issue submission uses the primary accent instead of warning styling. Disabled submit stays neutral and inert; review Request changes keeps its warning meaning. (#1260)
- Editor History snapshots are genuinely immutable, selectable monospace text rather than an editable buffer whose changes disappear. An EN/JA read-only header identifies the historical path and revision; Open current returns to the unchanged editable working-tree buffer. No restore or Git write operation is added. (#1264)
  - In the foreground Editor, historical-pane state blocks Save and repository Undo/Redo even while root or header controls have focus; the unsaved live buffer remains dirty and untouched. A retained Editor behind PRs/Issues/takeovers does not suppress those surfaces' shortcuts. LF snapshots use virtualized 32-line blocks; highlighting is disabled above 128 KiB or 2,048 lines. CRLF snapshots use one block to preserve clipboard line endings, sacrificing large-file warm-layout bounds for this correctness fallback.
  - Known presentation trade-off: the selectable reader currently lacks the former line-number gutter and horizontal no-wrap scrolling. [#1275](https://github.com/TomiXRM/kagi/issues/1275) tracks restoring both with a line-number column sharing the same virtualized scroll owner.
- Editor buffer tabs retain a zoom-scaled minimum width and scroll horizontally instead of collapsing. Duplicate file names show the shortest distinguishing parent path; every tab exposes its complete path through the pinned tooltip and accessibility name. Active-file selection keeps the tab visible, and dirty-close guards remain unchanged. (#1262)
- Editor Diff/History and Diff/Snapshot selectors reuse the existing single-stop keyboard tab-list owner with named selected Tab segments. Left/Right/Home/End select, Enter/Space activate, and keyboard selection retains selector focus instead of jumping to the root. (#1263)
- Destructive dialogs make Cancel the Return default; danger confirmation requires explicit keyboard focus or a pointer/AX action. Escape still cancels, non-destructive Return confirmation and existing two-stage safety gates remain unchanged. (#1270)
- Command Palette has one highlighted activation target: real pointer motion selects its row, while scrolling beneath a resting pointer leaves the keyboard selection unchanged. Enter runs that same row; unavailable commands remain inert. (#1271)
- macOS Command Palette shortcuts use the shared gpui-kit formatter's native ⌃⌥⇧⌘ glyph order in the right-aligned column, including remapped and named keys. EN/JA share bindings and glyphs; other platforms retain text shortcuts. (#1272)
  - Review: multi-stroke remaps format each chord stroke independently; malformed display strings remain visible verbatim rather than panicking. Native remap fixtures are confined to their isolated KEEP_GOING child.
- Icon and Text toolbar fitting measures captions in both groups and repository identity, preserves the workspace group's natural width, and moves trailing actions into » before controls intersect. Icon Only and the saved display preference remain unchanged. (#1252)
  - Review: measured side-column minimums keep wide-window actions centered and narrow workspace controls nonshrinking. Native coverage executes hidden workspace, Analyze and Settings entries in addition to repository commands.
- Branch context menus keep at most ten top-level command/submenu rows, with frequent actions first and less common commands in Integrate, Advanced and Worktree submenus. All prior commands and safety guards remain; disabled rows stay keyboard-focusable but inert, with EN/JA reasons in tooltips and accessibility descriptions instead of inline paragraphs. Right/Enter opens a submenu; Left/Escape closes one level. (#1238)
  - Review: pointer cascades wait 200 ms and cancel pending parent changes when entering a child; closing or replacing a keyboard-focused child restores launcher focus. Protected Reset and dirty/conflicted Checkout warnings remain visible, and ADR-0050's no-op label rule is explicitly amended.
- Commit Panel List/Tree is now one named keyboard tab group: arrows and Home/End move the selection, and Enter/Space activate without losing the selected file, draft or scroll state. Pointer selection preserves the window's diff-file arrow navigation. (#1229)
- Commit Panel Template, Generate, co-author and Amend actions use named EN/JA ghost Buttons in visual keyboard order. Bulk staging actions are keyboard reachable, disabled actions expose their reason without executing, and Template exposes its pressed state. Hidden bulk controls return keyboard focus to the selected view cell and pointer focus to the diff-navigation root; distinct paint/control identities preserve expanded pointer targets. (#1230)
- The co-author picker supports arrow/Home/End selection, scrolls the active candidate into view, exposes named listbox options and a system scrollbar, and returns focus to its opener after insertion or Escape/Tab/Shift+Tab dismissal. (#1231)
- Diff headers show Unified and Split as explicit selected segments, retaining icon-only collapse in narrow panes. (#1235)
- Binary diffs explain the absence of a text diff and the external-editor comparison action in EN/JA; image diffs name added, modified and deleted states instead of leaving a missing side unexplained. Binary conflicts reuse the same explanation. (#1236)
  - Review: image state follows blob existence rather than preview support. Modified files with only one supported image encoding retain both Before/After sides and explicitly mark the unavailable preview; native EN/JA coverage includes deleted images and binary conflicts.
- Diff hunk actions use named EN/JA keyboard-reachable ghost Buttons and tooltips; Enter/Space preserve the displayed hunk approval and existing staging pipeline. The dense header geometry is retained. (#1233)
- Focused diff panes support caret arrows, Shift-arrow range extension, Shift-Cmd-arrow start/end extension and Cmd+A; the caret scrolls into view, Cmd+C still copies code only, and Escape clears selection before notifications. Mouse selection remains unchanged. (#1234)
  - Review: the body is a real Tab stop with a layout-neutral, keyboard-only accent focus ring; focus identity follows the title across row-count reloads, while selection remains content-keyed. Header caret paint is measured after styling. Existing std Mutex poison recovery avoids adding a dependency.
- Inspector author identities retain their full names with layout ellipsis and full-name/email hover and accessibility text. The compact monospace SHA chip is a named EN/JA ghost Button; Tab, Enter and Space use the existing full-SHA clipboard action. (#1237)
  - Review: XSmall preserves the original SHA typography inside its dense hover/focus frame. Incomplete author/co-author identities omit empty or duplicated email brackets; native paint coverage asserts layout ellipsis and SHA label containment at both zooms.
- macOS Settings opens or raises one separate non-modal 715-point window with related EN/JA panes, live preferences, restored pane/position, compact unified chrome and standard close/minimize controls. Cmd-W closes only Settings; Escape leaves the window open (text editors keep their own Escape behavior). Other platforms retain the existing overlay. (#1198, ADR-0223)
  - Review: Settings holds weak repository/callback handles and closes when its owner is released, without retaining fetch/watch/terminal state. Dock reopen restores a repository even when only a utility window remains; the last content height prevents repeated frame drift on reopen. Native coverage includes owner release, stable geometry, Dock restoration and unwind-safe host selection.
  - Quit review: bounds changes persist height as well as position, so quitting the app while Settings remains open restores the same frame. Native geometry coverage dispatches repository Cmd-Q through an explicit quit seam and recreates roots without running Settings-close persistence.
  - Persistence review: position and content height share one `settings_window_frame` string so bounds bursts coalesce instead of synchronously saving alternating keys. The former position/height pair migrates once and is removed; isolated real-store tests require at most two commits for 100 bounds changes.
- Commit Panel headings, List/Tree selectors and conflict badges now follow EN/JA language changes. The feature name Commit Panel and Stage/Unstage/Discard operation-button names remain English per ADR-0048, with consistent capitalization. Named view/batch controls expose Button roles and selectors expose their selected state; headers wrap rather than clipping longer labels, with at least 20 logical-pixel hit height. File paths, diffstats and user-owned commit Template text remain untouched. (#1227)
- Error notifications now remain until explicitly dismissed, wrap to three lines, and preserve errors before routine notifications when the four-card stack fills. Routine notifications pause their four-second timer on hover. Named EN/JA ghost close Buttons and Escape provide keyboard dismissal without overriding modal/menu cancellation. (#1240)
  - Review: cap eviction always admits the new notification; stored previews are bounded to three lines/768 characters. Removal repairs orphaned keyboard focus; diff selection and PR drawers retain Escape priority. Native coverage uses production Button geometry, routine hover timing, keyboard activation and overflow inputs.
- Apple Dark readable secondary labels, counts, captions, timestamps and Commit Panel section headers now reuse `#98989f`, meeting 4.5:1 WCAG contrast on both panel and content backgrounds. Apple Light is unchanged; disabled controls retain their existing opacity behavior. (#1239)
  - Review: Dark input/composer boundaries use quiet `#67676a` (≥3:1) rather than readable secondary text; generic borders derive from this neutral at 85% alpha. Scrollbar hover/drag and active line numbers remain distinct when secondary text roles coincide; Apple Light paint mappings remain unchanged.
- Commit Panel selected file rows retain their window-aware selection fill while hovered; conflicted rows retain their diagnostic tint. (#1228)
- The commit body composer uses the Summary input's theme focus border around the complete body/footer frame without changing its auto-grow layout. (#1232)
- Non-link Commit Panel actions and file menus, shared menus, hunk actions, Home Recent/GitHub/Work rows and toolbar controls request the native arrow cursor; real URL-link controls retain their hand cursor. (#1241)
- Settings groups related preferences into Appearance, Graph, Network, Behaviour / Terminal, Language, Smart Commit and Analyze cards on the panel background. Rows share a base-colour surface and neutral hairlines; concise EN/JA descriptions retain extended avatar, theme and locking guidance through help. The existing window host, switch wrappers and single page scroll owner are unchanged. (#1200)
  - Review: hairlines separate rows from the top without tracing the rounded card bottoms; custom-theme rows follow the same rule. Analyze retains internal vertical padding/gaps and CLI warnings retain breathing room. Native coverage hovers avatar/theme/Analyze help and compares the full EN/JA explanations; prose keeps lowercase commit/local branch vocabulary. (#1200 review)
- Historical, superseded by #1462: #1179 combined native lights and toolbar controls in one frame and hid single-document repository/Home tabs below it; the frame owned +. Its 52/60-reference-px heights remain the restored second-row toolbar heights, not the titlebar height.
  - Retained: only real blank spacers own drag/double-click gestures; Home shares workspace chrome, and Home-only windows without a remote repository hide the inert +. Superseded: hiding TabList/TabPanel and releasing focus merely because two documents become one.
  - Historical viewport measurement: #1179 recorded a 440.5-logical-px desktop Commit Panel file list at 1392×883@100%; #1462 measures 408px after restoring the snapped 32.5px titlebar. Composer and primary-action heights remain unchanged.
  - Home containment coverage checks fixed chrome and the first Recent repository row against the window, not an overdrawn GitHub row below the scrollable list's fold; compact row/action containment and name-priority assertions remain. (#1179, #1212)
- Home caps its centered content column to the remaining window width, narrows Recent when the shared PR-reader minimum cannot fit, wraps folder/SSH actions in order, and ellipsizes pane labels before clipping. Compact repository rows reserve at least 40% for the name while keeping the trailing Clone/Open action visible; wide layout geometry is retained. (#1212)
- Home narrow-layout review: the shared compact threshold uses the same scaled horizontal padding as the column and its actual sidebar width; repository dates ellipsize before pushing Clone out of a narrower row, and Recent exposes full paths through tooltips and accessibility descriptions. (#1212 review)
- Status-bar Terminal and Operation Log controls expose localized names, show/hide tooltips, toggle state and individual keyboard Tab stops with focus-visible rings. Enter/Space reuse their existing callbacks; compact 16px glyphs and neutral hover affordances remain inside the footer. (#1207)
  - Review: Cmd+J appears only on Terminal's active Hide tooltip; Show does not imply the panel shortcut selects or starts Terminal. Native coverage checks adjacent forward/reverse focus stops and pointer activation's PTY focus.
- Toolbar overflow reserves its first-line shortcut column, bounds secondary unavailable reasons to the label column and caps the menu to the viewport. Japanese unavailable reasons no longer mix English prose, and the » tooltip is suppressed while its menu is open. (#1217)
- Home repository descriptions remain single-line but expose the full sanitized text on hover and in the row's accessibility description. Clone/Open now use a bordered small Secondary push button, distinct from quiet metadata badges; focused-row Enter/Space remains the single keyboard action path. (#1203, #1204)
- Home-row native regression observations are cleared per presentation cell and tooltip text is consumed after each hover; the description's accessibility assignment and recorder share one helper path. (#1203, #1204 review)
- Bottom-panel Operation Log / Terminal / Activity use the shared single-stop keyboard tab list with manual activation, localized EN/JA names and distinct focus rings. Rounded pane-selector cells retain their window-aware selected fill under hover; arrow focus alone never starts Terminal. Cmd+J and lazy terminal startup remain unchanged. (#1205, #1206)
- Bottom-selector review: keyboard Terminal activation keeps the selector focus for further arrow navigation, while a pointer click gives text input to the PTY. The native scenario restores in-memory theme/language/zoom as well as persisted settings. (#1205 review)
- Dialog Cancel uses the shared bordered Secondary push button at the same S size as the trailing default; destructive dialog actions use the filled red role. Input cards expose named modal Dialog boundaries with available Confirm/Cancel actions and reuse the existing focus trap, preserving localized EN/JA titles, blocked reasons, Escape and IME routing. Existing plan/alert boundaries remain unnested. (#1208, #1209)
- Dialog review corrections: filled destructive buttons derive readable labels and distinct hover/pressed fills in every theme; Clone Escape is handled on its actual task focus owner and restores window focus; Remote Browse exposes its available Connect/Open Repository accessibility action while busy tasks expose only Cancel. (#1208, #1209 review)
- Narrow/high-zoom toolbars move commands that cannot fit into a named, keyboard-accessible » menu rather than clipping them. Both display modes retain localized names, shortcuts, unavailable reasons and the same action paths; workspace controls collapse after repository actions, with the selected mode retained longest. (#1210)
  - Overflow stays open during layout remeasurement, refreshes command reasons, preserves action-assigned Terminal focus, releases deferred menu state when the trigger disappears, and removes separators whose neighbouring groups have overflowed.
- Settings section headings use neutral secondary-label 12-reference-px semibold text; the frame, title bar and section separators use literal 1px `surface` boundaries instead of selection tint. Selected gpui-kit radios paint a centered contrasting dot rather than a checkbox tick, retaining the existing radio-group keyboard and accessibility behavior. The dot uses the bridge's primary foreground (dark on yellow), and production measurement placeholders stay out of flow so titlebar alignment is unchanged. (#1199, #1201)
- Settings and Home Repositories / PRs / Issues now expose draggable vertical scrollbars on their existing scroll owners. Inspector message/files and Activity ranking gain the same affordance: automatic bars overlay content, while non-auto-hide macOS policies reserve a 16px lane. Scroll offsets, virtualized Home rows, wheel and keyboard navigation remain intact. (#1202)
- Scrollbar review: Home Recent also has a draggable thumb; Inspector/Activity offsets are repository-session-owned, reopening Settings resets to the first controls, and native test guards restore in-memory zoom/language/theme plus the kit palette on unwind. (#1202)
- Home's recent-repositories sidebar heading shares the navigator's 12px semibold secondary typography and 8px vertical padding, protecting descenders while truncating long localized text horizontally with an ellipsis and full-text tooltip instead of hard clipping. Sidebar width and recent-row behavior stay unchanged. (#1197)
- Toolbar display can be changed in Appearance or View → Toolbar → Icon Only / Icon and Text. Apple Light and Apple Dark follow Finder's default with a compact 36-reference-px icon-only row; other presets retain the 52px caption row. Names, tooltips, counts, unavailable reasons, keyboard rings and workspace feedback remain. (#1178)
- Test-only: migrated native repository-tab PR ownership scenarios to ADR-0200 retention, covering predicates, opened panes, unsent drafts, refreshed head evidence and rejected departed-request publication without changing production behavior. (#1194)
- Apple Light/Dark repository and Home tabs use neutral content/surface fills in key and non-key windows, with readable primary labels instead of full blue selection and no permanent top accent rail. Dark inactive cells remain distinct from strip gaps. Other presets retain their active rail and selection palette where fill contrast alone is insufficient. All themes lose full worktree tint and its stored lane rank (the 🌲 marker stays); inactive close controls appear on hover/focus, active closes remain visible, and Cmd+W/manual keyboard activation are unchanged. (#1180)
- macOS の Apple Light／Dark は Settings → Appearance に System／Theme accent（JA「システム」／「テーマのアクセント」）と検出した Mac の色名を表示します。未指定の Apple Light は System、Apple Dark は従来の Yellow `#ffd600` の Theme accent を既定にし、Dark で Mac の色を使う場合だけ System を選びます。`accent_source: system|theme` に保存し、旧 `accent: blue` は Theme、旧 `accent: system` は System へ移行して旧 key を削除します。起動／window activation 時の read-only CFPreferences は整数／文字列の Mac 設定を検出し、System の chrome・Primary・focus ring・text selection と Light の row wash へ反映します。全テーマで小さい accent 文字／filled label の contrast は4.5:1以上へ補正しますが、Git ref／lane／status palette・body link・Dark の無彩色 selection は保持します。(#1181)
- Conflict file rows retain their window-aware selected fill under the pointer and expose `aria_selected` in Conflicted / Resolved Files. Next / Previous conflict keep the selected file and content editor in sync, clearing stale editor state for binary previews. Resolving a draft keeps that file selected in Resolved Files for review until the user navigates to the next unresolved file. (#1187)
- Graph navigator の五つの section heading を EN の Title Case／JA の既存訳、secondary label の12px semibold、小さい chevron と無彩色の `(N)` pill に揃えました。28px の pinned header は上下8px、virtualized body の20／24px slot は維持し、collapse・keyboard focus・AX expanded state・保存先は変更しません。(#1177)
- Graph navigator header の chevron は SVG 自身に secondary 色を設定し、通常 build の空の計測 probe で余分な gap が入らない margin spacing にしました。count pill は header hover と別の neutral fill を使い、hover 中も輪郭を保ちます。(#1177 review)
- Pane resize targets keep their existing scaled 4px geometry, but resting boundaries paint centered 1 logical-pixel `surface` hairlines over the adjacent theme background: Graph sidebar sections, sidebar/content, commit-panel/inspector boundary, bottom panel, inspector message/files split, PR navigator and swimlane, Conflict A/B and Result, Editor tree and hunks, and File History detail and rows/diff. Hover/drag paints a solid accent across the target and line; Apple Dark keeps its own palette. (#1174)
- 非 key window の Graph commit／WIP（accent bar も減衰）・conflict file・repository/Home tab・toolbar workspace mode・sidebar・CommitPanel・Inspector・PR commit/file list・bottom panel tab・Home/filter/mode 選択を `inactive_selected` で弱調表示します。Apple Light は hover と区別できる中立 `#d3d3d3`、Apple Dark は既存の `#3a3a3c`。全 preset の primary label は4.5:1以上で、selection ID・geometry・Git の意味色を保持します。(#1176)
- PR-table horizontal scrollbars preserve macOS auto-hide: `Scrolling` overlays without a lane; non-auto-hide settings (gpui-kit `Hover` or `Always`) reserve 16px and use a persistent table scrollbar so its hitbox cannot steal last-row clicks. Header/row X alignment, independent Y scrolling and narrow JA high-zoom reachability are preserved. (#1182)
- Apple Light's shared `colors.input` boundary uses quieter neutral `#8e8e8e` (3.276:1 on white; 3.058:1 on panel grey): Input, unchecked Checkbox, Default-outline buttons (modal Secondary and inspector/oplog/conflict buttons), Radio, Select, Combobox, NumberInput and DatePicker. Existing outline geometry, focus rings, labels and Apple Dark remain unchanged. (#1183)
- Toolbar group hairlines, PR feed/table dividers and the command-palette frame now use neutral `surface`, not selection blue or label grey. Selection, focus rings and readable markdown-table borders remain unchanged. (#1175)
- Pinky Boo and One Light `surface` colours retain at least a 1.15:1 luminance step against panel/content backgrounds, keeping structural separators visible; Apple and Catppuccin palettes remain unchanged. (#1175 review)
- PR dashboard→detail→native resize／zoom→Escape／Home で、次 frame の focus cleanup が一覧の InputState を強参照して session の破棄後も保持する問題を修正しました。公開 gpui-kit fork の callback を弱参照にし、既存の session-owned input・draft・thread・scroll と wrapped row count API を維持します。combined lifecycle の native 回帰と leak assertion を復元しました。(#1172)
- Commit Panel の幅を viewport と graph の最小表示幅に合わせて cap し、狭幅・高 zoom でも List／Tree、Stage／Unstage、Undo を window 内に保ちます。file path は省略表示し、完全な path を tooltip／AX に残します。1392×883・100% の幅と既存の composer 高さは維持します。(#1171)
- Commit Panel は低い window／高 zoom で本文 editor を縮めて内部 scroll し、Template／Generate／co-author／Undo と Commit を表示範囲内に保つようにしました。1392×883・100% の主要領域の高さと soft-wrap による本文拡張を保持し、940×660／560・167% の EN／JA で本文入力と wheel scroll を検証します。cached wrap-row accessor を fork に追加し、3／11行本文と折返し本文も native 回帰で確認します。(#1167)
- PR 詳細は scaled 420px の本文幅を優先し、狭幅・高 zoom では swimlane、必要なら navigator を折り畳みます。toolbar／View menu／palette／⌘B・⌘⌥L から補助 pane を本文内に再表示でき、既存 Sidebar 設定、PR・file 選択、未送信 draft、opened thread、scroll を保持します。dashboard の navigator は維持し、inline に戻った reveal は破棄して Escape を飲み込まないようにしました。Overview／Review／Files・All PRs・composer の native bounds、dashboard toggle、Escape、resize の回帰を追加しました。(#1118)
- Smart Commit の LLM／CLI 生成中・生成成功・利用不可時のルールベース fallback・生成失敗の status を EN／JA に対応しました。生成メッセージの言語とは独立して UI 言語を使い、既存の診断ログと session 所有権は維持します。(#1168)
- 本文リンクの `link` token を ref／Primary の塗り色から分離しました。Apple Light の Issue composer preview／Issue・PR 本文リンクは白地で読みやすい青になり、全組み込みテーマで 4.5:1 以上の contrast を保ちます。既に読める Dark の色、ref chip・Primary palette、下線・選択・リンク操作は維持します。自作テーマでは `link` を継承または明示できます。`extends` のない旧 standalone file で `link` を省略した場合は、その file 自身の `color_branch` を使って従来の配色と読込を維持します。(#1077)
- Conflict の Take Incoming／Keep File の背景色を全テーマの `color_remote` に合わせました。Apple Light の Incoming pane／marker と同じ緑を使い、文字・hover・押下も Kagi の theme bridge で読みやすく導出します。status／破壊的操作の色は変更しません。(#1078)
- Conflict の Keep Current／Take Incoming は色値の一致ではなく操作 role で filled Primary／Info を選ぶようにしました。従来は13個中11個の組み込みテーマで remote=success の一致により Take Incoming／Keep File が tint になり、Color Vision でも remote=blocker／branch=success により Incoming と Keep Current が tint でした。これらを filled Info／Primary に変更し、Stage／警告／破壊的操作の tint と側選択の階層を分離します。既存の色覚対応 palette と安全フローは維持します。(#1079)
- Smart Commit のモデル一覧は共有 modal body／window 高さに追従する list 上限を使い、低い window でも Cancel 行へはみ出さないようにしました。fake Ollama の試験専用同期には追加 dependency を使いません。(#1080 review)
- Smart Commit でローカルモデルが0件の場合、Generate／初回同意の後に既存のルールベース生成を実際に1回実行し、EN／JA で下書きの挿入を報告します。開始した tab 以外には適用せず、以前の LLM 応答による二重適用も防ぎます。(#1082)
- Smart Commit のモデル選択に1つの list Tab stop、矢印／Home／End／Page key と選択先への scroll、focus ring と AX の選択状態を追加しました。Enter／Space だけでモデルを保存して生成を開始でき、Escape・一覧表示だけでは生成しません。(#1080)
- macOS の Kagi メニューに Services、Hide Kagi（⌘H）、Hide Others（⌥⌘H）、Show All を標準順序で追加しました。Services と application visibility は GPUI の OS API に委譲し、Hide は window／編集中の state を破棄しません。Linux／FreeBSD のメニューと Quit の settings flush は変更しません。(#1083)
- macOS visibility commands は command palette／in-app dropdown からも platform API に届くよう修正し、native menu と JA palette は同じ registry label path を使います。(#1083 review)
- Command Palette は狭い／低い window と高 zoom でも検索欄と結果を viewport 内に保ち、結果だけを独立して scroll します。長いテーマ名は省略表示し、完全な名前を tooltip／AX に残して shortcut 欄を保持します。(#1084)
- Trust Repository の承認保存失敗は modal に repository・失敗段階・理由の全文を保持し、コピーと Retry／Cancel を提供します。保存失敗時は untrusted session を維持し、保存後の reopen 失敗でも既存 session を失いません。EN／JA の native 回帰を追加しました。(#1081)
- 外部 filter の拒否は内容を取り込む Stage と index から復元する Discard に限定し、filter 対象の削除 Stage（単体・一括）と untracked file の raw backup→Discard は維持します。無効化した sparse-checkout の残存設定・patterns と別 worktree の patterns は Sync を妨げず、手動 skip-worktree entry と Sync の filter 拒否には専用の EN／JA 案内を表示します。(#1136／#1137 review)
- Sync to remote は HEAD の強制 checkout 前に、cone／non-cone sparse-checkout、未初期化／dirty submodule、変更対象の LFS／外部 filter を plan・preflight で EN／JA の理由付きで拒否します。incoming tree の `.gitattributes` と承認後の設定変更も確認し、backup ref・index・作業ファイルを書き換えません。通常 repo の保全と非 HEAD branch の ref-only Sync は維持します。(#1137)
- Stage（単体・一括・hunk）と Discard は外部 filter 属性を持つ path を書込み前に EN／JA の理由で拒否します。LFS pointer を生の作業ファイルで置換せず、`.gitattributes`／`.git/info/attributes` と `diff=lfs`／`merge=lfs` も確認します。git-lfs や任意の clean／smudge command は実行せず、Git での操作を案内します。HEAD／index の内容だけを使う Unstage は維持します。(#1136)
- Discard の file→symlink 型変更に、`core.ignorecase=false`（Linux の通常設定、macOS で明示設定した場合も含む）では外部 symlink target を切り詰め、index の内容で上書きし得る潜在的なデータ損失の不具合がありました。全 backup ref の保存後、承認済み entry の型が index と異なる場合は symlink 自体を unlink してから復元するよう修正しました。型・link target bytes・regular content を古い stat-cache に依存せず検証し、実 index・未選択ファイル・外部 target・一件の receipt を保持する backend 回帰を追加しました。(PR #1157 Linux CI follow-up)
- Branch rename は exact な repo-local subsection とその include 元だけを移し、複数の `merge` 値の順序・quotes／escapes・`pushRemote`／`rebase`／`description` を保つようにしました。`foo.bar` など別の branch や global／system 設定は変更しません。plan に移動する設定キーを示し、確認後の source／destination 設定の変更は ref 書込み前に拒否して一件の操作記録を残します。(#1129)
- Branch rename の include 元は canonical な Git directory 内に限定し、共有ファイル・作業ファイル・外部へ向く symlink は plan の EN／JA blocker で拒否します。対象 branch 名の `onbranch` include や Git がリネームできない header も ref 書込み前に拒否し、worktree 固有の `config.worktree` は複数値を保って移動します。(#1129 review)
- 未追跡ファイルを除外する Stash Push の After は「N untracked retained」／「未追跡ファイル N 件を保持」を表示し、既存の除外 warning と一致するようにしました。tracked の staged／unstaged 両方を退避し、include-untracked 時だけ未追跡ファイルも取り除く予測を実 Git の status と照合します。ブロック中の計画は変更なしのままです。(#1140)
- Stash Push の保持予測と除外 warning は、index で削除済みの同じ path が未追跡として現れる場合を除くようにしました。Git が HEAD の内容へ戻す path を「未追跡として保持」と表示せず、通常の未追跡ファイルだけを数えます。実行後の検証も同じ path 方針を使い、正しい復元を誤って Partial と判定しません。(#1140 review)
- Stash Push は最初の commit がない repository を計画時に EN／JA の理由でブロックし、After は変更なしを示します。preflight でも HEAD commit を再確認し、実行時の guard と未変更の Refused 操作記録を維持します。(#1141)
- Discard の backup ref は一件の tree entry に mode（120000／100644／100755）と内容を保持し、symlink の target を lossy UTF-8 に変換せず生の bytes で保存します。外部 target を読み書きせず、Git の tree export で dangling／非 UTF-8 target／tracked typechange も元の型へ回復できる backend 回帰を追加しました。既存の blob ref は従来どおり読め、oplog の blob OID・表示契約・一件の receipt は維持します。Kagi 内の file-backup 復元 UI／executor は追加していません。(#1138)
- Discard backup の executable mode は Git と同じ owner-execute bit だけで判定し、`core.fileMode=false` の場合は index の mode を保持します。group-only executable と両方向の mode 差替えを回帰で検証し、既存の linked-worktree native 試験も tree entry の mode・blob・内容を検証する形へ更新しました。(#1138 review)
- Commit／Amend／解決済み MergeCommit と user-visible libgit2 writer は実行と同じ Git CLI で作者・コミッターの identity を確認し、未設定なら user.name／user.email の設定を促す EN／JA の blocker を表示します。承認後も再確認し、checkout 前に identity を確定するため途中変更を残しません。user commit には仮 identity を使わず、内部の `refs/kagi/` snapshot／savepoint／backup だけは専用の application identity で identity 未設定でも復旧情報を保持します。ruleset の作者 email も Git の有効な identity を使い、backend fixture の identity は repo-local に明示します。
- Stage hunk / Unstage hunk は表示時の range と raw patch（context・改行を含む）の内容を承認対象にし、同じ range でも内容が変わった場合は index・HEAD・作業ファイルを書き換えず拒否します。EN／JA の理由を footer / toast に表示し、diff を再読込し、一件の Refused 操作記録を残します。(#1131)
- Unstage hunk は逆向きの diff を再計算せず、表示・照合済みの patch を反転するため、行の入替えも変更のない承認で解除できます。操作記録の HEAD は読みやすい branch 表記に揃え、実行失敗も EN／JA の footer / toast を維持します。file↔symlink の型変更は片側 hunk のボタンを出さず、既存のファイル単位 Stage / Unstage を使います。(#1131 review)
- Hunk 操作の backend 回帰はテストごとの隔離 log directory を使い、外部の環境設定なしでも拒否一件の操作記録を確認できるようにしました。untracked／added／deleted と unborn の whole-file hunk も index だけを Stage / Unstage し、literal 名の隣のファイルや作業ファイルを保持する回帰を追加しました。(#1131 integration)
- Commit／Amend の private index は stat-cache と skip-worktree flag を保持し、sparse なファイルを hook に削除として見せたり、全 tracked file を再 hash したりしないようにしました。empty commit の message-only Amend と HEAD の差分をなくす staged／both Amend を維持し、署名設定は Git の conditional include を含む実際の CLI policy に揃えます。(#1132／#1133 review)
- `commit.gpgsign=true` の Commit／fixup・全 Amend mode・解決済み MergeCommit は、system Git の `gpg.format`／署名鍵設定を使って署名します。鍵が無い場合は HEAD と元 index を進めず、Git の理由を操作記録と EN／JA の失敗表示に残します。成功後も要求された署名 header を確認し、hook による unsigned HEAD への置換を成功として扱いません。使い捨て SSH 鍵による署名と `git verify-commit` の backend 回帰を追加しました。(#1133)
- Commit／fixup・Amend・解決済み merge の Commit は user の Git hook を実行し、`core.hooksPath` を尊重するようにしました。拒否 hook の stderr は操作記録と EN／JA の失敗 footer／toast に残し、HEAD と元 index を保持します。承認時の staged identity を再照合した private index で実行し、完了後に HEAD の tree と parents を検証します。(#1132)
- PushTag は承認した remote 名・すべての push URL・tag object OID と peeled target を固定し、確認後の送信先や tag の差替えを送信前に EN／JA の理由付きで拒否します。annotated tag も承認した object を明示 refspec で送り、複数 push URL と remote 固有の Git 設定を維持します。remote の既存 tag は force せず、各試行を一件だけ記録し、reconcile も remote 名を保持します。(#1135)
- MCP `kagi_commit_show` は graph の 2000 件表示予算から独立して revision を解決します。古い完全 SHA・一意な短縮 SHA・tag を参照でき、曖昧な prefix は候補の完全 SHA と長い prefix の案内を返して拒否します。blob／tree は commit として返しません。(#1134, #1139)
- Unstage は単体・一括とも選択した literal path の index entry だけを HEAD の OID／mode に戻すようにしました。`[]`・`*`・`?`・先頭 `#`・POSIX の `\` を名前として扱い、未選択ファイルの staged 内容や作業ファイルを書き換えません。対象の index 内容が plan 後に変われば拒否し、conflict stage の解除・directory から file への変更も正しく処理します。一括失敗時の cached index は再読込し、途中の変更が次の Stage に混ざらないようにしました。Editor の実ファイルメニューと backend の回帰を追加しました。(#1130)
- 複数 commit の cherry-pick を中止すると、先行する成功済み pick が残っていた問題を修正しました。sequencer が記録した開始 HEAD へ index・作業ファイルとともに復元し、開始点が不明な場合や HEAD が sequencer の安全基準から移動した場合は成功扱いせず EN/JA の理由で拒否します。無関係な手動 commit・未追跡ファイル・tracked file の未 stage 編集を保持し、単一 pick の中止と解決バッファーの退避・操作記録は維持します。(#1127)
- Stage はリンク先ではなく symlink 自体の存在を確認し、リンク切れでも削除ではなく mode 120000 と生の target bytes を stage するようにしました。単体・一括、既存リンクの更新・新規リンク、相対・絶対・非 UTF-8 target の backend 回帰で、選択外の index と作業ファイルの保持を確認します。(#1128)
- Discard は POSIX のファイル名に含まれるバックスラッシュを区切り文字へ変換せず、選択・確認・バックアップ・実行・検証で同じパスを保つようにしました。`a\b.txt` の破棄が隣の `a/b.txt` を上書きする問題を修正し、一括破棄でも両者を別の対象として扱います。Windows の既存 absolute / `./` 入力は解決後の component を Git の区切り文字で再結合し、`core.worktree` で分離した working tree も元の Git directory から復元します。安全に表現できないパスは書込み前に理由を表示して拒否します。(#1125)
- Commit／fixup と staged／both Amend は、承認時の index のパス・blob OID・mode を固定して実行前に照合するようにしました。同じパスの内容差替えや mode／対象ファイルの変更は HEAD・index・作業ファイルを書き換えず拒否し、見直しを促す EN／JA の理由と操作記録を残します。未 stage の編集は commit に混ぜず、変更のない承認は従来どおり実行できます。(#1126)
- Commit の計画中にステージ済みの内容が変わって拒否された場合も、log だけで終わらず、EN／JA の失敗 footer と error toast に見直しの理由を表示するようにしました。以前の承認は解除し、実行は開始しません。解決済み merge の確定 Commit でも、承認後の blob 差替えを拒否して HEAD・MERGE_HEAD・index・refs を保持する backend 回帰を追加しました。(#1126)
- 有効な Primary ボタンの通常・hover・押下の背景色をアクセントから導出し、全組み込みテーマで操作状態を区別できるようにしました。無効状態・focus ring・確認から実行までの安全フローは変更しません。(#1076)
- Remote repository の directory picker を1つの keyboard list にし、矢印・Home / End / Page Up / Page Down で選択と表示位置を移動できるようにしました。Enter / Space は directory と親だけを開き、file は開きません。読み込み中の再実行を防ぎ、失敗・閉じる／開き直す際に古い一覧や focus を受け渡しません。Escape 後は window に focus を戻し、空 directory は親 row を残したまま空の案内を表示します。(#1071)
- Toolbar の件数 badge は固定 9px をやめ、100% で 11.7pt の文字・行高・chip 寸法を UI zoom に揃えました。70%／167% でも 1・99・99+ が切れず、primary button と同じ読みやすい foreground を使います。拡大した chip は矢印を覆わないよう外側へ伸ばし、操作できない button では件数も淡く表示します。(#1067)
- 大きな diff の未計測行を高さ 0 として扱い、scrollbar の範囲が誤ったり末尾に届かなかったりする問題を修正しました。末尾方向への大きなホイール入力 1 回で実際の最終行に到達し、thread の開閉、同じファイルの再クリック、Editor の未変更 reload、幅変更でも読んでいる位置を保ちます。PR の adaptive layout でも native window resize による明確な thread reflow を測り、anchor／extent／末尾到達の回帰検査を維持します。(#1122)
- `gh` が未導入の場合は PR の共有 Open 情報と Closed / All 一覧の読み込みを開始せず、repository の切替や一覧を開くたびに不要な GitHub error を表示・記録しないようにしました。native E2E の注入済み読み込みは `gh` の有無にかかわらず維持します。(PR #1119 review)
- PR 一覧を 1 回最大 100 件の cursor paging にし、表示中の末尾や「続きを読み込む」から次のページへ進めるようにしました。Open の共有情報と選択中の Closed / All を分離したまま、絞り込みで 0 件でも続きを取得でき、追加取得の失敗時は読み込み済みの行と cursor を保持して再試行できます。repository タブを往復しても選択 state・開いている PR・入力中の下書きを保持し、古い読み込み結果を採用しません。同じ head の実際の詳細 payload と Fresh 判定を対応させ、詳細取得は表示範囲と既存の同時実行上限を維持します。PR workspace で Open の追加ページを閲覧中・追加取得中は自動 tick による先頭ページへの置換を延期し、表示中／開いている PR の checks 等は更新します。自動更新では table のスクロール位置を動かさず、先頭ページの更新に失敗した場合は自動更新を再開して回復できるようにしました。保持する一覧は最後に受理した membership であり、新規・Closed・削除の反映には手動 Refresh が必要です。手動更新・Closed / All・workspace 離脱時の通常更新は維持し、table の layout 計測を可視行と誤認せず実際の clipped prepaint から詳細取得範囲を報告します。最終ページでは空の末尾行を残さず、狭幅・167% の画面でも最後の PR を表示し、続きを取得できる場合の loading / retry と横・縦スクロールの契約を維持します。(#1104, #1107)
- PR table／sidebar の Peek が成功しても PR workspace や WIP の Commit Panel に隠れ、比較した変更ファイルが見えない問題を修正しました。既存の Graph／Inspector の Compare を表示し、graph 未取得の PR head でもファイルから main diff を開けます。PR head が graph 未取得の場合は以前の commit 選択を解除し、別 commit の情報や操作ボタンを表示しません。Peek では開いている PR tab と未送信 comment draft を保持し、PRs に戻るとそのまま復元します。dirty Editor は既存の確認を経由し、Cancel・古い tab／editor／input の承認では buffer を捨てません。保持中の Commit Panel と未送信 draft、read failure 時の表示文脈は維持し、fetch・checkout・repository write は行いません。(#1102)
- Create Branch は実行に成功して branch の存在を確認したときだけダイアログを閉じ、次のキーボード操作をウィンドウへ戻すようにしました。失敗した名前とエラー、通常の再読み込み中の未送信入力は保持します。チェックアウトの選択、IME の確定 Enter、1 回の作成につき 1 件の操作記録は維持します。(#1092)
- Pull / Pull ff-only の確認タイトルは upstream の表示名を分割せず、設定済み remote 名をそのまま示すようにしました。`team/origin` を `team` に切らず、local upstream の `.` も正しく表示します。完全な upstream ref / OID・承認照合・fetch / push の既存解決規則は変えません。(#1105)
- Pull 前の remote-tracking refs が古い場合に、未取得の更新があるのに「すでに最新です」と表示していた問題を修正しました。clean / dirty の両方で既存の安全な fetch を完了してから確認または最新の案内を出し、fetch 失敗時は最新と判断しません。fetch 由来の未変更 checkout の確認は watcher の reload で消えず、外部の HEAD / 作業ファイル変更や後から始めた別の操作には古い確認を押し付けません。(#1087)
- Pull が進行中の silent auto-fetch に参加した場合も、fetch 失敗を現在の tab の footer と error toast に一度だけ表示するようにしました。通知は既存の Operation Log の記録・表示 owner に集約し、ユーザーの Pull 要求を持たない background fetch と離れた tab は静かなまま、一つの durable receipt を残します。(#1087)
- Branch の右クリックメニューも、未fetchの `behind=0` だけで Pull / Pull ff-only を無効化したり「最新」と表示したりしないようにしました。busy・detached HEAD・upstream 未設定などの構造的な制約は維持します。(#1087)
- Pull の確認・実行は branch 名・local OID・remote 名・完全な tracking ref を承認済み identity として照合し、確認後の checkout / upstream 差し替えを拒否するようにしました。同じ upstream の新しい commit は取得できます。ローカル branch と追従先の名前が異なる場合も merge / auto-stash 復元の予測と ff-only 計画は設定済み upstream を使い、fetch 後に実際の競合 blocker がある場合は upstream 変更エラーで置き換えず、その blocker を表示して実行を止めます。(#1087)
- dirty Pull の確認後に、同じ commit を指す別の tracking ref / remote へ upstream が差し替えられた場合も、auto-stash **前**に承認済み identity を照合して拒否するようにしました。従来は backend が拒否する前に stash と復元が走り、元の staged 内容が失われる場合がありました。実 UI の独立した二回帰ケースは修正前に両方 FAIL、修正後に両方 PASS：HEAD・index の staged OID / mode・全作業ファイル・refs・既存 stash stack を維持し、stash の実行記録なしで Pull Refused を一件残します。同じ upstream の新しい commit と既存の auto-stash 復元 policy は維持します。(#1087)
- current branch の通常 Pull / Pull ff-only と非 current branch の Pull / Pull ff-only は、対象 branch の実際の upstream remote を fetch してから確認するようにしました。通常 Pull も live HEAD の branch を使って設定済み remote を正確に取得し、別 remote に追従する branch を HEAD の remote と取り違えず、確認まで HEAD・index・working tree を維持します。fetch 後の照合は表示 title ではなく完全な remote identity を使い、reload も upstream の完全な ref 名と namespace で確認を保持します。slash を含む remote 名や local upstream `.` でも、正しい確認を拒否したり reload で消したりしません。quiet fetch は remote-tracking refs だけの更新も `changed=true` なら一件の成功 receipt を残します。ref 移動の詳細は既存の local heads / tags の観測範囲なので空になりうる一方、変更のない fetch は成功 receipt を増やしません。成功 toast は増やしません。upstream 未設定でも manual fetch と新しい visit の Pull は同じ scope へ参加し、失敗理由を現在の footer / error toast へ一度だけ届けます。(#1087、PR #1088 review)
- current branch の Pull ff-only が branch ref だけを進め、index と作業ファイルが旧 commit のまま取り残される問題を修正しました。safe checkout を ref 更新より先に行い、upstream の tracked file の変更・追加・削除を HEAD・index・working tree に揃えます。乖離した履歴は merge せず拒否し、更新が dirty path に触れる場合も変更を保持して拒否します。別 worktree で checkout 中の branch は backend でも拒否し、確認後・fetch 中の占有変更を再検査します。checkout が実際に変わった後で ref 書込みが失敗した場合は、観測した実行後状態を Partial として一度だけ記録し、承認済み typed recovery を Operation Log に表示します。変更のない empty commit 失敗や checkout 前の拒否は Failed のままです。非 current・非占有 branch の ref-only Pull と通常の current Pull / auto-stash は維持します。(#1087、PR #1088 review)
- Stash & Pull の確認後に upstream / remote / local branch identity だけが変わった拒否を、「working tree changed」と説明していた問題を修正しました。承認対象の変更、dirty な stash 対象の変更、復元予測だけの変更を EN/JA で区別し、toast・footer・durable Operation Log に同じ理由を残します。stash 前の拒否と Pull Refused 一件の契約は維持し、同じ upstream の新 commit を identity の変更とは扱いません。(#1099)
- 同じ upstream の cached behind count が Pull の確認後に別の fetch で増えただけで、Stash & Pull が stash 後の preflight に失敗していた問題を修正しました。local Pull の title 比較は操作 variant・branch・remote を厳密に保ち、説明用の count だけを承認対象から除外します。完全な Pull identity と既存の安全条件は維持し、plan の自動書換え・retry は行いません。generic ff-only title の不正確な `ref-only` 表記も EN/JA とも除き、非 current branch の ref-only 実行・計画・recovery は維持します。(#1087、#1099、PR #1088 review)
- PR の右クリックメニューがウィンドウ下端・右端で切れ、Open on GitHub / Copy URL を選べなくなる問題を修正しました。4 項目・拡大率に応じた文字と余白・border の実際の描画サイズで位置を合わせ、popup / modal 層を footer の後に描いて末尾が上書きされないようにしました。Peek / Jump / Open / Copy の操作内容は変更しません。(#1098)
- Editor の file tree は、受理した file 一覧と folder の開閉が変わるときだけ可視行・accessibility の階層／同列位置・keyboard 選択の対応を更新するようにしました。再描画で全 file tree を作り直さず、同じ件数の一覧置換でも新しい file に選択を対応させます。folder の折りたたみで選択 file が隠れても開いている buffer は保持し、表示モード切替や file 一覧の更新後も選択と実際に開く file を対応させます。(#1073)
- PR dashboard は title / branch の最小幅を確保し、狭いウィンドウや UI 拡大時に列見出しが 1 文字ずつ縦に潰れて検索・行へ重なる問題を修正しました。見出しと行を同じ横スクロール領域に置き、state / author / checks / files / age に到達できます。縦の仮想化と表示範囲だけの PR 詳細取得は維持します。(#1095)
- Binary image diff の file / blob bytes と形式判定を、text diff と同じ background read 内で準備するようにしました。cached commit、Compare、staged / unstaged と linked worktree の読み込み先を固定し、閉じた pane、古いタブ滞在・選択・Compare からの結果は表示も row cache も更新しません。linked worktree の変更を解消してタブへ戻ったとき、空の Commit Panel が更新中に消えても、保持中の差分が自身の読み込み結果を受け取り、変更がなければ閉じます。片側だけの画像と未対応 binary の placeholder は維持し、GPUI の pixel decode は変更していません。(#1075)
- PR／Issuesのサイドバーで大量のcardを展開すると、行がwindow内に縦圧縮されてtitleが見えなくなる問題を修正しました。各cardとsection見出しの自然な高さを維持し、親のscrollで末尾まで辿れます。(#1089)
- Command Palette で矢印キー・検索変更による選択行が画面内へ追従するようにしました。マウスでのスクロールは再描画で巻き戻さず、次のキー操作で選択先を再表示します。disabled 理由の可変行高と、表示中の highlight / Enter の実行対象も維持します。(#1069)
- Editor の History 一覧は読み込み済みの履歴を不変 snapshot として保持し、再描画のたびに全 commit の message・body を複製しないようにしました。表示範囲の仮想化、選択 commit の Diff / Snapshot と作業中 buffer の分離は維持します。(#1074)
- PR の header・Review 件数を描くためだけに、description や reviews / comments / line comments 全体を毎回複製しないようにしました。既存 tab の情報を参照し、Commits の要素と Files / Conflicts の差分 snapshot は選択した画面だけで用意します。概要の card も同じ PR と本文を参照し、会話の仮想化・Markdown の内容・選択 / Copy・PR ごとの draft / scroll は維持します。header の GitHub / merge 操作は session と repository + PR 番号を保持してクリック時に対応する PR を取り出します。実 fixture の本文・件数・thread・commit 切替・draft 復元を確認しましたが、速度向上の before / after 計測結果ではありません。(#1108)
- GitHub の PR／Issue 本文は、同じ描画 ID の raw bytes と表示 format が変わらない再描画で、既存の Markdown 準備結果を再利用するようにしました。同じ byte 数の本文置換と、空本文の EN／JA placeholder 切替でも現在の表示をドラッグ・⌘C でコピーでき、古い本文を残しません。現在の style、literal code の文字、画像をリンクとして表示する privacy policy は維持します。実 PR の warm sample では繰り返しの準備 stack を観測しなくなりましたが、CPU／FPS／速度倍率の測定ではありません。この準備処理 slice だけでは会話全体の仮想化・末尾到達・画面外の選択／コピーを解決せず、後続の Issue Thread cutover は下記と ADR-0198 に分けて記録します。(#1091)
- Issue Thread は session-owned の受理済み投稿と単一の可変行高 list を保持し、本文・コメント・巨大本文の末尾・Reply composer を同じ外側 scroll で辿れる構成にしました。managed selection は未描画の投稿も canonical rendered text からコピーし、真の comment ID に結び付いた変更のない選択区間は区間外の reorder / prepend / edit 後も維持します。選択本文の変更・削除と owner / 画面 / modal 離脱は旧 Copy を失効させ、style reflow と source revision を分離します。focused post が unmount されても ⌘A は同じ投稿の全 rendered text を選び、会話全体や別投稿を選ばず、enabled Reply の ⌘A/⌘C 優先権も維持します。詳細をまだ受理していない Issue を選んだまま別画面へ移った場合は、Issues に戻ると同じ owner の詳細を読み直し、読み込み中・失敗の表示を残します。PR 会話の仮想化・広い性能/width比較まで済んだことにはしません。(#1091、ADR-0198)
- Home から Issue を開くときは、保持中の未受理 selection を先に読み直さず、選択先の詳細だけを一度取得するようにしました。同じ番号を再度開く場合も二重取得せず、toolbar から Issues に戻る場合の詳細再取得は維持します。(PR #1117 review)
- Home の絞り込み中に、一覧が不完全または古いにもかかわらず、一致する行が無いと「一致なし」と表示していた問題を修正しました。対象は、読み込みに失敗した一覧・上限で打ち切られた一覧・organization の読み込み中・Refresh の読み込み中(保存済みの一覧を表示している間を含む)・自分の一覧の読み直しに失敗して前回の一覧を表示している場合です。Repositories / Pull Request / Issue のいずれでも、絞り込み中も既存の失敗理由・「さらにあり」/「100 件以上」を残し、前回読めた一致行も表示します。「一致なし」は切り替え先ごとに判定し、他の切り替え先の失敗や打ち切りには影響されません。自分の一覧の読み直しに失敗したときは toast に加えて理由を一覧の先頭に表示し、再び失敗すれば新しい理由に置き換えます。この理由は絞り込みや切り替えでは消さず、次の読み込みが成功するかアカウントが変わるまで残します。「一致なし」は現在の読み込みがすべて完了したときだけ表示し、Refresh 後も絞り込みの文字列を保持します。一致判定・取得の上限・保存・アカウントの扱いは変更しません。native GUI E2E(Tier A)では、最初に追加した二つの回帰シナリオが修正前は両方 FAIL し、修正後は Home 周辺の既存 6 シナリオと合わせて 8 シナリオすべて PASS しました。review で追加した三つの回帰(読み直しの失敗・Repositories の Refresh 中・Pull Request / Issue の検索中)は修正前に三つとも FAIL し、修正後は同じ 6 シナリオと合わせて 9 シナリオすべて PASS しました。最終のソースでも 9 シナリオと 7 つの gate がすべて通過しています。default build の実アプリでも、合成した offline の `gh`(実際の GitHub の障害・SAML・アカウント切り替えではない)で、3 つの切り替え先の失敗・絞り込み・前回の一致行・読み込み中・成功後の回復、Repositories / Issues の打ち切り、切り替え先ごとの判定、理由の置き換え、アプリが保存した一覧からの起動中の絞り込みと読み込み完了後の「一致なし」を確認しました。アカウントが変わったときに理由が消えることはコードの確認のみです。(#1070)

### Changed

- Adopt an official-upstream/public-API-first dependency policy and a staged zero-`[patch]` migration (#1479/#436). Existing pins remain unchanged in this policy-only slice; new fork commits/pins and reintroduced cosmetic patches are prohibited. Record the owner-approved move to official GPUI Kit's `gpui-pre` family and unmodified TextView/default selection scope: cross-post/offscreen conversation selection will no longer be a Kagi guarantee, while naturally supported mounted cross-view behavior is not prohibited. The runtime/oracle cutover remains [#1479-S6](docs/rearch/upstream-migration-1479.md).

- Apple Light のサイドバーと toolbar の明度を同じ Mac の実 Finder ウィンドウと比較して揃え、hover の灰色を中立色にしました。見出し・更新時刻・フォーム label などの補助文字も読みやすくしました。白い本文、Git の状態色・レイアウト・Apple Dark は維持します。(#1065、#1066)

### Internal

- Same-path reopen の GitHub evidence native 回帰は、tab の即時 read に加えて ticker の初回 read も offline `gh` fixture の空ページで応答するようにしました。実 `gh` の認証失敗を旧 session の completion による error と誤認せず、新 owner の read 完了を先に検証します。旧 completion が PR・error・availability・UI domain を変えない assertion と製品の read admission は維持します。

- #1091 の SDK を既存 `TomiXRM/gpui-kit` fork の公開 commit `ae37bfd433781abe44e15edd40867fac1b7b3b2c` に固定しました。UI/assets は同じ Git/full rev、TEMP path/commentを完全除去し、実 metadata は UI0.5.2/assets0.5.1/macros0.5.1を各一つ同じ公開revへ解決します。元b004 API/featuresとGPUI90b3aa0 familyを維持し、latest upgrade・追加 macros patch・alias/path shimなし。新revisionは旧941を正常FFで継ぎ、誤ったlocal-cache pushの新refだけをexact conditional削除したので、cache全体不変とは主張しません。SDK post-format lib334/Clippy新規warningなし/scope3 configured2024fmtがPASS（全SDK workspace baseline fmtはFAILのまま）。
- 画面外 SelectAll の corrected同一GFM/Unicode oracle は public941 Before650でPARTIALのままFAIL、private local After651で元10件を含む全11PASS。旧inline-code fixture647/648は別の歴史的FAILで、padding policy/expected literalをAfterに合わせ直していません。最終public ae37上のRoot全7gateはPASS（557.496848 s、nativeはPASS filtered scenariosでexplicit11countとは別）。default Tier Bは実Issue281の二つの80段落post、full選択→bounded画面外CmdA→戻った元選択とzoom/themeのform/contentを観測しました。partial→full exact bytesはprivate nativeの証拠であり、default Copy/Paste/host clipboard・新width/perf比較・Settings経由の選択維持は未検証です。CI/外部review/mergeと広い#1091は未完了です。
- SDK managed-selection の canonical extraction に置き換わって未使用となった `ParsedDocument::text` / `BlockNode::text` を削除し、custom Markdown plugin の実テキスト期待値は既存 `RenderedDocument` で維持しました。SDK の Input fixture は disabled 境界を明記して元の draft / empty Copy assertion を残し、enabled Reply の SelectAll / Copy・Backspace 清空・empty Copy・Tab による元会話選択の回復は実 AppKit native scenario の責務に分離します。skip・panic 抑制・fake native hook は追加せず、旧 window selection と Unicode / code / image / HTML の coverage を保持します。凍結 source の SDK lib は334 PASS・0 ignored/filtered（body0.06 s/process12.75 s）、2件の新規 dead-code warning も compile 出力から消えました。全 SDK workspace の fmt check は未変更 baseline と対象 code の diff で FAIL、edition/style edition2024の既存設定で変更対象13 Rust fileだけを formatting/checkし、scope は PASS。未変更 `text_view.rs` 行463の空白や他の baseline を巻き込まず、repository 全体の format green とも doctest（SDK manifestで無効）とも主張しません。(#1091)
- herdrでのPM／Agent間の指示・受領・質問・完了報告をmailに統一し、task／exact head／返信先を明示するルールにしました。mail未対応時にterminal promptへ黙ってfallbackしたり共有serverを変更したりせず、受領返信と完了証拠を要求します。明示承認された独立Codex／Solによる代替レビューも、対象head・実行された検証・CIを区別してPRへ記録します。
- Git fixture の isolation 回帰に、repo-relative／`~/` の local include と main／include の重複 key があっても、再初期化後の実 signature／commit と plain Git signing が repo-local 設定を使い、included file・未知 multivar・HOME を保持する検証を追加しました。plain Git consumer にも `maintenance.auto=false`／`gc.auto=0` の 2 key だけを明示し、hostile global identity／signing の oracle を維持したまま detached maintenance と cleanup の競合を防ぎます。include を展開する `git2::Config` による fixture 最適化案は実回帰で失敗したため採用せず、元の hermetic CLI helper をそのまま保持します。最適化案の実 1 FAIL から元の CLI で全 4 test PASS を確認し、PM の自動整形後の corrected source でも全 7 gate、限定 native 2P0F、全 4 fixture test が通過しました（`/tmp/kagi-hig-audit-20261008/git-fixture-include-guards-final-gates/manifest.json`）。過去の prototype の gate／timing は最終受入れや性能改善とは扱わず、subprocess 削減や速度向上を主張しません。製品・trust／write pipeline・error 判定・parent env・sleep／timeout／cleanup の意味は変更しません。(Refs #1106)
- Pull の native 回帰では、branch-menu matrix と専用 checkout 整合性ケースが同じ current ff-only fixture を二重に起動していたため、matrix の重複分だけを除きました。専用ケースの HEAD・index・tracked file 更新／追加／削除の検証、非 current の通常／ff-only × origin／alternate remote の全 4 組と既存 assertion は維持します。compile を除いた retained native binary の独立 warmup 2 回 + 交互 3 pair は全 8 run で対象 2 シナリオが通過し、実 window 起動は毎回 6 回から 5 回になりました。同じ cwd／env／host の runner process wall は Before 12.6790975／13.109934209／11.952215875 秒、After 10.396232959／10.479815／10.118310167 秒でした。background load 未制御の 3 pair であり、精密な速度比や CPU・harness body・workspace・全 gate の速度向上とは扱いません。異なる compile cache の従来の wall 比較も速度向上の根拠にしません。製品の動作は変更していません。(Refs #1106)
- `gpui-terminal` の通常テストから、ホストの共有 clipboard に接続・書き込み・消去しながら失敗を判定しない 3 件の smoke test と、consumer の挙動を検証しない clipboard event の転送 echo 2 件を削除しました。製品の `Clipboard` API と event proxy は変更せず、terminal の選択文字列抽出と、GPUI の private clipboard を使う既存の選択 / Copy の検証を維持します。arboard と OS clipboard の実連携を検証したことにはしません。
- process の deadline / stop / incomplete-I/O テストの後片付けは、ホスト全体の process 名検索・`pkill` ではなく、fixture が公開した子孫 PID と kernel の起動 identity を照合して行う契約にしました。identity helper とそれを使うテストは macOS / Linux に限定し、macOS は既存の `proc_identity` helper、Linux は `/proc/<pid>/stat` を使います。root group を止めるのは所有する Child が未 reap の間だけで、reap 後は記録した子孫 identity だけを照合します。実 subprocess による停止・未完了 I/O の判定と、その他 Unix でも使える既存の direct deadline / I/O テストは維持します。
- Linux の process fixture は各ケース専用の再実行 subprocess を subreaper にし、停止する group の外にいる thread が private な子孫 PID と起動 identity を照合して、その PID だけを reap します。identity 照合 + signal と identity 照合 + reap は fixture ごとの共有 mutex で排他し、signal 前に reaper が PID を解放する競合を防ぎます。container の PID 1 が orphan zombie を回収しなくても fixture 自身が回収し、共有 test harness の subreaper 設定・runner 所有の direct Child・製品の group-stop proof は変更しません。macOS の fixture 動作は維持します。(#1094 review follow-up)
- process cleanup の回帰テストは、leader を reap した後の numeric pgid の消滅ではなく、fixture が記録した PID + kernel start identity による実子孫の停止を判定します。macOS CI では identity による停止確認後も保守的な group 存在 probe が true になり、無関係な追加 assertion が失敗しました。未 reap の所有 Child の group を止める製品の順序・writer lease の保守的な停止証明・同名の別 fixture を止めない検証は変更しません。

## [0.44.0] - 2026-10-06

### Added

- macOS / Linux 向けに `curl -fsSL https://raw.githubusercontent.com/TomiXRM/kagi/main/install.sh | sh` で使えるインストーラーと、mise が直接取得できるプラットフォーム別 tar.gz を追加しました。macOS tar は `kagi-<version>-arm64-macos.tar.gz` (旧 Linux aarch64 更新クライアントが誤認しないよう `aarch64-macos` は使わない)で、ad-hoc 署名済み `Kagi.app` と `bin/kagi` の相対 symlink を格納します。Linux は従来のディレクトリ構造を保ってアーカイブ名に `-linux` を追加します。旧 Linux 名の併載は release workflow の `--legacy-linux-name` 明示指定による移行リリース限定で、この PR を含む初回リリースの公開後にフラグを外します。DMG / AppImage zip / deb / Windows zip は変更しません。`--version vX.Y.Z` / `--prefix DIR` / `--dry-run` / `--no-modify-path` に対応します。実リリースからのインストールと mise 導入は初回対応リリースの公開後に利用できます。(#1055)
- `kagi --version` / `kagi -V` で Cargo のバージョンを表示し、GUI・設定読込・既存インスタンスへの転送より前に終了できるようにしました。(#1055)

### Changed

- Operation Log で展開した行の文字列(before / after・復旧・記録した ref・reflog)をドラッグで選択し、⌘C でコピーできるようにしました。これまでは行の開閉を避けるために押下を止めていたため選択が始まらず、記録した ref と reflog はそもそも選択できませんでした。開閉は 1 行目の要約だけで行い、展開した部分の押下やドラッグでは閉じません。「この操作を取り消す…」「この時点まで戻す…」は背景と同じ色で枠も見えなかったため、アイコン付きのボタンにしました(無効時も無効なボタンとして表示します)。(#1053)

### Internal

- modal / popup の棚卸し撮影を再利用できる E2E 道具にしました。`KAGI_GUI_E2E_ONLY='inventory:<name>'` で 74 対象を EN/JA × Dark/Light で撮り、`scripts/inventory-diff.sh` で before/after を横並びに合成します。(#1047)
- README の画像 10 枚を Apple Dark / 現在の UI で撮り直しました。(#1049)
- PR / issue のスクショは orphan の `pr-assets/*` branch ではなく `gh --attach` でアップロードする運用にし、AGENTS.md を更新しました。(#1058)

## [0.43.0] - 2026-10-05

### Added

- Add Worktree のパス入力の左にフォルダアイコン(24px、「Choose folder / フォルダを選択」)を追加しました。クリックすると Finder / エクスプローラーのフォルダ選択ダイアログが開きます。worktree は新しいフォルダが必要で、ダイアログは既存のフォルダしか返さないため、選んだフォルダの中に branch 名のフォルダを置くパスを入力し、手入力と同じく計画を作り直します。キャンセルでは何も変わりません。(#1043)
- Operation Log に承認済み計画の復旧説明と復旧コマンドを追加しました。Success / Partial / Unknown のみ現在の表示言語で表示・コピーし、説明に埋め込まれたコマンドは別行に一度だけ表示します。記録のない旧エントリは「記録されていません」と明示し、Failed / Refused では復旧欄を表示しません。(#1025)
- merge を操作キューに追加しました。実行中や同じ tab の列の後ろに積み、対象 branch を凍結して順番が来たら計画を作り直します。確認は常に merge modal で行い、競合が起きれば後続は実行しません。(#355 段階 3b-2)
- commit を操作キューに追加しました。先行する操作の後に実行し、本文や stage 済みの内容が変わっていれば確認します。入力欄に focus がある間は待機します。(#355 段階 3b-1)
- 別の操作が実行中のとき、checkout(double click・Enter・branch の右クリックメニュー・確認 modal)を断らずに後で実行する列に入れます。入れた瞬間に status bar の上の列(右寄せ)と短い toast(`Queued: checkout b`)に出ます。順番が来たら plan を作り直し、blocker も warning も無ければ modal なしで実行し(実行中の行に 2 秒以降の経過秒)、それ以外は確認 modal を出します。前の操作が成功しなければ後ろの checkout は実行せず、理由つきで取り消し一覧に残します(消去・tab を閉じる・終了まで、32 件)。列の各行は「外す」でいつでも取り出せ、「すべて取り消す」で tab の列を空にできます。列がある tab では背景の fetch を行いません。commit(段階 3b-1)と merge(段階 3b-2)も同じ列で受け付けます。(#355 段階 3a)
- 2 秒を超えた lease 保有の書き込み操作の busy snackbar に、操作の種類に基づく理由と更新される経過秒数を表示します。未分類は汎用文とし、Skip・残り時間・進捗率は出しません。remote SSH pull も write lease に載った(#997)ため、pull と同じ network の理由を表示します。(#355 段階 1)
- 2 秒を超えた操作と読み込みの説明を、理由(書き込みは経過秒数も)だけにしました。前置きの「時間がかかっています:」と、読み込みの「大きいリポジトリでは〜に時間がかかります」の説明文は表示しません。(#355)
- commit / branch / remote branch / tag / stash / worktree の右クリックメニューをキーボードで操作できるようにしました。開くと最初の有効な項目に focus が移り、↑/↓(端で折り返し)と Home/End で無効な項目を飛ばして移動し、Enter / Space で実行、Escape で閉じます。閉じると focus は開く前の場所へ戻ります(項目が確認 modal を開いた場合は window へ)。Shift+F10(Windows キーボードの Menu キー)で、Graph では選択中の commit のメニューを、サイドバーでは focus のある行(branch / remote branch / tag / stash / worktree)のメニューを、その行の左下に開きます。ウィンドウより長いメニューは項目の部分がスクロールし、キーで移った項目は常に見える位置まで送られます。Home やほかのタブへ移るとメニューは閉じます。項目は `Role::MenuItem`、無効な項目は AX の disabled 状態を持ちます。(#985)

### Changed

- SSH remote pull の実行前検査と `git pull` を同じ SSH 接続内の 1 本の remote script にまとめました。確認後に host の repository・worktree・HEAD・upstream・pull 設定・staged index・作業ツリーが変われば pull せず理由つき Refused を記録します。計画時の拒否は内部コードを除いた理由だけを示し、実行時は `ssh -T` で stderr の判定を保ちます。SSH 認証失敗や結果不明時も既存の Operation Log と lease / reconcile を維持し、別接続への自動 fallback はしません。(#1014)
- CherryPick / StashApply の復旧説明をカード本文から外し、実行可能な計画でのみ復旧コマンドを 1 つの閉じた行に表示します。説明文は「Copy all」と読み上げ用 dialog に残し、Windows の cmd ではコピー可能なコマンドを提示しません。(#1023)
- Clone / Remote Browse / Smart Commit / Update / App Notice / Trust Repo / PR 項目編集 / Editor の名前入力・削除・未保存確認のボタンを 24px に統一しました。無効な確認も消さず、理由を読み上げと tooltip に表示します。削除・破棄の確認は危険色にし、英日両言語の表示と警告アイコンを見直しました。Stash Drop / Pop、履歴操作、Switch to Latest、PR レビュー・merge の実行後 chip は短い状態語とし、完全な予告文は読み上げと Copy all に残します。(#1016 PR B)
- 計画確認・入力確認と Amend / Discard / CherryPick / Commit Plan / StashApply の操作ボタンを共通の 24px に統一し、実行できない確認操作は理由を読み上げられる無効なボタンとして残します。削除・破棄・復元などの破壊的な確認は 2 段階目も blocker 色にし、maintenance 計画の実行後 chip を短い状態語に、詳細を読み上げと Copy all に分けました。commit-graph の確認は「Write / 書き込む」に修正しました。detached HEAD で Reset Current を開いたときの空の branch chip も表示しません。(#1016 PR A)
- 既定の表示スケールを従来の 90% 相当（100%=14.4px/rem）にしました。保存済みの拡大率は初回起動時に換算して従来の見た目を保ち、旧 150% まで保持できるよう上限を 167% に広げました。Graph の行と線・Terminal・メニューの位置も同じ倍率に揃えました。(#1019)
- 共通の計画確認カードと Amend / Discard / CherryPick / Commit Plan / StashApply の見出しを小さな inline icon・短い操作名・対象 chip に整理しました。対象がなければ代替 chip は出さず、behind・操作意図(承認 / コメント / 修正依頼 / upstream 設定)・ファイル / stash / branch の件数を英日それぞれの単位で表示します。入力カードの見出しは対象外です。共通カードと Amend / Discard の復旧コマンドは Ready かつ blocker なし・コマンドありの場合だけ閉じた行に表示します。入力カードの復旧行も有効な入力と Ready を要し、Copy all のコマンド欄も Ready の場合だけ出します。説明文は Copy all と読み上げ用 dialog description に残し、相当コマンドも同じ折りたたみ表示に統一しました。(#994)
- 全 plan 確認カードの状態比較を Stash と同じ縦 2 段の CURRENT / AFTER（日本語は現在 / 実行後）に統一し、MD モーダルを 640px に広げました。Stash の CURRENT ラベルが途中で折り返される問題も修正しました。変更のない状態の chip は日本語では「変更なし」と表示します。SM / LG のカードも指定どおりの幅で描かれるようになりました（これまでは窓幅の上限が自分の幅の 90% として働き、LG が MD より狭くなっていました）。(#1017)
- 計画確認カードの CURRENT → PREDICTED を同じ幅の 2 列と中央の矢印に整理し、状態チップは行内でスクロールできるようにしました。相当する Git コマンドがある計画は折りたたんでコピーでき、見出しは Tab / Enter / Space と読み上げにも対応します（Pull は実行時に再 fetch して merge commit を作る場合があるため、等価コマンドを提示しません）。Operation Log の ref 復元は REFS の移動と削除、変えない対象、既存の線を保った復元後のグラフを先に示し、確認を 2 回必要とする安全境界は維持します。低い窓でも対象 ref の先頭 3 行を優先し、復元後のグラフは 6 行を上限に内容分だけの高さにし、拡大時の横方向の線も見切れないようにします。削除する ref は赤いチップで示します。不正な ref 行の計画は開かず、詳細を Operation Log に記録して短いエラーを表示します。(#988)
- Graph で commit を選ぶと Inspector が 180ms で開き、再クリックや Esc で選択を外すと 150ms で閉じるようにしました。途中の反転は現在の幅から続き、`reduce_motion`、タブ切替、Home、Conflict とほかの workspace への移動は即時です。(#1001)
- Graph の commit 行で author と経過時間を小さい文字にし、名前の列を 96px、時間の列を 48px に縮めました。時間は右寄せの等幅フォントで `36m` などと表示し、名前の全文は tooltip、支援技術向けの経過時間は従来の完全形のまま残します。行高 29px は変更しません。(#1003)

### Fixed

- BranchPicker を開いたまま別の repository や Home へ移ると、旧 repository の branch 一覧が新しい画面に残る問題を修正しました。画面離脱時には About / Keyboard Shortcuts、Command Palette も同じ MenuOverlay として閉じ、元画面への focus を引き継ぎません。(#1039)
- Linux / FreeBSD の platform menu を Settings や確認 modal と重ねたとき、Tab / Shift+Tab を keybinding より前に止め、Escape は dropdown だけを先に閉じるようにしました。menu command の dispatch 前に dropdown を閉じ、tab / repository 離脱時には古い Settings / dropdown を閉じます。dropdown 終了後は Command Palette と入力 modal の既存 input に focus を戻します。macOS の native Tier A で 4 組み合わせと command による tab 閉じ・palette 入力復帰を確認します。workspace の scrim は Linux の titlebar head を覆うため、Tier A は head click の到達性ではなく dropdown が開いた状態の挙動を検証します（Home の titlebar は scrim の外）。(#990、#1038)
- 先行書き込み中に dirty な commit を Enter で checkout しようとしたとき、stash + checkout は 2 回の書き込みなので 1 件のキューとして受け付けられない理由を英日それぞれ footer と toast に表示します。commit checkout の直接入口が対象 OID を固定し、先行操作後の新しい計画と確認・preflight を通ることも native テストで検証しました。(#1032)
- busy 中の checkout は先行 write の途中で変わり得る dirty 状態で投入を拒否せず、静的に不可能な参照だけを先に拒否して Operation Log に理由を記録します。列には即時表示し、順番が来たときの新しい plan の blocker / warning で安全に確認または拒否します。(#1028)
- サイドバー非表示でも Graph の BRANCH / TAG・GRAPH 列の境界がドラッグした 40px だけ動くように修正し、サイドバーの開閉途中もポインターに追従させました。(#1011)
- queued commit の確認中に staged 内容が外部で変わった場合や merge が始まった場合、確認済みの計画を実行せず取り消します。投入時の branch の draft だけを消し、detached HEAD の commit も検証して後続を進めます。入力中の待機理由を明示し、Commit を押した後は入力欄の focus を解放します。(#355、#1020)
- 入力欄を持つ確認 modal を Escape で閉じた後、表示されていない入力欄の focus が残っても操作キューが待機し続けないようにしました。(#355)
- 確認カードと Operation Log からコピーできる復旧コマンドの branch・ref・remote・stash message・worktree path などの実値を POSIX shell で安全に引用するようにしました。`$()` やシングルクォートを含む名前／パスも 1 引数として扱い、手順用の `<branch>` などのプレースホルダーは変更しません。(#1004)
- SSH 経由の remote pull を write lease に載せ、実行中は終了操作とほかの書き込みを保留するようにしました。計画時と実行前に remote の staged index・作業ツリー状態を照合し、変化や再読込失敗があれば pull せず Refused を記録します。結果が Unknown・Partial の場合は reconcile 通知から明示的な確認と監査記録を経て lease を解放します。ssh-agent だけの接続でも計画・実行できます。(#989、#997)
- SSH remote pull はホスト側の `pull.rebase`・`branch.*.rebase`・`pull.ff=only`・`branch.*.mergeOptions` によらず確認どおり merge (可能なら fast-forward) します。`merge.autoStash` と `submodule.recurse` による予告外の stash・submodule 更新も明示的に無効化します。(#997)
- SSH の remote pull で選択したパスが symlink の場合、確認中に別の linked worktree へ付け替えられても誤った worktree に pull しないよう、計画時の物理パスを保持し、実行前に照合してからそのパスで実行するようにしました。異なる場合は実行せず Refused を記録します。(#997)
- SSH の remote pull で確認中に対象 worktree の branch・HEAD commit・upstream が変わっても別の変更を pull しないよう、計画時の状態を実行前に照合し、異なる場合は実行せず Refused を記録します。(#997)
- SSH remote pull は確認を開く前にキャッシュ済みの branch・upstream・HEAD commit・作業ツリーの変更状態とホストの現在の値を照合し、ずれている場合は確認 modal を出さず更新を促します。読取 probe は任意の index lock を取りません。(#997)
- SSH remote pull の計画時に有効な remote URL、fetch refspec、branch の remote / merge 設定を保持し、確認後に設定が変わる・再読込できない場合は pull せず Refused を記録するようにしました。(#997)
- snapshot の作成と conflict の continue / skip(stash の continue、merge の continue、sequencer の continue 確認を含む)が UI thread を止めていたのを直し、background で実行するようにしました。実行中も画面は描画され、2 秒を超えると busy snackbar に経過秒数が出ます。異常終了しても Operation Log に不明な結果を記録し、reconcile の確認後に次の書き込みを許可します。完了前に別のタブへ移っても記録は元の repository に残り、toast や再読み込みは元のタブにだけ出ます。stage / unstage / hunk の後の WIP の +/− 集計も background で行い、連続して stage しても最後の状態だけを表示します。restore-snapshot の遅延理由は stash ではなく worktree の書き込みとして表示します。stage / unstage / hunk 自体は write queue の導入まで同期のままです。確認 modal が途中で閉じても、現在のタブで完了した sequencer continue の表示は再読み込みされます。別のタブへ移った後の snapshot や conflict の失敗は通知に残り、WIP 集計は watcher の新しい結果を古い scan で上書きしません。(#996、#355 R4)
- worktree の容量を計測している間、busy snackbar に「worktree の容量を計測中…」と Skip が出ていたのをやめました。計測は background で続き、行の「計測中」表示と結果はこれまでどおりです。(#1012)
- Windows では、確認カードの Copy all の `commands:` ブロックと、入力カードの折りたたみ行に復旧コマンドを出さないようにしました。コマンドの引数は POSIX shell 向けに quote されていますが、Windows の既定 shell(`cmd.exe`)では single quote が効かず `&` なども区切りとして働くため、貼り付けると意図しないコマンドが動き得ました。説明文はそのまま表示します。(#1007)
- background の fetch・remote branch fetch・PR ref fetch・Editor 保存が異常終了したとき、write lease と実行中の表示が理由なく残り、以後の書き込みを拒否し続ける問題を修正しました。不明な結果を Operation Log に記録し、reconcile の確認後に次の書き込みを許可します。Editor の保存中にペインを閉じても、不明な結果を実行中の Operation Log に反映し、短いエラー toast を 1 回だけ表示します。fetch の完了が元のタブへ戻った後の新しい滞在に表示される問題と、Busy の拒否で確認済み計画が失効する問題も修正しました。(#355 段階 0)
- Info パネル(About / Keyboard Shortcuts)や branch picker が前面にある間、サイドバーの行で Shift+F10 を押しても何も起きないようにしました。これまではメニューは次の描画で閉じるものの、その前に Graph の選択とスクロールがパネルの背後で branch の commit へ移っていました。また、メニューの項目に focus がある状態で ⌘W で最後のタブを閉じると、メニューの状態と消えた項目への focus が残り Welcome でキーが効かなかったのを直し、メニューを閉じて focus を window へ移すようにしました。(#1000)
- Reset Current などの共通確認カードで、blocker がある計画や実行しない計画に相当 Git コマンドが付いていても、折りたたみ行・専用コピー・「Copy all」に表示しないようにしました。Operation Log の復元カードと同じ条件を使います。(#993 review)
- Operation Log の ref 復元計画で不正な ref 行やリポジトリセッションの欠落を検出したとき、footer と toast の短いエラーを表示言語（英語・日本語）に合わせました。完全な decode エラーは引き続き Operation Log に残します。(#993 review)
- 計画確認カードの相当 Git コマンドで、branch・remote・refspec・target OID など動的な引数を POSIX shell 向けに quote します。名前に `$(` や single quote が含まれても、表示・コピーしたコマンドで shell の置換を実行しません。POSIX quote が使えない Windows ではコマンドを隠し、Push は remote 名の前に `--` を付けて option としての解釈を防ぎ、「Copy all」の見出しも EN/JA に合わせます。(#993 review)
- Operation Log の ref 復元プレビューで「Copy all」を使うとき、branch から外れる commit の印を現在の表示言語（英語・日本語）で出すようにしました。(#988)
- Operation Log の ref 復元計画に blocker がある場合は、実行できない `git update-ref --stdin` をカードと「Copy all」から除きます。長い ref 名は名前欄で省略し、全文は tooltip・読み上げ・コピーに残したまま、移動前後の OID をカード内に表示します。(#988)

### Internal

- 操作キューの核を副作用のない app reducer として追加しました。session ごとに intent を並べ、成功・検証・記録・reconcile の receipt で後続を判定します。画面への配線は #355 段階 3 で行います。(#355 段階 2)
- remote pull の後片付けと SHA の fallback を検証するテストを追加しました。(#1037、#1041)
- UI ガイドの Known gaps を現状に合わせて更新しました。Settings の通常の focus trap と前面判定、Home / Graph の行キー操作は対応済みとし、未解決の 100 Tab stop 超の制限、Linux / FreeBSD の platform menu と overlay の組み合わせ、UI thread の同期書き込みと WIP diffstat を明記しました。(#974、#976、#980、#986、#981、#987、#990、#996)

## [0.42.0] - 2026-10-04

### Added

- Home の行一覧と Graph の commit 一覧を Home / End / PageUp / PageDown で移動できるようにしました。Cmd+↑/↓ でも先頭・末尾へ移動します。ページ移動は表示中の行数を基準にし、移動先が画面に収まるようスクロールします。(#980)

### Fixed

- Home や Branch Cleanup が Graph を隠している間、または Settings・確認 modal・メニュー(menu overlay、commit / branch / stash / tag / worktree の右クリック、Linux / FreeBSD の platform menu など)が Graph に重なる間、End / Home / PageUp / PageDown と ↑/↓ で背面の commit 選択が変わる問題を修正しました。選択中の先頭行をホイールで画面外へスクロールした後も、Home で再表示できます。(#980)
- filter menu が開いている間に Stash などの確認 modal が届くと、背面の確認を Enter で確定できてしまう問題を修正しました。重なり順を `Z_ORDER` に一元化し、描画とキーの前面判定を同じ順序で行います。(#976 review)
- repo A の file context menu を開いたまま repo B を開くと、描かれなくなった A の menu が Enter を消費し続ける問題を修正しました。描画とキー操作は同じ可視判定を使い、Escape は前面の menu をまとめて閉じます。(#974、#976 review)

- ツールバーで使えない Pull / Push / Stash / Pop / Undo / Redo も Tab で選べるようにしました。キーボードで選ぶと枠が表示され、Enter / Space はクリックと同じ理由を下部に示します。AX のボタン名は維持し、使えない理由を `aria_description` に設定します。(#972)
- Home のレビュー依頼の行で、GitHub Enterprise の host が大文字を含むとき(`GHE.example.com` など)、取得済みのアバターではなくイニシャルが表示される問題を修正しました。(#968)
- 実行中の clone の card を閉じて 1 秒以内に開き直すと、card の再描画が二重に動き続ける問題を修正しました。(#968)
- Graph / PRs / Issues の切り替えで ←/→ で移動したあと、Tab 以外の方法(マウスで別の操作部品を押すなど)で focus が外れると、次の Tab が選択中のタブではなく矢印で移動したタブに着く問題を修正しました。Home の「リポジトリ / Pull Request / Issue」とリポジトリのタブ帯も同じです。(#968)
- Operation Log の RestoreToPoint 確認カードから重複する説明文を外し、グラフを表示できないときは EN/JA とも短い状態だけを表示します。ref の移動と戻さない対象の警告、および Git の相当コマンドはそのまま表示します。旧ログの branch だけを観測した記録は tag を含む復元の根拠にせず、安全のため実行前に拒否します。Kagi 外で動いた tag は reflog が残る場合だけ検出し、同じ tag / branch をその後で Kagi が動かしても、記録外の遷移が一つでもあれば拒否します。reflog の無い tag は戻せない限界を「変更なし」行で明示します。(#953)
- Settings を開いたままスクロールすると、背面の画面(Graph・PRs・Issues・Editor)も一緒にスクロールする問題を修正しました。Settings の背景が、背面へのマウス操作をスクロールも含めて遮るようにしました。
- Terminal に focus がある状態で Settings を開くと、Settings の中で押した Tab が Terminal の shell に届き、Esc でも Settings が閉じない問題を修正しました。Settings を開くと focus が Settings に移り、Tab / Shift+Tab は Settings の中だけを循環します。閉じると、開く前の場所(Terminal など)に focus が戻ります。ただし開いている間に別タブへ切り替わった場合は古い Terminal ではなく新しい画面へ戻し、テーマの選択 popup を開いたまま modal が届いた場合はその modal の入力欄以外の focus をウィンドウへ移して Esc を届かせます。(#974)
- Settings を開いたまま Cmd+J または View → Toggle Terminal で下部パネルを閉じると、Esc の後に非表示の Terminal へ focus が戻り、画面のキー操作が効かなくなる問題を修正しました。開閉どちらでも Settings を閉じ、表示中の画面に focus を戻します。(#974、#976 review)
- Worktree 削除の確認後や削除前ステップ後に ignored ファイル・フォルダーが増えた場合、削除前に中止し、計画の再確認を促すようにしました。(#934)
- 初期化済み、または未初期化でも gitlink のパスにローカルファイルがある worktree は Remove の計画時・実行前に削除を拒否します。空・不在の gitlink は削除可能なままとし、削除前ステップ後の拒否も EN/JA の短い toast に理由だけを表示します。(#934)
- Remove は削除対象の worktree を開いているタブからは計画・実行前に拒否し、EN/JA の短い理由を示すようにしました。自己削除に必要だった main worktree の場所の証明と削除後の観測は、安全性に対して複雑すぎるため廃止しました。main worktree の削除も、main / linked のどちらのタブからも拒否します。(#915、#938)
- 存続する別タブからの recorded Remove は実行元の HEAD と共有 ref の前後を観測します。branch 維持は `Some(空)`、branch 削除は OID 差分を記録し、RestoreToPoint でも利用します。削除前ステップで実行元の checkout が変わった場合は HEAD 移動も記録し、その記録を越える RestoreToPoint を拒否します。(#915、#938)
- bare repository を common dir とする複数の linked worktree でも、linked 側から別の linked worktree を Remove できます。削除前 copy / symlink ステップの入力元は bare / non-bare を問わず実行元の checkout とし、`--separate-git-dir` の common dir から推測した main を使いません。recorded Remove は存続する実行元から ref を読みます。(#915、#938)
- Remove 対象のディレクトリ内に別の登録済み worktree がある場合は、実行元のタブを問わず計画・実行前・削除直前に拒否します。ignored フォルダー内の未バックアップのファイルも保護します。(#915、#938)
- `--separate-git-dir` で common dir が削除対象 worktree の中にある場合は、main workdir が別の場所でも Remove を計画・実行前・削除直前に拒否し、共通 ODB / refs を保護します。削除境界では実行元の workdir、common dir、および common repository の main workdir がある場合はその場所を保護します。(#915、#938)
- Remove 対象の worktree の中に別のリポジトリ(`.git` を持つもの。移動した main checkout、独立した clone、submodule の checkout など)がある場合は、計画時・実行前・削除直前に拒否し、その場所を示します。tracked / untracked / ignored を区別せず、対象のフォルダー全体を調べます(削除自体も同じ範囲をたどります)。symlink はたどらず、最初に見つかった `.git` で止まります。名前の大文字小文字は区別しません(macOS / Windows では Git が `.GIT` も `.git` として開くため)。(#951)
- GitHub Enterprise の repository で、Issues の「Assigned to me / Created by me」と PR の Mine などの判定に github.com のアカウントを使っていた問題を修正しました。repository のサーバーでのアカウントで判定し、それがまだ分からない間は件数を「—」にして、空だとは表示しません。Enterprise のユーザーのアバターも、github.com の同名ユーザーではなく、そのサーバーから取得します。(#906)
- New Issue の本文エディタが Tab を字下げとして取り込み、キーボードだけでは下のラベル・担当者・Create へ進めなかった問題を修正しました。Issue の本文エディタ(新規・返信)でも Tab / Shift+Tab でフォーカスが移動し、字下げは Cmd+] / Cmd+[(Linux / Windows は Ctrl+] / Ctrl+[)で行えます。(#909)
- Linux / FreeBSD の View メニューで、テーマ一覧がウィンドウの下へはみ出し、後半のテーマや言語の項目を選べなかった問題を修正しました。メニューの高さをウィンドウ内に収め、入りきらない項目はメニューの中でスクロールして選べます。(#935)
- Operation Log で行を開いたときの詳細が「before: branch: maindirty: cleanafter: …」のように 1 行につながって表示されていた問題を修正しました。before / dirty / after などを 1 項目ずつ改行して表示します。(#908)
- Operation Log の操作者の表示を、日本語表示でも「人」ではなく「Human」にしました(CLI / MCP と同じく英語の表記)。(#908)
- Inspector の commit 本文で、箇条書きなど改行で区切られた行が「- bump the version- tag the release」のように 1 行につながって表示されていた問題を修正しました。本文の改行どおりに 1 行ずつ表示します。(#946)
- Issues の下書き(新しい Issue と返信)を、clone ごと・番号ごとに加えて書き込み先の repository ごとに保存するようにしました。`gh repo set-default` で別の repository に切り替えた後、同じ番号の別 Issue の欄に前の repository 宛ての下書きが出て、そのまま投稿されることはありません(前の下書きは消さずに残ります)。以前の版で保存した下書きは、clone の remote が指す repository が 1 つだけのときにその repository へ 1 回だけ引き継ぎ、複数あるときは引き継がずに元の場所に残します。(#940 review)
- Home の「リポジトリ / Pull Request / Issue」の切り替えと、左上の Graph / PRs / Issues の切り替えを、キーボードで操作できるようにしました。Tab で選択中のタブに移動し、←/→ で隣へ、Home / End で端へ移動します。Home の切り替えは移動と同時に表示が切り替わり、Graph / PRs / Issues は移動のあと Enter / Space で切り替わります(PRs / Issues は開くと一覧を読み込むため)。キーボードで移動したときだけ、入力欄と同じ色の枠を表示します。(#944)
- Home のリポジトリ行と PR / Issue 行は、一覧全体で Tab 1 回分になり、↑/↓ で前後の行へ移動します(画面外の行はスクロールして表示します)。Enter / Space でクリックと同じ動作をします。Tab で一覧に戻ると最後に選んだ行(画面に無ければ画面上の最初の行)に入り、絞り込みなどでその行が消えたときは残った最初の行へ、行が無くなったとき(読み込み中や読み込みの失敗で一覧が消えたときも)はウィンドウへ移ります。検索欄の ↑/↓ は検索欄のままです。(#959)
- 上部のタブ帯(リポジトリのタブと Home)もキーボードで操作できるようにしました。Tab で選択中のタブに移動し、←/→ / Home / End で移動して、Enter / Space で切り替えます(タブを切り替えるとそのリポジトリを読み直すため、矢印では切り替えません)。タブ帯は Tab 1 回で抜けられます。各タブの × と右端の + は Tab で止まりません(⌘W で前面のタブを閉じ、⌘T で Home を開けます)。focus のあるタブを閉じたときは、focus はウィンドウに移ります。(#959)
- clone 中のリポジトリ行(「Cloning…」)をクリックすると、開始前の新しい clone card が開き、clone が終わっても残っていた問題を修正しました。実行中の clone の card を前面に戻します。(#944)
- Home のレビュー依頼の行で、GitHub Enterprise のユーザーのアバターを表示するようにしました。github.com の同名ユーザーではなく、その Enterprise のサーバーから取得します。(#944)
- PR を開いたときの会話・レビュースレッド・merge 状態を、その PR の repository から読むようにしました。これまでは clone の `gh repo set-default` が指す repository から読んでいたため、Home から別の repository を指す clone で PR を開くと、同じ番号の別 PR の会話が本文の下に並ぶことがありました。(#940 review)
- `gh repo set-default` を別の repository に切り替えた後に Home からその repository の Issue を開くと、前の repository の Issue 一覧が残り、その行を選ぶと新しい repository の同じ番号の Issue に返信できてしまう問題を修正しました。宛先が変わった時点で前の一覧・選択・続きの読み込み位置を消し、新しい repository の一覧が読めるまで行は表示しません。(#940 review)
- 同じタブに別の repository の同じ番号の PR(A の #7 と B の #7)を開いていると、B の会話・レビュースレッド・merge 状態が A の PR 画面に入ることがあった問題を修正しました。PR の画面・詳細の読み込み・会話・merge 状態・コメント欄の下書きを、番号だけでなく repository と番号の組で対応づけます。Home から開いた PR の詳細も、PR 一覧にある別の repository の同じ番号の PR ではなく、開いた PR のものを読みます。(#940 review)
- Home から Issue を開くとき、そのタブの Issues をまだ一度も開いていなかった場合にも、確かめた repository に一覧と Reply の宛先を固定するようにしました。Issue の本文の読み込みも `-R` でその repository から読みます。これまでは確かめた直後に `gh repo set-default` が変わると、別の repository の同じ番号の Issue が表示され、それに返信できました。(#940 review)
- Set Upstream の形式エラーが入力欄の直下と plan blocker 一覧に二重表示される問題を修正しました。(#956 review)

### Changed

- Create Branch / Create Tag / Stash / Add Worktree / Rename Branch / Set Upstream の入力確認カードを、基本 32px の入力欄・確認ボタン、入力欄の直下に出る検証理由、常に見える無効な確認ボタンに統一し、見出しの従来の操作別アイコンは残しました。Stash 以外の CURRENT → PREDICTED は横 1 行にし、Stash は 38px のメッセージ欄の下に現在と実行後の状態を上下に並べ、暗い背景・小さい状態チップと簡潔な警告にしました。Stash のキャンセル・確認ボタンだけ従来の角丸のまま高さを 24px に縮めます。branch / HEAD と staged・modified・untracked などの状態は状態名・件数付きで表示し、警告の全文は Tooltip / 支援技術向けラベルに残します。空欄や実行不能な計画では復旧行を出さず、入力済みの実行可能な計画では Git コマンドだけ表示します（Create Branch はカード内に復旧行なし、Set Upstream は復旧コマンドなし）。完全な復旧説明は Operation Log に残します。IME 変換中の Enter は 6 種類すべてで Git 操作を確定しません。(#956)
- Graph の「Avatar commit nodes」(commit の点を作者のアバターにする表示)を既定で ON にしました。設定で一度 OFF にしている場合はそのまま OFF です。
- Worktree 行とホバーカードをアイコン・短い状態表示中心に整理し、再計測はアイコンのみ（支援技術向けの名前は維持）にしました。ignored file の注意はホバーから外し、削除時の確認計画で対象のファイル数とフォルダー数を示します。(#934)
- Graph で行を選択しているとき、Esc で選択を解除できるようにしました(右側の commit 詳細も閉じます)。メニューや diff、確認画面が開いている場合は、従来どおりそちらが先に閉じます。
- Terminal / Operation Log / Activity の下部パネルをウィンドウ全幅から main pane の下部へ移しました。開くと main の内容だけが縮み、サイドバーと右側の Inspector / Commit Panel に加え Editor の file tree / hunks と PR / Issues の navigator（PR の swimlane も）はステータスバーまで表示されます。従来の高さ変更、Cmd-J、タブ切替、Conflict 画面での非表示は維持します。（ADR-0007）
- Home の一覧(リポジトリ / PR / Issue)の項目を、描画のたびではなく、表示中の切り替え先のデータ・絞り込み・表示言語・手元の clone・clone 中のものが変わったときだけ作り直すようにしました。spinner やカーソルの点滅による再描画で、全件の小文字化と複製を毎フレーム行わなくなります。表示していない切り替え先の読み込みが届いても一覧は作り直さず、Refresh や表示言語の切り替えで作り直したときもスクロール位置を保ちます。(#937、#942 review)
- Home の見出しの下にあった説明文(「最近開いたリポジトリを選ぶか、フォルダーを開くか、SSH で接続します。」)を削除しました。見出しとボタンはそのままです。
- Cmd+J の下部パネル(Terminal / Operation Log / Activity)を、高さのアニメーションで出し入れするようにしました(開く 180ms ease-out、閉じる 150ms ease-in)。途中でもう一度押すと、その位置から逆向きに戻ります。動く間も Terminal の行数・桁数は変わりません。「動きを減らす」が有効なら即座に切り替わります。(#950)
- 左のサイドバー(View → Toggle Sidebar)と右の Inspector / Commit Panel(View → Toggle Commit Details)の表示・非表示も、下部パネルと同じ時間と動き(開く 180ms ease-out、閉じる 150ms ease-in)で、幅だけを動かして出し入れするようにしました。途中でもう一度押すとその位置から逆向きに戻り、Graph の行・選択・位置は動きません。Inspector と Commit Panel の切り替えや、モードの切り替えでの表示・非表示はこれまでどおり即座です。「動きを減らす」が有効なら即座に切り替わります。(#955)

### Added

- Graph のサイドバー(ローカル / リモートの branch・worktree・tag・stash)の行にキーボードで移動できるようにしました。各ペインが Tab で 1 か所ずつ止まり、↑/↓ でそのペインの中の行を移動します(ほかのペインには移りません)。Enter / Space で、branch は checkout の確認(現在の branch はグラフ上の位置へ移動)、worktree はクリックしなくても点検カードの表示 / 非表示(Esc でも閉じます)、グループは開閉、リモートの branch と tag はそのコミットへの移動、stash は中身の表示です。折りたたんだペインは見出しに止まり、Enter / Space で開いて最初の行へ移ります。キーボードで focus したときだけ枠が出ます。マウスで行をクリックしたときの動作はこれまでどおりです。(#981)
- リポジトリのタブ帯で選んだタブの中身(サイドバー・中央・右ペインの本体)と Home の本体を、支援技術に「選択中のタブの中身(tab panel)」として伝えるようにしました。名前はそのタブの名前です。toolbar とステータスバーは中身に含めません。Conflict Mode の画面はまだ tab panel ではありません。見た目は変わりません。(#983)
- Home の「リポジトリ / Pull Request / Issue」の切り替えの下の一覧と、サイドバー上部の Graph / PRs / Issues の下のページを、支援技術に「選択中のタブの中身(tab panel)」として伝えるようにしました。名前は選択中のタブの名前(件数なし)です。Branch Cleanup などタブが選ばれていない間のページは tab panel にしません。見た目は変わりません。(#979)
- Settings のスイッチ(Graph の表示 2 つ・自動 fetch・動きを減らす・terminal の自動ロック・Smart Commit)をキーボードと支援技術から操作できるようにしました。Tab で 1 つずつ移動し、Space / Enter で切り替えます。読み上げでは行の見出しを名前とするスイッチとして、オン / オフの状態とともに伝わります。キーボードで focus したときだけ枠が出ます。見た目とマウス操作はこれまでどおりです。(#970)
- Home に「リポジトリ / Pull Request / Issue」の切り替え(件数付き)を追加し、全リポジトリ横断で自分が作った open な PR、自分にレビュー依頼された open な PR(依頼者のアバター付き)、自分に assign された open な Issue を一覧できるようにしました(`gh search`、各最大 100 件、それ以上あれば表示)。行をクリックすると手元の clone のタブでその PR / Issue を開き、手元に無ければ GitHub で開きます。行の右の「Open」はどの行でも GitHub で開きます。読み込み中の件数は 0 ではなく spinner で示し、一覧はリポジトリ一覧と同じくアカウントごとに保存して次回すぐに表示します。(#928、ADR-0219)
- Operation Log の「この操作を取り消す」「この時点まで戻す」で local tag の作成・削除・移動も戻せるようにしました。lightweight tag と annotated tag の raw object OID を前後で記録し、確認後に tag が変われば拒否します。復元前の OID は backup ref に保持し、作業ツリー・index・untracked・stash・remote branch は変更しません。tag を動かす計画では不正確なグラフ予測を出さず、短い状態だけを card に示します。(#887)
- タブ帯の「+」と New Tab(⌘T)で「Home」タブ(ダッシュボード)を開けるようにしました。最近開いたリポジトリ、フォルダーを開く、SSH リモートへの接続に加え、自分と所属 organization の GitHub リポジトリ一覧(owner ごとの見出し付き、`gh repo list` で各最大 1000 件、それ以上あれば表示。読めない organization はその理由を表示し、organization の一覧自体を読めないときもその旨を一覧に表示)を検索欄で絞り込み、手元にあるものは開き、無いものは clone できます。手元の clone は最近開いたリポジトリと開いているタブの `origin` で照合し、`~/.ssh/config` の別名(`git@work-github:…`)も実際の host に解決します。一覧は前回読んだものを読んだ GitHub アカウントと一緒に `settings.json` と同じフォルダーの `github_repos_cache.json` に保存し、同じアカウントのときだけ Home を開いた瞬間に表示し、自分と organization の一覧を並列に読み直して差し替えます(読み直しに失敗したときは前回の一覧を残して toast で知らせます。`gh auth switch` で別アカウントに切り替えた後は前の一覧を出しません。読み込み中の Refresh は押せません)。clone ではまず保存先のフォルダーを選びます(Kagi が勝手に決めることはなく、選ぶまで clone ボタンは押せません)。選んだフォルダーの中にリポジトリ名のフォルダーを作り、既に何かがあるときは clone しません。失敗・中断して残ったものは削除せずに場所を Operation Log に記録し、Home にも toast で知らせます。clone は 30 分で打ち切り、Kagi が起動したプロセスだけを止めます。成功すると Home がそのリポジトリのタブになります。タブが無いときの Welcome 画面は Home に置き換わりました。(Closes #923、Closes #924、ADR-0219)
- `~/.kagi/themes/*.json` (または `KAGI_LOG_DIR/themes/*.json`) から自作テーマを読み込み、組み込みテーマの色を部分上書きして Settings / メニュー / command palette から選べるようにしました。Settings ではフォルダーのパス表示・作成して開く操作(Windows では Explorer で開きます)・画面を止めないバックグラウンド再読み込みもできます。再読み込みの連打では最後の結果だけ反映します。フォルダーを列挙できないとき(権限など)は読み込み済みのテーマを残し、理由を toast で知らせます。形式と各色の用途は [テーマガイド](docs/themes.md) に記載しています。(#922、ADR-0220)
- gpui-component の固定版全 UI 部品、Kagi の画面別操作部品、Zeron の設計例を一次資料と実画面で比較した調査資料を追加しました（#931、実装・外観の変更はありません）。
- Modern UI の PM 基準案を Kagi と固定版 gpui-component の寸法・操作状態・高密度画面・安全確認に照らして批判した資料を追加しました（#931、実装・外観の変更はありません）。
- UI 実装ガイドの統合版を実コードと固定版部品に再照合し、PR/Issue 行・タブ・Switch・モーダルの誤認を訂正して、第 2 ラウンドの批判と検証手順の不足を記録しました（#931、UI 動作の変更はありません）。
- UI 実装ガイド(`docs/ui/modern-ui.md`)に、下部パネル・サイドバー・右ペインの開閉で共有する `panel_motion`(開く 180ms ease-out、閉じる 150ms ease-in、`reduce_motion` と layout の切り替えでは即座、動いている間は divider のドラッグを無視)を既存部品として追記し、ガイドの有無で同じ画面を作らせた #931 の A/B の結論を記録しました(兄弟部品がある画面では見た目は変わらず、効くのは検証と記録)。UI 動作の変更はありません。

### Internal

- #956 の入力確認カードで古く見えた要因と、6 カードの役割別の Input・エラー・確認ボタン・復旧表示を `docs/ui/modern-ui.md` に記録し、未解決の「テキストボックスの原因」から切り分けました。(#956)
- GUI E2E runner(Tier A)が開発者の環境を読まないようにしました。`HOME` は run 専用の空の directory(fixture と同じ git の identity だけを置く)にし、継承した `GIT_*`・`GH_*`(と `GITHUB_TOKEN` / `GITHUB_ENTERPRISE_TOKEN`)の環境変数は起動時にすべて除去します(`gh` も開発者の設定と認証情報を使いません)。terminal を起動する scenario は、login shell ではなく行を読むだけの代わりの shell を使います。代わりの shell が無いまま terminal を起動しようとすると、利用者の `$SHELL` を起動する前にその scenario が失敗します。Smart Commit の生成を差し込む scenario は、差し込んだ生成が使われたことを確かめます。(#516)
- Web(Playwright)の harness は、`crates/kagi-web/dist` が無いと設定の読み込み時に止まり、足りないファイルと実行すべき `scripts/build-web.sh` を示すようにしました。これまでは 60 秒後に webServer のタイムアウトとして失敗し、実行時のハングと区別がつきませんでした。(#516)
- GUI E2E runner に `KAGI_GUI_E2E_KEEP_GOING=1` を追加しました。選んだ scenario を 1 つずつ別の runner process で実行するので、1 つが失敗(panic・crash・既定 600 秒の timeout)しても残りを実行し、最後に scenario ごとの PASS / FAIL と失敗の証跡の場所を一覧にします。1 つでも失敗すれば終了コードは 1 です。既定は従来どおり最初の失敗で止まります。(#516)
- 検証手順(`.claude/skills/verify/SKILL.md`)の Tier A に、GUI E2E の各 scenario で文字が本物の `InputState` にどう入るか(キー入力・貼り付け・`set_value`)と、`InputState` を使わない代わりの経路(commit panel の `commit_msg` fallback、Remote Browse の host 入力、`queue_*` の読み込み差し替え)の表を追加しました。GPUI の終了時の leak 検出を無効にしている scenario が無いことも確認して記録しました。製品の動作は変更していません。(#516)
- GUI E2E runner で、bare の `origin` に `main` を push して clone する scenario(`remote_pull_lease` など)が `src refspec main does not match any` で落ちていたのを直しました。新しい repository の既定 branch(`init.defaultBranch = main`)は、これまで Apple Git の vendor 設定から来ていて、#963 で system 設定を読まなくしたときに一緒に消えていました。run の `.gitconfig` と、fixture の `git` が読む command-scope の設定に、この 1 つだけを戻しています。製品の動作は変更していません。(#516)

## [0.41.0] - 2026-10-02

### Changed

- テーマ「Flower Road」を 1 つにまとめました。graph の線は「Flower Road Vivid」の 8 色(色相環に均等配置)にしました。「Flower Road Bloom」「Flower Road Vivid」は削除し、それらを選んでいた設定は「Flower Road」として読み込みます。
- サイドバー上部の Graph / PRs / Issues で、選択中のタブの文字が背景と近すぎて読みにくいテーマ(Flower Road のベージュ地にピンク、1.6:1)では、同じピンクを背景に対して 4.5:1 まで濃くして表示するようにしました。

### Added

- Issues の「新しい Issue」で、作成前にラベルと担当者を選べるようにしました(#866)。PR の項目編集と同じ picker で repository の一覧から選び、作成時に `gh issue create --label … --assignee …` で送ります。作成直前に repository を読み直し、無くなったラベルや割り当てられない担当者があれば `gh` を呼ばずに Operation Log へ「拒否」と記録して toast で知らせ、入力した本文と選択はそのまま残します。作成者は「<login> として投稿」と表示するだけで変更できません(`gh` は認証中のユーザーで投稿するため)。選択は本文と一緒に下書きへ保存され、アプリを再起動しても残ります(#903)。読めなくなった下書きファイルは上書き・削除せず `<file>.corrupt` として残します。(Closes #866, Closes #903)
- Editor Workspace と Commit Panel の file tree を支援技術から名前付き tree として読めるようにしました。各ファイルは名前・変更状態・選択状態・階層と兄弟位置を持ち、Editor のフォルダーと Panel の Generated / Agent fold は開閉状態を伝えます。名前は EN/JA に対応し、未保存の編集も読み上げます。通常の flat 表示は tree にしません。（#354 slice 3）
- Graph のサイドバーを local branch / remote branch / worktree / tag / stash の 5 つの縦ペインに分けました。PR は上部の PRs タブで見られるため Graph には重複表示しません。各ペインの見出しは固定、一覧は別々にスクロールでき、境界をドラッグして高さの比率を変更できます。既定の比率は 35 / 25 / 20 / 10 / 10。通常の行は 20px / xs、worktree 行は 24px、補助文字はさらに小さくして、ズームとともに拡大縮小します。閉じたペインは見出しだけになり、再展開で元の高さ比に戻ります。比率と開閉は `settings.json` に保存され次回起動時も復元されます。狭い画面では 5 枠の外側をスクロールできます。不正な保存値は既定配置で表示し、起動・描画だけでは上書きしません。(#864、ADR-0217)
- Graph の WORKTREES 行にホバーすると、その worktree の容量・削除可否・注意点を詳細カードで確認できます。従来の下部固定ペインは廃止し、5 つのペインに使える高さを確保しました。カードにポインターを移しても閉じず、容量の再計測はカード内の「更新」から行えます。右クリックメニューを開いている間はカードが退き、メニューを隠しません。削除しない main worktree は一覧と容量計測から外し、見出しは linked worktree の件数を示します。main への Graph からの移動と port block の共有は残します。観測と削除の安全条件は変わりません。（#911 review、ADR-0175 / ADR-0217）
- conflict の continue / skip / abort と解決内容の保存(save)も、Operation Log に動かした ref を記録するようにしました(#884、ADR-0214 §4)。これまでは記録なしの扱いだったため、Kagi で conflict を解いた merge や cherry-pick をまたぐ時点には「記録なし」で戻せませんでした。今は merge 前の時点にも復元できます。実行前に拒否された場合は「動いた ref はありません」と記録します。rebase は途中で HEAD が detached になるので、これまでどおり復元の対象外です。(Refs #334)
- PR 一覧（PR モードの表）の accessibility（#354 slice 3、3 本目）。支援技術から表を list として、各行を「番号・タイトル・状態・作者・branch・check・更新」で名前付きの項目として、並び順の何番目か付きで読めるようにしました（表は選択状態を持たないので selected は付けません）。（Refs #354）
- Operation Log の「取り消す / この時点まで戻す」の確認 card に、戻した後のグラフを表示するようにしました(#334 slice 2c、ADR-0214 §6)。branch が戻る位置と、どの branch からも外れる commit の数を、変化する部分の前後(最大 40 行)だけ、通常の commit graph と同じ描き方で示します。計算は読み込み済みの履歴だけで行い、戻し先がその中に無い場合は推定せず「プレビューできません」と表示します(復元自体はできます)。表示専用で、確認するまで何も書き込みません。(Refs #334)
- サイドバーの accessibility（#354 slice 3、2 本目）。支援技術から Graph の 5 ペインを tree として、section・group の見出しを開閉状態付き、branch・remote branch・tag・worktree・stash の各行を階層と兄弟の中での位置付きで読めるようにしました。PR は上部の PRs タブの list で読み上げます。現在の branch と worktree は名前に「現在」と含めます。（Refs #354、#864）
- commit 一覧の accessibility（#354 slice 3、最初の一覧）。支援技術から commit 一覧を list box として、各行（WIP・stash・commit）を「件名・作者・日付・短い SHA・ref」で名前付きの選択肢として、選択状態と全体の何番目か付きで読めるようにしました（画面外の行は描画しないため、位置と総数で全長を伝えます）。（Refs #354）
- 色覚対応テーマ「Color Vision (Blue/Orange)」/「色覚対応（青 / 橙）」を追加（#354 slice 4、ADR-0216）。Catppuccin Mocha を元に、追加 / 削除・成功 / blocker・ours / theirs・diff 行の背景を Okabe–Ito の青 / 橙に、warning を黄（輝度差）に、graph lane を 8 色の色覚安全パレットに置き換えました。CIEDE2000 で通常視 20 以上、1 型・2 型・3 型色覚のシミュレーション後も 15 以上の色差をテストで保証しています（既定テーマの diff 背景は 2 型で 4.3）。Settings の theme 選択から選べます。（Refs #354）
- Operation Log の選択した行に「この操作を取り消す…」「この時点まで戻す…」を追加しました(#334 slice 2b-2、ADR-0214 §5)。押すと確認 card を開くだけで、card には branch ごとの戻し方(移動・削除・作り直し)と、戻らないもの(作業ツリー・index・untracked・stash・tag・remote branch)を表示します。branch を書き換える操作なので二段 confirm で、確認すると branch だけを戻し、動かす前の先端は `refs/kagi/backups/` に残ります。取り消し・復元自体も Operation Log に記録され、その行から同じように取り消せます。ref の移動が記録されていない操作(この記録より前の操作や、記録に失敗した操作)では両方のボタンが押せず、理由を表示します。(Refs #334)
- Operation Log からの「1 操作の取り消し」(`op-revert`)と「時点への復元」(`restore-to-point`)の backend(#334 slice 2b-1、ADR-0214 §5)。どちらも各操作が記録した ref の移動だけを根拠に branch を戻し、記録の無い操作を含む範囲・HEAD の切り替えを含む範囲・後の操作が同じ branch を動かした取り消し・記録外で動いた branch・進行中の merge / rebase・checkout 中の branch の削除は理由を示して拒否します。実行前に動かす branch の現在の先端をすべて `refs/kagi/backups/` に保持し、`git update-ref --stdin` の 1 トランザクション(古い値は git が照合)で戻します。作業ツリー・index・untracked・stash・tag・remote branch は戻しません。取り消しや復元自体も ref の移動つきで記録されるので、さらに取り消せます。二段 confirm が必要な操作です。UI はまだありません。(Refs #334)
- `worktree_run_mode` が `"nonconcurrent"` のときは、同じ repository のすべての worktree の terminal に、main worktree の port block を `KAGI_PORT` として渡すようにしました(固定 callback URL がどの worktree でもそのまま使えます)。`KAGI_WORKTREE_PATH` などはこれまでどおり各 worktree のものです。サイドバー WORKTREES の link もすべて main worktree の port を指します。保存済みの割当は変更しないので、`"concurrent"` に戻せば各 worktree の block に戻ります。警告だけのモードは追加しません。(#869、ADR-0213)
- worktree の削除で、Kagi の埋め込み terminal がその worktree で起動した shell がまだ動いていれば、plan を blocker で止めるようにしました。理由は「先にその terminal で exit してください」です。Kagi はユーザーのプロセスを終了させません。shell の終了後も `nohup`・`disown`・bash の `&` などで起動したプロセスがその shell の session に残っていれば、件数を添えて「このディレクトリを使っている可能性」を warning で出します(削除は止めません)。Kagi の外で起動したプロセスは検出しません。(#867、ADR-0171 Follow-up 1)
- 確認 card の accessibility（#354 slice 2）。共通 plan card（delete branch / delete remote branch / reset / force-with-lease push / replay / conflict abort など）と amend / discard の card を、支援技術から dialog（destructive・二段 confirm は alert dialog）として、名前・説明（未 arm / arm 済み / 実行不可）・Confirm / Cancel の操作付きで読めるようにし、warning は note、blocker は alert として名前を付けました。amend / discard の確定ボタンの文言を EN/JA 化しました。（Refs #354）
- Operation Log の各操作に、その操作が動かした ref(実行した worktree の HEAD の指す先と commit、各ブランチ)を old / new の OID で記録するようにしました。行を選ぶと記録された移動を「記録」として表示し、記録の無い以前の操作や記録しない経路の操作だけを、これまでどおり時間帯の reflog から「推定」として表示します。失敗や拒否で何も動かなかった操作は「動いた ref はありません」と記録されます。以前の Operation Log もそのまま読めます。repository への書き込みはなく、Operation Log の各行に 1 項目増えるだけです。時点への復元や 1 操作の取り消しはこの記録を前提に今後追加します。(Refs #334、ADR-0214)
- branch 右クリック menu の「Sync」に「Sync to remote (keep local)…」を追加（#536 slice 2）。現在の branch で upstream がある場合のみ有効。既存の branch plan card に先行 commit 数・保全する変更数（staged / unstaged / untracked）・ignored は不変の警告と、`git update-ref` / `git stash apply --index` の復元 2 コマンドを表示し、二段 confirm（ADR-0023）で実行します。（Refs #536）
- settings.json の `worktree_run_mode` に `"nonconcurrent"` を追加しました。DB が 1 つしかない・callback URL が固定、のように並列に動かせないプロジェクト向けです。この設定では、同じ repository の worktree のうち、埋め込み terminal の shell を動かせるのは 1 つだけになります。別の worktree で terminal を開こうとすると起動せず、terminal 欄・footer・toast に、動作中の worktree 名を添えて理由を出します(`[kagi] terminal: nonconcurrent blocked <path> (running in <path>)`)。その shell が終了すれば起動できます。既定は従来どおり `"concurrent"` です。Kagi の terminal 以外で起動したプロセスは数えません。(#859、Refs #342、ADR-0213)
- サイドバー WORKTREES の各行に、その worktree に割り当て済みの port block の先頭を `localhost:<port>` として表示するようにしました。クリックすると `http://localhost:<port>` をブラウザで開きます。表示は割当を読むだけで、新しい割当はしません(まだ terminal を開いていない worktree には出ません)。terminal を開いて割り当てられた port は、その場で表示に反映します。(#855、Refs #342)
- sync-to-remote の backend（#536 slice 1、ADR-0215）。`Operation::SyncToRemote { branch }` は、ローカル branch を取得済みの upstream に揃える前に、旧先端を `refs/kagi/backups/<op>/0` に、index と作業ツリー（untracked を含む）を `git stash apply --index` で戻せる stash 形式の commit として `refs/kagi/backups/<op>/1` に保全します。ignored ファイルには触れず、untracked は保全した blob と一致するものだけを個別に取り除きます。upstream 未設定 / 未 fetch / detached / 進行中操作 / conflict / 別 worktree で checkout 中 / 追従済みは blocker、先行 commit 数と保全する変更数は warning。git2 のみで実装し、UI はまだありません。（Refs #536）
- Operation Log の各行に、誰が実行したか(人 / MCP / CLI)のバッジと、どの worktree で行ったかのバッジを付けました。行を選ぶと、これまでの before / after / error に加えて、その操作の時間帯(同じ worktree の前の操作以降)に動いた reflog 行(HEAD と各ブランチ)を表示します。同じ worktree の別の操作と同じ秒に記録された行は、どちらの操作のものか判別できないため「別の操作と同じ秒」と明示し、どちらにも帰属させません。読み取りだけで、repository への書き込みも Operation Log への追記もしません。時点への復元や 1 操作の取り消しは今後の slice で追加します。(Refs #334、ADR-0214)
- branch 右クリック menu の「Integrate」に「Replay <branch> onto <current>…」を追加（#344 slice 2、ADR-0211）。既存の rebase 確認 card を流用し、`git replay` が印字した ref 更新（warning「Updates N ref(s)…」）、署名消失・別 worktree の index が古くなる警告、merge / conflict / dirty worktree / 追従済みの blocker を表示します。履歴の書き換えなので二段 confirm（ADR-0023）。実行後は branch の ref だけが動き、どの worktree にも触れません。（Refs #344）
- `git replay --onto` による「worktree を汚さない rebase」の backend（#344 slice 1、ADR-0211）。`Operation::ReplayOnto { branch, onto }` は、別の worktree で checkout 中の branch を含め、作業ツリーと index に触れずに ref だけを動かします。plan は git 自身が印字した `update <ref> <new> <old>` をそのまま載せ、execute は `git update-ref --stdin` の 1 トランザクション（old 値は git が照合）、旧先端は `refs/kagi/backups/` に退避して oplog に記録します。merge を含む範囲・conflict・checkout 中の worktree が dirty・既に追従済みは plan の blocker です。git のバージョン検出（`cli::git_version` / `GitFeatures`、起動時に `[kagi] git: version=… replay=… revert=… history=… fixup=…`）を追加し、2.53 以降で replay が既定で ref を書く仕様差は常に print モードで吸収します。UI はまだありません。（Refs #344）
- worktree 自動ロックの解除を、ロックファイルを一旦退避(rename)してから中身が自分の token のときだけ消す compare-and-unlock にしました。解除の直前に別の git が別の理由でロックし直していても、そのロックは元に戻して解除を拒否し、他人のロックを消しません。戻す先に新しいロックができていた場合は上書きせず `locked.kagi-*` として残し、次の自動解除はそれを理由に止まり、手動の unlock カードに「中身を確認して `locked` に戻すか削除」と表示します(自動では消しません)。(Refs #772、#836、ADR-0212)
- PR mode の diff に review thread を重ねて表示するようにしました。thread のある行の横(gutter)に件数バッジが付き、クリックでその行の直下に thread を折り畳み表示します(もう一度で閉じ、閉じれば行の並びは元どおり)。side-by-side 表示では thread の面に応じて左右の gutter に出します。古い位置(outdated)の thread は薄く表示し、解決ボタンは出しません。取得は従来の line comment の REST 呼び出しを GraphQL の review thread 1 本に置き換えたもので、gh の呼び出し数は変わりません。取得に失敗したときは空として黙らず、理由を `[kagi] pr-threads:` の log に 1 行出します。(Refs #351、ADR-0209)
- 埋め込み terminal の worktree 自動ロック(Phase 1、既定 OFF の opt-in、macOS / Linux)。Settings の「terminal を開いている間 worktree をロック」を ON にすると、linked worktree で terminal を起動したときに Kagi 所有 token(`kagi:auto:<session>`)付きの `git worktree lock` の確認カードを出し、shell 終了(render に依存しない wait で観測)時にそのロックの解除カードを出します。confirm するまで何も書かず、手動のロック・他の terminal のロック・別の worktree のロックは plan と preflight の両方で拒否して触れません。crash 後に残ったロックは従来どおり手動解除で回復します。（#772、ADR-0208）
- PR mode のファイル一覧に「確認済み」の checkbox と「N / M viewed」(JA「N / M 確認済み」)の進捗を追加しました。確認済みの行は薄く表示されます。印はそのときの head 側のファイル内容(blob)に紐づき、PR の head が進んでそのファイルが変わると自動で未確認に戻ります(変わっていないファイルは確認済みのまま)。状態は `~/.kagi/pr-viewed/` に PR ごとに保存し、壊れたファイルは上書きせず退避します。Operation Log には記録しません。(#351、ADR-0207)
- PR のレビュー会話で ```suggestion 付きの行コメントに「提案を適用…」(EN「Apply suggestion…」)ボタンを追加しました。押すと plan card を開くだけで、確認すると作業ツリーのファイルだけを書き換えます(stage も commit もしません)。レビューの行番号は PR の head 側のものなので、作業ツリーのファイルが PR head の版と一致するときだけ適用でき、ローカルで変更されている・head がローカルにない場合は理由を示して拒否します。適用前の内容は `refs/kagi/backups/` の backup ref に保持して Operation Log に記録し(`git cat-file blob <backup-ref>` で読み戻せます)、CRLF や末尾改行なしのファイルも範囲外の行はそのまま残します。(#351、ADR-0210)
- Commit Panel から開いた diff の hunk header に「Stage hunk」/「Unstage hunk」(JA「hunk を stage」/「hunk を unstage」)ボタンを追加しました。unstaged 側では押した hunk だけを index に入れ、staged 側では押した hunk だけを index から戻します(working tree は変更しません)。split view でも header 行に同じボタンが出ます。押した後は diff を読み直し、その側に何も残らなければ pane を閉じます。diff を表示した後にファイルや index が変わって hunk が一致しなくなった場合は、別の hunk を代わりに stage せず理由を示して拒否します。失敗は従来の Stage / Unstage と同じく Operation Log・footer・通知に出ます。行単位の選択や hunk の分割は含みません。(#842、Refs #357)

### Fixed

- Graph サイドバーで高さが異なる LOCAL / REMOTE 等の境界をドラッグし始めると、separator が pointer からずれる問題を修正しました。header を含む実測 pane 高から重みを求め、初回の小さな移動から pointer に追従します。（#911 review、ADR-0217）

- fetch の失敗、PR の comment / review / edit、issue の作成・コメント、worktree の lock / unlock / prune / repair / 削除を含む時点への「この時点まで戻す」が、「記録なし」で拒否されていた問題を修正しました(#885、ADR-0214 §4)。これらの操作も、動かした branch を Operation Log に記録します。fetch や worktree の操作は、実行の前後で branch を読んで実際の移動を記録します。mirror 型の設定で fetch が local branch を動かした場合や、worktree の削除で branch も消した場合は、その移動が記録されます。GitHub 側だけを書き換える操作は「動いた branch なし」と記録します。実行した PR merge は local branch を消すことがあるため、終了が確認できない操作と同様に、これまでどおり記録なしで扱います。(Refs #334)
- 「この時点まで戻す / 取り消す」で、直近 1000 件より古い操作を選ぶと「この repository の操作にない」と表示されていた問題を修正しました。古すぎて範囲外であることを示すようにしました(戻せないことは変わりません)。また、プレビューできないときの文言で、「commit をさらに読み込む」で表示できる場合と、どの branch からも届かず表示できない場合を案内するようにしました。(#888、Refs #334)
- 「この時点まで戻す」が、削除した worktree や、削除した無関係の repository で行った操作を範囲に含むだけで拒否されていた問題を修正しました(#894)。Operation Log の各操作に、記録した repository(common dir と、unix ではそのファイル ID、取れる環境では `.git` の作成時刻)を残すようにしたので、worktree が消えていても、この repository の操作か別の repository の操作かを判定できます。削除した worktree での操作は戻す対象に含まれ、無関係の repository の操作は除かれます。削除して同じ場所に clone し直した repository は、古い `.git` と同じファイル ID を得ても作成時刻で区別し、作成時刻を確かめられなければ拒否します。この記録の無い以前の操作は、これまでどおり判定できなければ拒否します。repository を別の volume に移動した場合のように判定できないときも、黙って戻さずに理由を示して拒否します。(Refs #334)
- 「この時点まで戻す / 取り消す」が HEAD の切り替えを含む範囲で拒否されるとき、どの操作で HEAD が何から何に切り替わったか(branch、または detached の commit)と、戻す手順(先に元の branch / commit を自分で checkout してから、その操作以降の時点へ戻す)を示すようにしました。HEAD を含む復元は、作業ツリーに触れるため引き続き行いません(ADR-0214 §7)。(#886、Refs #334)
- 既定 OFF の opt-in「terminal を開いている間 worktree をロック」を ON にした場合の自動ロックを修正しました。これまでは acquire の確認カードを Cancel しても、その提案の token を所有済みとして記録し、shell 終了時に同じ reason の別人のロックを解除提案できました。acquire 成功後だけ所有権を保存し、shell が承認前に終了したらカードを閉じます。解除は元の tab / shell 世代 / repository identity が一致するときだけ提示し、別 tab のカードは上書きせず元の tab まで保留します。token は再起動後に再利用されない random nonce を含み、観測不能や crash 後は従来どおり手動で確認して解除します。既定 OFF の利用者の動作は変わらず、ON でも lock / unlock はそれぞれ確認が必要です。（#772、ADR-0218）
- 自動ロック取得後、shell 終了時に別の確認カードが開いていた場合、そのカードをボタンで閉じてもロック解除の確認カードが出ない問題を修正しました。modal slot が空いた後に元の tab へ再提示し、解除カード自体を Cancel した場合は再提示しません。（#914 review、#772）
- Commit Panel を同じ worktree で開き直すか、merge 後に再読込した際に、file tree の兄弟位置が以前のファイル構成のまま残り、新しい行から支援技術向けの TreeItem が欠落する問題を修正しました。新しい状態に差し替えるたびに位置表を無効化します。（#901 review、Refs #354）
- Operation Log の「取り消す / この時点まで戻す」の確認 card と Operation Log の review 指摘を修正しました(#883 / #871 / #878)。
  - 「戻した後のグラフ」が長いと card の下側が切れて見えなかった問題を修正しました。行は card 内でスクロールします。
  - branch Solo 中や、PR head(`refs/kagi/pr/**`)だけが保持する commit があるときに、どの branch からも外れる commit の数を誤って表示していた問題を修正しました。
  - プレビューできない場合に「消える commit はありません」と表示していた見出しを中立にしました。
  - card の「Copy all」に戻した後のグラフも含めるようにしました。
  - Operation Log の行の「コピー」に、記録された ref の移動(OID 全桁)を含めるようにしました。
  - absorb と merged branch の一括削除も、動かした ref を記録するようにしました(これまでは「推定」表示で、時点への復元の根拠にできませんでした)。(Refs #334)

- 確認 card・commit / PR / WIP 一覧・サイドバーで、支援技術に伝える名前や状態が画面と食い違う問題を修正しました。linked worktree の amend は対象名を読み上げ、warning / blocker は note / alert、conflict Abort は次の確認で実行されると説明します。名前に含まれる `{}` や制御文字は崩さず安全に表示し、サイドバーの兄弟位置は行更新時に計算します。commit 一覧の「さらに読み込む」はボタンとして操作できます。（#354、#872、#876、#879 review）
- commit 履歴が打ち切られた際の「さらに読み込む」ボタンを、支援技術の list box 内の選択肢ではなく、その下の独立したボタンとして表示するようにしました。ボタンは打ち切り中だけ表示し、クリックで従来どおり履歴を追加します。（#896 review、Refs #354）
- worktree の削除と `nonconcurrent` モードの判定で、shell が動いたままの terminal タブを閉じると、その shell を終了済みとして扱っていた問題を修正しました。タブを閉じても shell が hangup を無視して動き続けることがあるため、shell の終了を実際に観測するまでは動作中として扱います。shell の終了待ち自体が失敗した場合も、終了扱いにはしません。(#867 / #869 の review 指摘)
- Operation Log の「この時点まで戻す」が、正確に戻せない範囲でも成功していた問題を修正しました(#878 の review 指摘)。次の場合は理由を示して拒否します。
  - Operation Log の途中の記録が消えている・読めない場合。
  - 範囲に、削除された worktree で行った操作がある場合(この repository の操作だった可能性があるため)。
  - その時点の後に Kagi の外で作られた・動かされた branch がある場合(戻しても残るため)。
  - いずれかの worktree で merge / rebase などが進行中の場合(これまでは呼び出した worktree だけを見ていました)。

  あわせて Operation Log パネルの固定文言を `Msg` のキーに移しました(表示は同じ)。(Refs #334)

- 色覚対応テーマを View → Theme メニューから選べない問題、UI 言語切替後に Settings のテーマ名だけ古い言語のままになる問題、開いた Commit Panel の WIP 行が支援技術では未選択になる問題を修正しました。（#354、#889 review）
- 埋め込み terminal の `KAGI_WORKTREE_PATH` と `KAGI_MAIN_WORKTREE` が、git の workdir をそのまま使っていたため末尾に `/` が付いていた問題を修正しました(main worktree・linked worktree とも)。`"$KAGI_WORKTREE_PATH/foo"` が `//foo` になりません。(#870)
- Cmd+R(Refresh)の読み直しが捨てられ、Kagi の外で変えた状態(起動後に置いた worktree lock の 🔐・右クリックの Unlock など)が画面に反映されない問題を修正しました。Refresh は読み直しの直後に fetch を始めますが、fetch の受付が実行中の読み直しを無効にする一方、何も取得しなかった fetch は読み直しをしないため、Refresh の読み直しが失われていました。fetch は、自分の受付で無効にした読み直しを、取得の有無や失敗に関わらず完了時にやり直します。自動 fetch が watcher の読み直しと重なった場合も同じです。(#851)

- worktree の port block(マシン全体で既定 `3000-3099` / 10 ずつ = 10 block)が尽きると、埋め込み terminal 自体が起動しなかった問題を修正しました。枯渇時は `KAGI_*` を渡さずに terminal を起動し、footer と toast に理由と、`settings.json` の `worktree.port_range` を広げれば割り当てられることを示します(`[kagi] terminal: port block exhausted <path> (range <start>-<end>, per <n>)`)。既定の range は変えていません。あわせて ADR-0171 の「2 つの repo が同じ番号を出しうる」という記述を、全 repo で 1 つの store を共有して番号が重ならない現行の実装に合わせて訂正しました。(#852、Refs #342)

- PR の merge 状態(mergeStateStatus・未解決 thread 数・merge queue の位置)が一度も読めていなかった問題を修正しました。GraphQL query を `\` 行継続で書いていたため、継続時に次行の字下げが消えて `mergeStateStatus` と `reviewThreads`、`state` と `mergeQueue` が 1 語につながり、GitHub が query ごと拒否していました。query を改行区切りにし、読み取りに失敗したときは黙って空にせず `[kagi] pr-merge-status: #N read failed: <理由>` を 1 行出します。同じ書き方の再発を防ぐ CI gate `check-string-continuation` も追加しました。(#843)

- コミットのファイルを開いたとき、未キャッシュの diff を UI スレッドで読んでいたため大きな diff で画面が固まる問題を修正しました。読み込みは Compare / WIP と同じくバックグラウンドで行い、読み終わるまで表示中の diff はそのまま残り、2 秒を超えると busy snackbar が理由を示します。読み込み中に別のファイルを開いた・閉じた場合は古い結果を捨て、reload で行番号が変わった場合はコミットの新しい行に付け直して表示します。（#829）

- File History で先頭の WIP 行を選んだとき、右の詳細ペインの「Changes」が「+0 −0」と表示される問題を修正しました。WIP 行は `git status` から作られ行数を持たないため、行数が分からない項目では「Changes」行を出さず、実際の +/− は下の diff pane が示します。コミット行の +/− と binary 表示は従来どおりです。（#813）

### Added
- Analyze に「Health」タブを追加しました。commit-graph が無い / HEAD より古い、`core.fsmonitor` が未設定(macOS / Windows)を検出し、EN/JA の説明と「有効化…」ボタンを表示します。ボタンは plan(等価な git コマンドと戻し方を含む)を開くだけで、確認するまで何も書き込みません。確認すると `git commit-graph write --reachable` または local config への `core.fsmonitor=true` を実行し、Operation Log に記録します。（#358、ADR-0205）
- 2 秒を超えた読み込み(ahead/behind の計算・worktree の状態・worktree 容量の計測・Analyze・Compare / WIP / File History の大きい diff)について、busy スナックバーに「時間がかかっています: <理由>(大きいリポジトリでは <対象> に時間がかかります)」を EN/JA で追記するようにしました。ahead/behind と worktree 容量は「スキップ」で計算をやめ、既存の「—」/「未計測」表示にできます(次の読み込みで再計算、Operation Log には記録しません)。2 秒未満の読み込みでは何も出ません。(#355、ADR-0208)

### Fixed

- コマンドパレット(Cmd+P)から開いた Push などの確認 modal で Enter も Escape も効かず、Cancel のクリックでしか閉じられなかった問題を修正しました。パレットを閉じるときは開く前の focus へ戻し、Push / branch の Push・Pull の確認 modal は開くときに focus を画面本体へ移します（起動時に端末が focus を持っていても Escape で閉じます）。blocked な Push は branch menu から開いた場合と同じく、Enter で拒否されて理由が footer に出て、Escape で閉じます。（#817）
- Settings を × または Escape で閉じた直後に、↑↓ が File History / graph の一覧に届かなかった問題を修正しました。Settings のテーマ選択などで移った focus を、閉じるときに開く前の場所へ戻します。（#812）

### Changed

- 実行できない plan（blocker あり）を Enter や確認ボタンで確定したとき、footer と toast が「refused (N blockers)」の件数だけでなく、先頭の blocker の具体的な理由を表示するようにしました（EN/JA、残りの件数も併記）。checkout・branch 操作・merge / cherry-pick / revert・reset / rebase / force-with-lease push・remote branch 削除 / tag push・discard・undo / redo・worktree lock / unlock / prune・pull / push・PR merge / review / 編集・stash・remote stash drop が対象です。Operation Log には従来どおり全 blocker が残り、実行は従来どおり拒否します。（#353）

### Fixed

- 狭い window や高い zoom(例: 1000px・125%)で、Main Diff のヘッダ(Back・外部エディタで開く・History・表示切替・+N −M)と File History のヘッダ(Back・Refresh・Copy Path・Open File・Follow Renames)のボタンが中央 pane からはみ出して押せなくなっていたのを直しました。ファイル名を先に省略し、それでも足りないときはボタンをアイコン表示にして名前は tooltip で示します(File History のブランチ名とコミット数はタイトルの tooltip へ移ります)。(#809)

### Performance

- WIP・Compare・File History の diff を開いたとき、diff の読み込みと構文ハイライトが UI スレッドを止めないようにしました。まずテキストを表示し、ハイライトは別スレッドで計算してから反映します。別のファイルや別のテーマに切り替えた後に古いハイライトが反映されることはなく、同じ内容の再読み込み（外部変更による reload など）ではハイライトも side-by-side の再計算も行いません。（#495）
- File History のコミット一覧を、Graph・sidebar・Editor History と同じ `uniform_list` による仮想リストに揃えました。500 件の履歴でも構築される行は表示範囲分だけになり、選択行は一覧の外にあってもスクロールして表示します。見た目・ページング・diff 対象の選び方は変更しません。（#496）
- コミットグラフの「さらに読み込む」と手動 Refresh(ツールバー)の snapshot 読み取りを UI thread から background へ移しました。結果は読み取りを要求したタブ(owner)で、かつその要求が最新のときだけ適用し、連打・Refresh との競合・タブ切替で古い結果を反映しません。読み込み失敗は footer と toast で通知し、選択・スクロール・開いているパネルは維持します。（#487）
- Inspector の changed files（先頭 100 件の切り出し・生成ファイルの折り畳み・tree・diffstat の対応付け・件数集計）とコミットメッセージの HTML 変換を、描画のたびではなく入力が変わったとき（選択・読み込み完了・reload・compare）だけ作り直すようにしました。Path⇄Tree の切り替えと Generated の開閉では作り直しません。表示・クリック先・右クリックメニュー・Copy Path・「… and N more」は従来どおりです。（#512）

### Internal

- GUI E2E runner が、各 scenario の前後で共有状態を比較するようにしました。
  - 比較するのは `settings.json`(実行時の session 系 key を除く)、port store、oplog(既存行の不変と追記先)、runner 専用 `TMPDIR` 直下の entry です。
  - 差分があれば、どの資源が何から何に変わったかを列挙して、その scenario を失敗として扱い、証跡を残します。
  - 全 scenario を分割実行して見つかった違反は直しました。言語・diff_split・ui_zoom・theme・terminal_auto_lock の保存 key の後始末(19 scenario)、port store の後始末(6 scenario)、共有 oplog の件数を repo で絞っていなかった `push_failure_keeps_modal` です。

  製品の動作は変更していません。(#516 slice 3)
- GUI E2E の 3 scenario(`cleanup_partial_presentation`、`pull_refuses_when_the_dirty_set_moved`、`pull_completion_drops_when_its_tab_is_left`)の期待値を現行の提示仕様に合わせました。#718 で失敗した pull の確認画面を開き直さなくなり、#747 で記録済みの結果は toast と Operation Log で示すようになり、背景タブの結果は repository 名付きで出るようになっていました。これらの PR で期待値の更新が漏れていたものです。安全性の検査(何も stash / pull しない・local branch を変えない・記録は 1 件・確認画面を再表示しない)は弱めていません。製品の動作は変更していません。(#898)
- GUI E2E runner で scenario が失敗したとき、`target/gui-e2e/<scenario>/` に失敗証跡を残すようにしました。中身は panic の内容、直近 200 行の `[kagi]` ログ、mount した fixture repository の `git status --short` と `git log --oneline -5`、window の PNG(撮れない場合は理由を書いた `window.txt`)です。stderr には `[gui-e2e] FAIL <scenario>: evidence <dir>` を 1 行出します。window は前面にも画面内にも出しません。終了コードと「最初の失敗で止まる」挙動は変わりません。製品の動作は変更していません。(#516 slice 1)
- GUI E2E の scenario 間の隔離を監査し(#516 slice 2)、違反を直しました。
  - fixture の外に作っていた worktree / bare repo(3 scenario)を TempDir の中に移しました。
  - 背景で残る `sleep` を待たずに終わっていた pull scenario は、それが終わるまで待つようにしました。
  - theme(2 scenario)、`graph_copy_target`(settings.json 全体を上書きしていた)、言語と保存済みキー(3 箇所)、PR の viewed 記録、`PATH` の復元を、元の状態に戻すようにしました。
  - `gh_available` の process 内 cache が偽の `gh` を置いた `PATH` で決まらないよう、置く前に確定させるようにしました。
  - 共有 oplog の件数を 100 件の窓で数えていた箇所は、log 全体で数えるようにしました。

  製品の動作は変更していません。(Refs #516)
- GUI E2E `cross_worktree_merge` を #722 P1 r3（03b16092）以降の仕様に合わせて修正しました。別 worktree への drag merge は editor の未保存変更を確認せず、元の tab の editor は変更を保ったまま残ります（← Graph では従来どおり確認）。ADR-0144 の記述も更新。（Fixes #880）
- Toolbar の利用可能状態 → AccessKit `disabled`(#797 で実装済み)の検証を追加しました。`ButtonState` → (表示, disabled) の pure な変換を切り出して unit で固定し、GUI E2E `toolbar_a11y_disabled` で remote なし fixture の Push / Pull / Stash / Pop が disabled、Branch / Settings が enabled、Terminal は on/off どちらでも disabled にならないこと、dirty にすると Stash が enabled に転じることを確認します。製品の動作は変更していません。（Refs #354）
- ADR-0211: `git replay` / `git history` を plan パイプラインに載せるための調査と設計（実装なし）。git 2.50.1 で `git replay` を実測し（worktree / index に触らない、出力は `update-ref --stdin` 形式で `<old>` が CAS、他 worktree の branch を rebase できるがその index が古くなる、merge を含むと exit 128、conflict は exit 1 で状態なし、hooks は走らない）、2.53 で replay が既定で ref を更新するようになった事実を含む版ゲート（(a) 隠す、検出は kagi-git に 1 回、`Backend` が保持、experimental は設定で隠す）を提案しました。（Refs #344）
- headless 起動 hook `KAGI_SELECT_FIRST=1`（`select_headless`）が inspector の changed files を同期で埋めるとき generated/lockfile flags を計算していなかったため、その経路では `Cargo.lock` が「Generated (N)」に畳まれませんでした。通常のクリック選択（非同期 read）では起きず、ユーザー操作への影響はありません。同期経路も files / diffstat / generated flags の 3 つを揃えて埋めるようにし、Tier A に SELECT_FIRST 経路の fold assert を追加しました。（#818）
- 統合テストの fixture 用 `git`（`tests/support/git_fixture.rs`）が `git maintenance run --auto --detach` を起動しないようにしました（`maintenance.auto=false` / `gc.auto=0`）。`git commit` 直後に detached process が残す `.git/objects/maintenance.lock` を CI の snapshot 比較が拾って落ちていた flake（`fixture_is_identical_under_a_hostile_home`）の原因で、index の racy 書き換えではありませんでした。製品の動作は変更していません。（#819）
- 統合テストの Git fixture 構築を `tests/support/git_fixture.rs` に共通化しました。fixture 用の `git` は継承した `GIT_*`・global/system/XDG 設定・hooks・template を遮断して identity を固定するため、開発者の設定や hook から渡された `GIT_DIR` で fixture の内容が変わったり実 repository に書き込んだりしません。ops / blame / push_tag / pr_conflict_preview / app_read の各 suite を移行し、残りは follow-up で移行します。製品の動作は変更していません。（#514）
- #514 の follow-up: 残り 72 ファイル（`tests/*.rs`・`tests/recovery/*.rs`・`tests/support/pr_merge_local.rs`）の suite-local な `git` helper を `tests/support/git_fixture.rs` に寄せました。テスト数 713・assertion は移行前後で同一で、明示 opt-in の 5 suite は従来どおりです。製品の動作は変更していません。（Refs #514）
- Tier A `push_failure_keeps_modal` を、#747 で変わった通知の契約（記録済みの失敗は Operation Log と footer / toast で伝え、閉じるだけの AppNotice は出さない）に合わせました。#747（e5644c6f）以降このシナリオは古い期待（AppNotice の queue）のまま落ちていました。Remote Browse の入力を失わないことは従来どおり確認します。製品の動作は変更していません。（#824）
- Tier A `worktree_panel_amend_discard` の (c) を ADR-0084 の reflog seed の契約に合わせました。#773(4e0d6567)で共有 fixture の main に commit が積まれて以降、tab の Undo の先頭は main 自身の reflog entry(undo 可能)になり、それ以前は初期 commit しかなく Undo が拒否されて何も動かなかったという前提に依存した「どちらの repo も動かない」の期待で落ちていました。worktree の amend が tab の undo stack に漏れないことは、Undo の先頭が main 自身の entry であること、Undo は main だけの soft move で worktree A / B は不変であること、Redo で main が元に戻ることで確認します。製品の動作は変更していません。(#849)
- Tier A `stage_failure_notice` を #747 の通知の契約に合わせました。index.lock 下の stage / unstage 失敗(editor・panel・一括ボタン、linked worktree の panel を含む)は Failed footer と Error toast と Operation Log で伝え、閉じるだけの AppNotice は出さないことを確認します。#747(e5644c6f)以降、このシナリオは古い期待(AppNotice)のまま落ちていました。owning repo/path・index 不変・oplog 記録の確認は従来どおりです。製品の動作は変更していません。(#846)
- backend 専用の統合テスト 61 suite（ops / discard / absorb / conflicts / pull / push / stash / worktree / oplog など）を `tests/` から `crates/kagi-git/tests/` へ、純粋 logic の 2 suite（message_template / trailers）を `kagi-domain` の unit test へ移しました。`cargo test -p kagi-git` だけで主要な mutation / preflight / recovery の契約が走り、GPUI を含む root を build しません。テスト数は移行前後で同じ（2650）で、製品の動作は変更していません。（#515）

## [0.40.1] - 2026-09-29

### Fixed

- 登録だけ残り作業ディレクトリが削除された worktree があると、無関係なブランチの削除まで `failed to resolve path` で失敗する問題を修正しました。消えた worktree は管理ディレクトリから HEAD を読み、そのブランチは `git branch -D` と同じく引き続き削除を拒否します。（#802）
- `origin/master` などの別名ブランチを upstream に持つローカル専用ブランチを Push すると、remote には載るのに upstream が変わらず、毎回「pushed N」が出て完了しないように見える問題を修正しました。この場合は `push -u` で `origin/<branch>` に公開し、upstream をそこへ移します（VS Code の Publish と同じ）。ブランチメニューの Push も同じ扱いで、初回 push のコミット数は remote に既にある分を数えなくなりました。（#803）
- 縦長画像の diff で画像の上下が見切れる問題を修正しました。Before/After の各列で、ラベルの下の領域に縦横比を保って収めます。（#804）

## [0.40.0] - 2026-09-23

### Added

- Toolbar と sidebar navigation に accessibility role と名前を付与し、sidebar の既存の選択状態を公開しました。modal footer は既存 Button の意味情報を維持します。VoiceOver・確認ダイアログ・リスト全体の対応は含みません。（#354）
- Embedded terminal に worktree 固有の `KAGI_WORKTREE_PATH` / `KAGI_WORKTREE_NAME` / `KAGI_MAIN_WORKTREE` / `KAGI_DEFAULT_BRANCH` / `KAGI_PORT` を渡すようにしました。親 shell の値は上書きし、再起動でも保存済み port を再利用します。port 枯渇や metadata 取得失敗は既存の terminal 起動失敗として表示します。localhost link と nonconcurrent は対象外です。（#342、ADR-0171）
- Worktree のロック理由を自由入力できるようにしました。入力後に計画を確認してからロックし、日本語・引用符を含む理由や理由なしのロックに対応します。入力画面や計画画面の Cancel / Esc ではロックしません。terminal の自動ロックは別 Issue #772 で追跡します。（#372）
- WORKTREES の行に占有容量と Git 上の削除条件を表示し、クリックで `target/` 内訳・計測時刻・EN/JA の理由・手動再計測を確認できるようにしました。ignored ファイルも background で計測し、選択変更や tab 離脱で古い計測を破棄します。判定は clean・lock なし・merge 済みまたは push 済みを取得済み ref で確認する参考情報です。ignored 内容の警告と既存の削除メニューへの案内を表示し、削除・backup・lock の動作は変更しません。（#633、ADR-0175）
- WIP 行に「次のコミットが載る点」を追加しました。HEAD の lane 色で中空 ring と badge→ring→HEAD の点線を描き、同じ HEAD の複数 WIP は一本の縦線を共有します。ring は実 commit の表示径に揃えた 2px stroke で、hover／選択でも塗りつぶしません。detached HEAD にも対応し、未ロード／unborn の HEAD に架空の点は描きません。（#767、ADR-0174）
- Issues / PRs の一覧に共通フィルターを追加しました。Open/Closed/All、複数ラベル、author、タイトル部分一致を組み合わせ、PR は draft と取得済み checks でも絞れます。updated / created / number / comments の昇降順、適用後の件数、既存の collection との AND に対応し、設定はタブ内だけで保持します。Closed は実際の closed 一覧を取得し、PR の merged も含みます。（#753、ADR-0198 / ADR-0200）
- PR の Closed/All は専用の読み取り結果として保持し、Graph の open PR 一覧・branch badge・inspector chip・定期更新には混入させません。PR mode を離れると state は Open に戻ります。closed PR は actionable な bucket から外し、Issues は Recent を既定とし、絞り込み中の追加ページ取得は明示操作にしました。（#753 review follow-up）
- LOCAL / REMOTE の branch 行に、その branch の先端 commit の日時を相対表記（`· 3d ago`）で表示し、tooltip でフルの branch 名と絶対日時を確認できるようにしました。表示は読み込み済みの commit の committer 日時で、branch を古い commit に貼り替えれば古い日時のままになります（ref を動かした時刻や fetch 時刻ではありません）。読み込んでいない先端は `—` と表示し、理由は断定しません。Git の追加読み取り・追加 fetch は行いません。幅が足りないときは日時から先に省略され、branch 名・`↑↓`・PR バッジ・`×` と 24px の行高は保たれます。タグ行と graph の日付列（author 表記）は対象外です。（#358）

### Fixed

- Conflict の Continue 判定用 marker 状態を Result 編集と同期して保持し、判定時の再描画ごとの全文連結・走査をなくしました。同じ手編集を再描画で undo stack に積み直すことも防ぎます。未解決・binary・deletion の優先順位と、保存／実行前の fresh validation は維持します。（#497）
- Worktree 削除の初期 blocker が `plan has blockers` に潰れる問題を修正しました。具体的な全理由を oplog に残し、先頭の typed blocker を既存 EN/JA の通知と toast に表示します。削除条件・確認・安全検査・他 family は変更しません。（#353）
- Avatar の map を File History / Editor と共有し、PR conversation / lane は借用するようにしました。描画ごとの map・key の複製をなくし、新しい解決 batch が届いたときだけ snapshot を更新します。取得方法・画像の fallback・Git 操作は変更していません。（#498）
- Toolbar の既存の利用可能状態を AccessKit の disabled に反映する処理を追加しました。Terminal の選択状態とは区別し、無効時のクリック理由 footer と Undo/Redo の busy guard は変更しません。native AXEnabled は main でも AXWindow が取得できない環境のため未検証で、次回セッションで確認します。（#354）
- Issues 一覧の filter/sort と sidebar 4タブの件数を session 所有の派生キャッシュで共有し、スクロール描画ごとの全件再計算をなくしました。追加ページ・同件数の更新・filter/tab/login/mentions 変更で再計算し、仮想 list には表示順を `Rc` で渡して毎フレームの全件コピーも避けます。取得・ページング・表示順の契約は変更していません。（#791）
- resolution buffer の autosave JSON を serde で保存・読み込みするようにし、surrogate pair で表現された文字が解決草稿から脱落する問題を修正しました。既存の schema、未解決と空テキストの区別、行 provenance、raw OID＋mode、欠落時の既定値を維持します。保存先・書き込み方式・解決操作・oplog は変更していません。（#513 resolution slice）
- draft の外側 JSON record を serde で保存・読み込みするようにし、surrogate pair で表現された文字が commit-message draft から脱落する問題を修正しました。既存のフィールド・省略時の値・保存先・atomic replacement は維持します。Issue 本文 payload、oplog、resolution の形式は変更していません。（#513 draft slice）
- conflict Skip の結果が `Unclear` のとき、結果分類より先に writer lease を解放していた問題を修正しました。停止済みでも sequencer の結果が不明な場合は lease を保持して既存の reconcile 経路へ渡し、停止未確認の `TerminationUnknown` も `Unknown` のまま扱います。Continue / Abort や read/ack の改修は含みません。（#569 (1)）
- 右クリックメニューのグループ間に区切り線を追加し、見出しのないグループも見分けられるようにしました。Worktree の削除2項目は、移動・Lock/Unlock・保守項目から分離して末尾に配置します。操作や確認画面は変更していません。（#454 Phase 3 の一部）
- Conflict Continue の実行後に状態を読み取れない場合、通常失敗として再操作を許可せず、結果不明として lease を保持し既存の照合に登録します。実行前の拒否・通常エラー・既知の `Staged` 結果の扱いは変更しません。（#569）
- Worktree inspection の review 指摘を修正しました。local ref に解決される upstream を push 済みの根拠にせず、Windows の圧縮ファイルは物理使用量を取得できない場合に不明とします。初回計測が途中で中断されても、tab 復帰時に cache を保持して未計測 worktree の走査を再開します。（#633 / #779）
- Remote Browse が接続フォームからディレクトリ表示へ移った後や、PR の reviewer / assignee / label 入力を Cancel / Apply で閉じた後に、非表示の入力へ focus が残って Esc が届かなくなるのを直しました。非表示になる入力の focus だけを root へ戻し、別の入力が既に得た focus は維持します。（#755 follow-up）
- conflict の Abort 確認が、reload で前提が変わったあとも古い内容のまま残っていたのを直しました。取り込んだ reload で確認を閉じ、内容の差し替えはしません（閉じるだけで、中止は実行しません）。開き直すと現在の状態で計画し直します。合わせて Abort 確認を開くときに window の focus を root へ戻すので、Result pane を編集して Preview に戻したあとのように focus が描画されていない要素に残っている状態でも Esc で閉じられます。（#755）
- WIP の点を HEAD と同じ lane の真上に置き、履歴との衝突で遠い専用列へ迂回しないようにしました。点線は実 commit・edge の背後を通ります。WIP / stash 行も commit 一覧と一緒にスクロールし、WIP が画面外でも可視の HEAD までは viewport 上端から点線が続きます。（#773、ADR-0174）
- preflight refusal の native 回帰テストを現在の通知契約へ更新しました。旧 dismiss-only modal の代わりに EN/JA の footer / error toast、oplog の具体的な拒否理由、repository 不変を検証します。製品の通知動作は変更していません。（#764）
- remote write の結果を観測できない場合、停止証明後に「未確認のまま制限を解除」を二段階で選べるようにしました。EN/JA の警告と解放理由の監査ログを残し、元の Unknown は保持します。実行中・結果不一致・通信失敗・ログ保存失敗では解除しません。SSH alias の実 host を期限付きで解決し、PR fetch の remote 識別にも使います。（#706、ADR-0196）
- 短いウィンドウでも確認ダイアログの対象リストと Cancel / Confirm を確認できるようにしました。zoom 換算800px以下では余白と見出しをコンパクトにし、復旧説明だけを既定で折り畳みます。警告・拒否理由・最終確認の注意文は表示領域内に保持し、対象は最後の行までスクロールできます。開閉の選択は高さ変更で反転せず、次の確認ではリセットされます。（#462）
- fork PR の merge 後も local branch を安全に扱えるようにしました。計画を background で作り、承認時の branch 名・full OID・不在を固定します。checkout 中・PR head 不一致は事前に「保持」と表示し、queue 投入時も branch を残します。gh 成功と server の merge 成立を両方確認した場合だけ既存の削除ガードを通し、後発の OID 変更・同名 branch 出現は削除せず Partial とします。receipt と EN/JA 通知に結果を残し、fork remote は削除しません。Unknown の照合で未着手 local branch の削除を要求することもありません。（#705、ADR-0202）
- conflict の Save / Abort が拒否された理由を EN/JA の通知に表示し、計画後の状態変化や conflict marker の残存を判別できるようにしました。footer のログ契約と oplog の英語詳細は維持します。（#711）
- Issues 一覧を最下行までスクロールすると次の100件を追加取得するようにしました。読み込み済み件数は続きがある間 `(N+)` と表示し、失敗時は一覧と cursor を保持して再試行できます。戻り・手動更新は先頭ページから取り直します。（#752）
- Issues / PR の本文とコメントの Markdown 描画を直しました。画像はリンクとして表示し取得しません（`![alt](url)` → `[alt](url)`、alt がなければ URL 自体をリンクに、参照画像も定義先の URL を保持）。URL に文字参照で改行などの制御文字を埋め込んでもリンクが壊れず、画像として復活しません（制御文字は percent-encode して 1 本のリンクに収めます）。HTML コメントは従来どおり非表示ですが、コード span や fence の中に書かれた `<!-- -->` は消えずに残ります。fence 内のコードに inline code 用の細空白が混入しなくなりました。4 本以上の backtick で囲んだ fence の中に書いた ``` も fence の終わりとは見なさず、その中のコードの改行をそのまま保ちます。Issues Thread・Composer の Preview・PR 会話は `kagi_ui_editor::markdown::prepare_github_markdown` の 1 経路を共有します。Editor の preview の画像表示は従来どおりです。（#751 / ADR-0142 追記）
- Issues / PR の本文で、コード span や fence に書いた `<!--` `-->` が `<!—` `—>` に合字化されず、書いたとおりの文字で表示されるようにしました。会話本文は contextual alternates（`calt`）を切って描画します。コード span は renderer 側に run 単位の hook がないため、地の文を含む本文全体で切れます（本文のどの文字も、原文の文字のまま描画されます）。Editor の preview の描画は従来どおりです。（#751 / ADR-0142 追記2）
- 新規 Issue の title に複数行を貼り付けると全文が title に潰れて body が空になっていたのを直しました。1行目から本文の既定タイトルと同じ規則で title を導き（`#` などの記法を剥がし、60 文字で打ち切ります）、残りを body に、Markdown と改行をそのまま（fence で包まずに）入れるので Preview でそのまま描画できます。1行目が空行や fence の開始で何も名付けないときは title 欄に触れず、貼り付けた全文を body に入れます。手で入力した title は従来どおり短縮しません。挿入は両方の入力欄の現在のカーソル位置に対して行い、入力済みの title / body を消しません。1行だけの貼り付けは従来どおり title に入り、本文欄への貼り付けの fenced block 化と Undo も変わりません。（#751 / ADR-0201 追記2）
- 操作の中断・拒否などを知らせる通知を、他の確認ダイアログと同じカード（角丸・境界線・見出し行・固定フッター）で表示するようにしました。見出しは種別を推測せず「操作に関する通知」の 1 種類で、本文（リポジトリのパスと理由）はそのまま残り、末尾に「Operation Log で操作の詳細を確認・コピーできます。」の 1 行を薄く添えます（保存済みであるとは述べません。コピー機能は Operation Log が持ち、通知には追加しません）。表示時間・閉じる操作・ボタンの文言と動作・キーボードの focus は従来どおりで、短いウィンドウでは既存の compact 表示に従います。（#792、#454 / #462）

### Changed

- Operation Log の手書き JSON codec を private serde DTO に置き換えました。既存 receipt の field・旧形式の id/parent 再構築・optional field の互換性を維持し、Unicode escape を正しく復元します。追記・lock・retention の保存方式は変更しません。（#513 oplog slice）
- 操作案内の EN/JA `Advice` カタログを全24 family に拡張しました。既存10キーに残り164キーを追加し、全224 Note variant と Git advice 43項目の対応・対象外を棚卸ししています。既存の表示文言・引数・警告／blocker の分類・英語の操作ログは維持し、blocker の書き換えや等価コマンドの追加は行っていません。（#353、ADR-0169）
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
