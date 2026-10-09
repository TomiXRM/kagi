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
- PR／Issuesで共用する `workspace_mode::sidebar_list_row` と `sidebar_section_header` は縦方向に縮めない。大量件数でもcardの自然なtitle＋metadataと見出しの高さを維持し、overflowは親sidebarのscrollで扱う。固定のpixel高さにはしない。（#1089）
- 戻り・手動更新は先頭ページを再取得し、cursor と scroll をリセットする。先頭ページ更新で generation を進めて古い追加取得を無効化し、detach 済み session の完了を別タブに適用しない。
- 未計測行には既存の最小行高を仮高さとして渡す。高さゼロの行が wheel / scrollbar の移動範囲を計測済み領域に制限することを防ぎ、描画時は実際の可変行高で置き換える。成功した一覧・追加ページは `klog!("github: issues page={} loaded={} has_more={}")` を出す。

## #753: session-owned 共通フィルター

- PR と同じ `list_filter_strip` を Composer の下に置き、state・複数 label・author・title 部分一致と updated/created/number/comments の昇降順を選ぶ。pure な `kagi_domain::list_filter` を共用し、既存の Assigned/Created/Mentioning/Recent collection と AND する。sidebar と main は同じ collection・predicate・sort を表示し、件数は適用後の読み込み済み行数とする。
- state は `issues(..., states:$states)` と `mentions:@me` の search scope に渡す。Open/Closed/All の変更では先頭ページへ戻り、request generation を進める。開始時の state を `github_issues_request_state` に固定し、追加ページは同じ state・repository・cursor を引き継ぐ。古い state の完了は新しい collection に append しない。
- label・author・title・sort は読み込み済みページだけに作用する。label/author/title または Recent 以外の collection が有効な間は、0件でも部分一致が残っていても明示的な「さらに読み込む」を使い、自動ページングしない。state は server collection、sort は順序なので、それらだけの変更では通常の自動ページングを維持する。
- filter intent と input/menu resource は既存の `TabUiState` に保持し、session を跨がず永続化もしない。sort/filter/tab の変更は main の `ListState` を先頭へ戻す。共通 policy と PR の追加条件は ADR-0200 の #753 追記に記録する。
- 既定の collection は `RecentlyUpdated` とする。viewer login 解決前でも取得した一覧を表示し、担当・作成・mention の選択時には従来どおり main/sidebar ともその集合で絞る。

## #791: render 間で共有する派生 Issue view

- filter/sort と4タブ件数は `TabUiState` の `RefCell<Option<IssueViewCache>>`
  に保持する。`TabUiState::default` で初期化し、`build_tab_view` へは複製しない。
  read と intent から再計算できる値であり、別の Issue 状態源ではない。
- キーは accepted Issue epoch・request generation・件数、共通 filter/sort、
  collection tab、viewer login、mention membership。accepted replacement /
  append は epoch を進める。同じ件数の差し替えも失効し、拒否された完了は触らない。
  refresh 開始や filter/tab/login/mentions の変更はキー不一致で失効する。
- miss 時だけ `apply_issues` を1回実行し、同じ結果から表示順と4タブ件数を作る。
  sidebar と main はそれを共有する。main の仮想 list closure は
  `Rc<Vec<usize>>` を保持し、全件 index Vec のフレームごとの複製を避ける。
  `RefCell` の借用は短い render 内の区間で終了し、closure へ持ち越さない。
- miss は `klog!("issues: view recomputed loaded={} visible={}")` で観測できる。
  native pagination 回帰は300件で実 wheel による viewport 移動を確認し、
  その描画で再計算0回、実 filter input の変更で main/sidebar 合計1回を要求する。
  ページング、sidebar の100行 cap、sort 順、失敗時の保持、GitHub transport は不変。

## #1091 (2026-10-09): accepted Issue conversation and managed selection

This amendment covers the **Issue Thread**, not PR conversation virtualization
or a new GitHub transport. It follows the existing session-owned read/UI
boundaries (ADR-0183/0197) and preserves ADR-0142's GitHub image-as-link and
literal-code preparation policy. It adds no write operation.

### Accepted projection and one scroll owner

- `TabUiState` owns an `Entity<IssueConversation>` with accepted posts and one
  variable-height `ListState`. Header, body/comments and production Reply
  composer/end share that outer list; offscreen posts retain text state rather
  than an eager column of rendered elements. A giant body remains complete,
  not a truncated preview.
- The detail loader's actual session, repository and request-generation checks
  decide acceptance. Beginning or failing a refresh keeps accepted content,
  selection and scroll anchor; superseded or departed-owner completions cannot
  install active Copy authority. Activation deferred after Root render also
  checks the actual session, consumer generation, mode and overlay state.
- `PostKey::Body` and `PostKey::Comment` using GitHub's opaque comment ID
  reconcile retained posts. Row index, equal author/time metadata and equal
  body length are not identity. The SDK member key stays tied to the retained
  text entity belonging to that real post.
- Each post retains raw source, presentation format, prepared `SharedString`
  and text state. Exact source/format reuse avoids repeating accepted
  preparation and parsing. Changed bytes, even at the same length, replace
  that post's source revision. Localized empty-body placeholders participate
  in the same source contract. No second parser or raw-Markdown Copy path is
  introduced.
- List reconciliation preserves the stable post plus intra-post pixel offset
  at the viewport top across prepend/removal. Source changes remeasure changed
  rows; current theme/width/zoom remeasure geometry separately. Loading/error
  chrome does not replace the accepted post anchor.

### SDK lifetime, source interval and native authority

The active consumer holds `TextSelectionGroupRetirement` returned by
`Root::set_text_selection_group`; Root owns only weak entities/lease ownership.
An unchanged binding shares the live lease. Explicit retirement, last consumer
drop, owner drop, screen departure or overlay displacement synchronously clears
the actual range and native geometry/viewport/edge task, without waiting for
paint. Returning to the screen permits a new selection, not revival of the old
one. An old lease cannot retire a replacement.

A changed same-group binding receives a fresh lease and loses **all prior
native hit/drag/edge authority**. It may preserve logical endpoints only if the
old range was valid in the active scope and its selected interval retains the
same ordered stable IDs, retained entities and accepted source revisions.
Reordering or prepending outside a single selected true B therefore preserves
B, not its old ordinal; an unselected source edit may also preserve B.
Selected source changes/deletion, interval insertions/reordering, invalid weak
members, different group/scope or retirement fail closed. Copy needs the
replacement viewport's paint before a preserved range is usable. The
intermediate rule “every membership refresh clears selection” is superseded;
preserving endpoints never preserves stale geometry.

The parse-owned `RenderedDocument` is the canonical rendered UTF-8 stream:
block/table separators, link labels and literal code, not raw Markdown or
hidden HTML/image destinations. SelectAll and logical Copy share the existing
`visit_rendered_text` traversal. `ParsedDocument::text` and `BlockNode::text`
were obsolete extraction wrappers and are removed; the custom Markdown plugin
test now uses the same canonical stream, while real paragraph/parser and
legacy window-selection consumers remain.

Current shaped layout chooses the nearest visual caret with grapheme-safe
endpoints; accepted-source caret metadata is cached. GPUI owns presence states
through actual layout/cached replay, and the selection controller holds weak
presence only. Actual unmount withdraws hit authority. Style-only reflow
rebuilds geometry rather than changing unchanged source offsets.

The managed ancestor also routes SelectAll for an **already focused retained
post** after its TextView unmounts. Mounted descendants and enabled Input keep
their normal action priority. The fallback revalidates current focus, lease,
scope, weak member and accepted source revision, then invokes the existing
TextViewState SelectAll handler after releasing Root. It selects that member's
canonical bytes, not the whole conversation; it neither steals focus nor
revives geometry or a retired owner. No new global binding/cache/scan is added.

### Coverage boundary and observed checkpoint

- SDK tests retain exact rendered Unicode, synthetic separator, image/HTML,
  code, offscreen, cached-layout and legacy window-selection assertions.
  The former composer fixture is explicitly
  `disabled_input_copy_does_not_fall_back_to_conversation`, rendered with
  `Input::disabled(true)`, and retains its exact draft/empty Copy assertions.
  This is meaningful **disabled/read-only boundary** evidence, not proof of
  enabled editing. No TestWindow native-hook panic is ignored/caught, and no
  test is skipped or replaced with a fake native platform.
- `issue_conversation_enabled_input_copy` uses the runner's real AppKit window
  and production enabled Reply. SelectAll/Copy returns the exact Unicode draft;
  physical Backspace empties it and Copy preserves clipboard poison. Physical
  Tab then copies the **original** conversation selection without refocus,
  reselect, reload or reactivation, making the empty-input boundary non-vacuous.
  Departure stops Copy; draft, HEAD, staged paths/OIDs/modes, working bytes,
  refs, stash and oplog invariants are checked. Clipboard is private to
  `VisualTestPlatform`; event simulation is not hardware input or IME proof.
- PM's final-caret/cache native run (`bg_600`, raw artifact 2153) was **9 PASS,
  1 FAIL of 10**. Offscreen/giant copy, anchor/error, geometry, edge drag,
  rejected/loading refresh, two owners, modal/reentry priority and the full
  enabled Input counterpart passed. Equal-author/equal-time comment-ID reorder
  alone failed at the `OLD_B` copy assertion.
- After the SDK stable-interval setter fix, PM's `bg_605` ran
  `issue_conversation_accepted_identity`: **1 PASS of 1**, 37.67 s process wall
  including build/runner overhead. Its original native assertions, coordinates
  and producers were unchanged; reorder, prepend, same-length mutation and
  deletion all reached their consumer oracles. The focused SDK refresh test
  separately specifies old-lease isolation and native-geometry retirement.

Subsequent PM runs exercised that frozen setter, disabled fixture and obsolete
extraction cleanup together:

- `bg_607`, raw artifact 2173: complete SDK **lib** suite **334 PASS, 0 FAIL,
  0 ignored, 0 filtered**, without skips. Test-body time was 0.06 s and process
  wall 12.75 s. Logical/legacy/parser/custom-plugin consumers all ran. The
  compile output no longer contained either obsolete SDK text dead-code
  warning; existing dependency future-compatibility warnings remained.
- `bg_608`, raw artifact 2175: native **all ten together 10 PASS, 0 FAIL,
  exit 0**, including the enabled Input recovery counterpart and all original
  nine consumers without assertion/coordinate/producer changes. Process wall
  was 64.80 s, including 28.35 s compilation. These are process/test timings,
  not UI latency or speedup measurements.
- `bg_609`, raw artifact 2177: SDK workspace `cargo fmt --all -- --check`
  **FAIL**, exit 1, 2.63 s. Its diffs included untouched assets/macros/story
  baseline and changed selection code. The repository config explicitly uses
  edition/style edition 2024; this is not evidence for a 2021 style override.
  Scoped formatting must not restyle that unrelated baseline.
- The authorized formatting-only phase ran scoped `rustfmt` and `rustfmt
  --check` with the unchanged `.rustfmt.toml` and `skip_children=true` on
  `root.rs` plus text's `document.rs`, `format/markdown.rs`, `inline.rs`,
  `inline_flow.rs`, `mod.rs`, `node.rs`, `state.rs`, `window_selection.rs`,
  `logical_selection.rs`, both logical-selection test files and `rendered.rs`.
  Both commands exited 0 (0.15/0.16 s). `text_view.rs` was not rewritten:
  its changed Copy line was already formatted and the check's line-463
  trailing whitespace belonged to unchanged baseline. No semantic split or
  refactor was included; scoped success does not make the whole workspace
  format check green.


The earlier SDK run with an unsupported enabled TestWindow case omitted is
not full-suite acceptance; the later complete lib run above supersedes that
limit. The SDK manifest has `doctest = false`, so no documentation test pass is
implied. The recorded passes precede the authorized formatting-only phase;
they do not claim an execution against a subsequently published revision.
Build/workspace/Clippy/format/invariant/default-build gates, default-app Tier B,
public fixed SDK pin, hosted CI, exact-head review, publication and merge remain
separate acceptance gates. No private absolute SDK patch path is a final
installation instruction, and these observations do not close #1091 or
establish CPU/FPS/latency improvements.


### Post-format SDK checkpoint (2026-10-10)

PM `bg_615` (raw artifact 2195) reran the complete SDK lib suite after scoped
formatting: **334 PASS / 0 FAIL**, test bodies 0.07 s. UI-lib Clippy then exited
0; combined process wall was 42.52 s. The two obsolete text dead-code warnings
remained absent and existing dependency future-compatibility notices remained.
Clippy introduced one `too_many_arguments` diagnostic on
`register_logical_inline` (nine arguments, threshold seven).

The private native paint registration boundary now has one localized,
documented `#[allow(clippy::too_many_arguments)]`: GPUI element identity/view,
accepted-source leaf/range, native shaped layout/line bounds/hitbox and
Window/App are supplied independently by its existing sole paint caller.
The existing `Geometry` additionally needs validated owner/scope/weak presence,
resolved only after registration guards. Moving that authority into a caller
or adding another payload solely to bundle arguments would broaden the change.
No signature, allocation, copy, guard or geometry behavior changed.

The localized lint annotation was added **after** `bg_615`. PM's exact-source
UI-lib Clippy `bg_619`, against `logical_selection.rs` SHA-256
`f1b028c23b42d14cafced1e8ca75d1819a6d47dca5ea80fdf24a9ef829a2b8ab`,
exited **0**, process wall 4.19 s including 3.99 s check/compile. No new
argument-count or dead-code diagnostics remained; only existing dependency
future-compatibility notices (`block`, `proc-macro-error2`) remained.

PM `bg_620` (raw artifact 2207) then ran that exact source's complete SDK lib
suite: **334 PASS / 0 FAIL, 0 ignored, 0 filtered**, bodies 0.07 s, followed by the same
13-target `rustfmt --check` with configured edition/style edition 2024 and
`skip_children=true`, exit **0**. Combined process wall was 7.33 s.
No public SDK revision is inferred. Native all-ten proof remains the earlier
pre-format run, not a second post-format native pass. Whole-workspace baseline
fmt failure and public-pin/default-app/root-gate/CI/review limits above remain
unchanged.


### Public SDK and consumer pin (2026-10-10)

PM published the original-b004-compatible SDK fix to the existing
[TomiXRM/gpui-kit fork](https://github.com/TomiXRM/gpui-kit), on
`fix/kagi-logical-selection-b004-20261010`, at exact commit
[`941f20e6c374ea80d2bab9cd08fc96021280db13`](https://github.com/TomiXRM/gpui-kit/commit/941f20e6c374ea80d2bab9cd08fc96021280db13).
The normal branch push (`bg_627`) exited 0 and PM's `ls-remote --exit-code --heads`
reported the same commit. This supersedes the earlier public-SHA-pending
checkpoint; it is SDK publication, not Kagi PR approval or merge.

At the first cutover, Kagi's existing original-source patch table pointed
**both** `gpui-component` and `gpui-component-assets` at
`https://github.com/TomiXRM/gpui-kit.git`, with the full revision above. Both
temporary absolute-path patches are removed. Original b004 dependency
declarations/features and the rev-less Zed declarations are unchanged; this
is the fixed SDK family, not a latest-SDK/API upgrade. No macros patch,
alias, path shim or extra crate version is introduced by the manifest change.

The SDK code at that first publication was the exact annotated source exercised by
`bg_619`/`bg_620`: lib334 without skips/ignored/filtered tests, UI-lib Clippy
without new warnings, and configured-2024 scoped13 fmt check. Whole-workspace
baseline fmt remains FAIL, distinct from scoped success. The earlier all-ten
native run remains 10 PASS, not a post-public-pin/native rerun.

The worker changed only the manifest; PM subsequently generated Cargo.lock
through actual resolution. The first targeted `cargo update` (`bg_632`) failed
to match the package specification during the path-to-Git transition (0.09 s,
no compilation). It was not hidden or treated as acceptance.
`cargo metadata --format-version 1` then succeeded (6.523 s, artifact 2230),
adding only these three SDK packages at the exact published Git commit:
`gpui-component` 0.5.2, `gpui-component-assets` 0.5.1 and
`gpui-component-macros` 0.5.1. PM's metadata selection contained each name once.
GPUI 0.2.2 and `gpui_platform` 0.1.0 retained the original Zed commit
`90b3aa0b3bd3b453775b11a386907c7ac9acd997`. No lockfile hand edit, latest-family
upgrade, second macros source or path shim was required.

That initial metadata checkpoint proved resolution, not compile/native gates.
Its then-pending root/default-app acceptance is superseded by the observed
new-revision checkpoint below; hosted CI and independent review/merge remain
separate.

### Offscreen SelectAll and final public acceptance (2026-10-10)

Internal code review found that the unmounted focused member's ancestor
restored Copy, but not SelectAll. A new real AppKit consumer physically selects
`PARTIAL` in `**PARTIAL**\n\n日本語🙂 _café_ **code🧭**`, scrolls that post
out with bounded wheel events until its control witness disappears while
another post remains visible, and verifies offscreen Copy still returns the
original partial range. Physical Cmd-A/C must produce exactly
`PARTIAL\n日本語🙂 café code🧭`, without raw Markdown or other posts.
The test checks the complete Reply draft and repository invariants and performs
normal cleanup before that final byte assertion. It does not dispatch SelectAll
directly, alter focus after unmount, inject a selection or use the host clipboard.

Corrected Before `bg_650` on resolved immutable public `941f20e6` was 0 PASS/
1 FAIL: left `PARTIAL`, right the unchanged full rendered literal; process
17.40 s (compile 8.97 s). Corrected After `bg_651` (artifact 2322) on private
local integration passed **all eleven together 11 PASS/0 FAIL**, including
the unchanged original ten and enabled Input precedence; process52.63 s
(compile8.48 s). The paired fixture/oracles/events/coordinates were identical.
Earlier inline-code fixtures `bg_647`/`bg_648` are separate historical failures:
Kagi's existing code-span preparation inserts U+2009; this regression now uses
strong/emphasis without asserting incidental code padding. No padding policy,
code/table/privacy coverage or original-ten oracle was changed.

Native After preceded the final three-file SDK formatting pass. Configured
edition/style edition2024 with `skip_children=true` format/check of those three
files exited0/0.27 s. Post-format SDK `bg_653` was lib334 PASS,0 failed/ignored/
filtered (body0.06 s/process11.19 s), then UI-lib Clippy0/process4.11 s,
combined15.40 s, no new warnings; existing dependency future-compatibility
notices remain. Whole SDK workspace baseline fmt FAIL was not rerun/declared
green, and doctests remain disabled.

The final SDK revision is public
[`ae37bfd433781abe44e15edd40867fac1b7b3b2c`](https://github.com/TomiXRM/gpui-kit/commit/ae37bfd433781abe44e15edd40867fac1b7b3b2c).
PM's explicit GitHub `bg_657` push was a normal fast-forward from immutable
`941f20e6`; exact public `ls-remote` matched. `bg_656` had mistakenly pushed a
branch to clone-origin/local Cargo SDK cache; `bg_657` conditionally deleted
only that new mistaken ref at its exact expected revision. No checkout/files
or objects were removed; this is not a claim that the cache was entirely
untouched. These are publication receipts, not external approval.

Both UI/assets entries now use the same full public revision above; all
TEMP paths/comments are removed. PM's real `cargo metadata --format-version 1`
exited0 in1.526879 s: UI0.5.2/assets0.5.1/macros0.5.1 each occur once on that
public revision. GPUI0.2.2/platform0.1.0 retain original Zed
`90b3aa0b3bd3b453775b11a386907c7ac9acd997`. No lock hand edit, latest-family
upgrade, additional macros patch, alias or path shim.

After PM's Root formatting, `bg_658` on this final public pin passed all seven
gates, total557.496848 s. Receipt directory:
`issue-select-all-public-ae37-final-gates` (`manifest.json`/raw stage logs).
Process walls are not UI latency or a speedup comparison:

| Stage | Exit | Process wall (s) |
|---|---|---|
| build | 0 | 25.170750 |
| native `issue_conversation_` | 0 | 52.765071 |
| workspace | 0 | 458.066894 |
| Clippy | 0 | 15.626914 |
| Root fmt check | 0 | 2.794284 |
| UV invariants | 0 | 2.494605 |
| default build | 0 | 0.575039 |

This final public native log reports `PASS filtered scenarios`, not an explicit
KEEP_GOING11 count. Keep it distinct from local `bg_651`'s explicit11/11
summary; do not relabel the older941 receipts as final-ae37 acceptance.

#### Default-app Tier B limits

PM's final-public default-app receipt is
`issue-select-all-public-ae37-tierb/observed-public-ae37.json`. Owned original
Popen PID42755/exact executable and largest layer0 WID6126 were used; capture
2784×1766 versus logical1392×883 gives a capture-derived ratio2, not a queried
NSWindow backing scale. Isolation explicitly used `USER`, `KAGI_NO_ACTIVATE`,
`KAGI_NO_RESTORE` and `KAGI_LOG_DIR`. Real fixture
[Issue281](https://github.com/TomiXRM/kagi-hig-ux-fixture-20261008/issues/281)
has two posts with80 paragraphs each, actual GFM bold/italic/Japanese/emoji/café.

Mounted physical Cmd-A visibly selected the body. Ten bounded−120 wheel events
reached distinct comment071–080 and Reply with the body wholly outside; physical
Cmd-A there did not visibly select the other post. Ten+120 events returned to
the selected original body without a body re-click/reselection. This trial
began fully selected: **exact partial→full expansion is the private-clipboard
native oracle, not a screenshot inference**. Actual unmount is the native
control witness, not pixels alone. Default Copy/Paste/host clipboard were not
used.

Actual zoom100→110→100 and AppleLight→CatppuccinMocha rendered the form/content.
Settings changes focus; selection was not shown on return, so this is not
selection retention across a modal/theme transition. No new width-resize,
performance sample, CPU/FPS percentage or causal Before/After timing is claimed.
Historical941 photos/8-second samples stay historical. Cleanup was Cmd-Q0,
original Popen wait0, proc_pidpath0, persistent owned app0.

This is the accepted Issue-only public-consumer slice, **Refs #1091**, not full
issue closure. Broader performance/width/comparison and PR-conversation scope,
hosted CI, independent exact-head review, Kagi PR publication and merge remain
pending. Internal omp review is not herdr mail or external merge approval.

