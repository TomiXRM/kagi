# ADR-0057: Resolution Buffer と Undo(repo を汚さない解決)

- Status: Accepted(2026-06-13)

## Decision

- choose(current/incoming/both-ordered)・手編集は **WT/index に触れない解決バッファ**上の操作。
  ファイルごとに Result 草稿 + 操作履歴(undo/redo)。draft(ADR-0042)と同じ
  `~/.kagi/conflicts/<repo-hash>/` へ自動保存 → 中断・再開可能。
- 明示した Save または Continue 時に WT へ書き出し+stage。
  **abort してもバッファは oplog 参照付きで退避**
  (jj の「部分解決を失わせない」の git 互換実装)。
- 行ごとの採用元(current/incoming/manual)を保持し UI で出所可視化(BC/KDiff3 流)
- `LineOrder::CurrentBlockFirst` / `IncomingBlockFirst` preserve hunk-level Both as all taken lines of one side followed by all taken lines of the other after checkbox edits; fresh line selections retain positional `CurrentFirst` / `IncomingFirst` tie-breaking.
- Ordinary UI edits and selections capture through `queue_autosave()` (#1760).
  Queue acceptance is not durability: coalescing lasts up to 250 ms before
  starting, and in-flight I/O can extend the unacknowledged crash window.
  Close/quit/tab close and Save/Continue/Abort/Skip/D-F await `flush()` off the
  UI thread, before repository admission. A successful current-generation flush
  alone permits approval; changed generations/revisions/attachments invalidate it.
  No draft lock may be nested under the repository admission lock.
  Non-cancellable OS termination instead captures all live panes before windows
  disappear and urgently flushes their owned tickets within GPUI's shared 200 ms
  shutdown budget. Failing/slow I/O can exceed that budget; this path does not
  claim a successful durability acknowledgment or remove the crash window.
- `has_unflushed_drafts()` drives the unsaved gate, including failed saves.
  Failed persistence retains live text, records the failure and refuses the
  operation. Explicit Retry must durably finish; intentional close-time loss
  requires both Discard confirmations. Merely recovering storage permissions
  does not authorize Abort to substitute an older disk-loaded draft.
  Each explicit operation retries the current captured body after storage repair.
  A mixed-state close-sheet Retry flushes conflict decisions only; ordinary
  editor writes remain behind their separate, explicitly labeled Save All.
  Retrying an identical capture may create a new writer generation without
  changing its UI decision token. Real decision changes invalidate that token
  even if queuing fails; approval still requires the current writer ticket.

## JSON codec（#513 resolution slice）

- autosave の JSON 構文処理は `serde_json` に統一する。保存時は借用する
  `Serialize` view を使い、domain 型への serde 依存や草稿文字列の複製は増やさない。
- 既存の `repo` / `updated` / `files` と各 file の `path` / `binary` /
  `current` / `incoming` / `result` / `raw_result` を維持する。
  `result: null` は未解決、空配列は空テキストへの解決であり、混同しない。
  行の `t` / `o` と選択済み raw の `oid` / `mode` も維持する。
- 読み込みは呼び出し側の repo path をキーとし、欠落・不正な optional field は
  従来の既定値へ戻す。path のない file は読み飛ばし、不正な raw result で
  テキスト草稿を失わせない。JSON 自体が不正なら読み込みは `None` とする。
  surrogate pair は Unicode 文字として復元し、再保存しても脱落させない。
- live index 由来の raw metadata は再検出する。保存済みの選択を適用するには、
  以下の既知かつ一致する file identity と text/raw の互換性を要求する。
  undo/redo・source hunk data は JSON に保存しない。hunk choices・line selections・
  positional/block の全 LineOrder と materialization fingerprint は保存し、
  既知かつ一致する live file identity と materialization にだけ復元する。保存先と oplog は維持する。

## 描画用 marker 派生状態（#497）

- `FileResolution` の private Result 差替え境界で、その内容の marker 判定と
  `ResolutionBuffer` の集約件数を同時更新する。選択・手編集・hunk 操作・undo/redo
  は同じ境界を通り、変更していない file は再走査しない。別の数値 revision や
  repository の `ConflictRevision` を key にせず、Result と派生状態を一緒に所有する。
- load は判定を再構築し、live index への autosave overlay は該当する draft と
  検証済みの派生状態だけを運ぶ。派生値は JSON に保存しない。
- render は cached marker verdict を読み、dashboard の Continue と理由表示は
  同じ blocker 評価を共有する。未解決・binary・deletion の優先順位は変えない。
- Edit 中の同期は hunk の元データではなく現在の Result と比較する。
  `text_to_lines` と同じ末尾改行の扱いで比較し、同じ手編集を再描画ごとに
  undo stack へ積み直さない。空テキストと空行は区別し、Preview の組立表示は維持する。
- cache は表示専用。`files_with_marker_residue` と Save/Continue の実データに対する
  fresh validation は維持し、cache を書き込みの安全ゲートにしない。

## Pure selection and provenance gates（#1746）

- A chosen zero-line side represents an adopted deletion: its side state is
  `All`, not `None`, before and after line-selection materialization. Persist
  explicit empty-side adoption alongside the masks; older drafts derive it from
  their hunk choice. Parent deselection respects adopted empty sides.
- Selection masks are immutable in size outside their validated constructors.
  Restoring a draft rejects either mismatched dimension before changing any
  hunk. Assembly also bounds-checks source lines so a stale in-memory selection
  cannot panic or invent content. An invalid line edit is a complete no-op.
- Readiness, fresh Save and Continue marker checks inspect each `ResolvedLine`:
  only `Manual` lines can contain blocking residue. Unresolved hunks re-emit
  markers with that origin. Literal marker-looking text from an adopted Current
  or Incoming side, and shared Context, is source content, not residue.
  Provenance-absent manual overrides use a plain-text resolution detector:
  complete `<`, `=` or `>` runs of at least seven characters, followed by space
  or end-of-line, block even when the synthetic delimiters are wider than seven.
  The independent staged-blob checklist's existing detector is unchanged.
- Recorded Save freezes and hashes provenance together with text and file
  identity, and restores those exact origins after preflight. Dropping origins
  at the request boundary must not turn a chosen literal into a manual marker;
  changing only provenance invalidates the prepared payload binding.
- Marker provenance is established by unambiguous materialization, not by
  reconstructing stage blobs: zdiff3 passthrough deliberately includes the other
  side's clean changes. For base, Current and Incoming, find the longest
  line-start run of `<`, `=`, `>` or `|`; choose `max(7, longest + 1)` and give
  that width to libgit2 for either marker style. Unrepresentable widths refuse
  materialization rather than wrapping.
- Materialization carries its text and chosen width together. The parser accepts
  only that exact width and the state sequence opener → optional base →
  separator → closer. Marker-looking source lines therefore keep their source
  origin, including literal openers, separators and wider runs; unresolved
  re-emission retains the chosen width and `Manual` origin. Whole-side choices
  continue to read the stage blobs directly. This closes the parser collision
  in #1751 without blocking normal per-hunk choices after clean auto-merges.
- The #1730 hunk-state fingerprint already covers the delimiters in materialized
  text. A width-7 draft whose sources require wider markers cannot restore that
  authority: preserve its exact body in recoveries and clear the live
  resolution. Validate all saved hunk models during live restoration, not only
  the selected file, so Save/Continue cannot consume an unvalidated old body.
  Materialization-rejected drafts carry a persisted `recovery_only` flag:
  matching file/worktree identities retain inspection context but never restore
  their write authority. Every post-detection `retain_drafts` revalidates all
  retained hunk states against fresh materialization, including cached models and
  unselected files; saving another file cannot revive an incompatible body.
  Operation Log's existing preserved-draft inspection uses the same loader and
  shows that body read-only, never applying it to the fresh conflict.


## Result input と永続 draft の適用権限（#1721）

- Result の `InputState` は path だけでなく、受理した `ConflictRevision` と
  content signature に束縛する。再検出の受理時は旧入力を旧 buffer に同期してから
  新しい観測を採用する。旧入力を新 revision の buffer へ pull してはならない。
- 同じ既知の file identity なら、編集中の本文・undo/redo・選択と入力 entity を
  保持する。無関係な index entry の更新だけで同じ file の入力を捨てない。
  同 path でも次 commit / stage / operation context が違えば新しい未解決 Result を
  表示し、旧本文は保全するだけで `has_resolution` の根拠にしない。
- 永続 file identity は version 付き fingerprint とし、canonical gitdir、raw path bytes、
  operation context、各 conflict stage の有無・OID・mode を含む。
  HEAD または必要な operation metadata を確認できなければ identity は不明とする。
  表示用 observation の best-effort 読み取りと、draft 適用の認可を分離する。
- live buffer は空 session も含め、元 repository と strict な operation context を
  メモリ内の書込 provenance として保持する。clone は元の値を保持し、JSON decode や
  `new` は権限を作らない。`retain_drafts` は受け手の live provenance を上書きしない。
  recorded Save の最小 buffer は、元の frozen observation と file/payload binding が
  現在値に一致した preflight 境界でのみ、その検証済み context を受け取る。
  この provenance と、確認済み Continue が凍結する全 staged content の revision は
  別の権限である。入力保持のために古い Continue の承認を新しい観測へ付け替えない。
  全操作の承認だけに属する message / author-script / replay option / autostash の
  変更は、既知で一致する同 file の入力を捨てる理由にしない。旧承認は失効し、
  新しい明示 plan に同じ live buffer を使えることと分離する（ADR-0067）。
  この修正で永続 file identity の minimal metadata の形式や順序を拡張しない。
- `identity` / `path_bytes` / `editing_origin` / `draft_state` と、同じ document 内の
  `recoveries` を保存する。旧形式、不明 identity、不一致の本文は自動 overlay しないが
  黙って削除しない。明示した undo-to-unresolved の null は「未編集の初期状態」と区別し、
  過去の本文を復活させない。既存 clear/save/adoption 経路も保全を共有する。
- Continue / Skip / merge 完了は永続 document を先に消さない。foreground の再検出・
  adoption より前に終了しても、既に保存した本文を同じ document から復元できる。
  完了または次 step への遷移で失効した identity は inspection-only に落ちるため、
  自動適用を防ぐ目的の completion-time clear は不要である。
  `ResolutionBuffer::clear` は現在の draft を明示破棄する API として残し、
  古い recovery と明示 null の保全を維持する。operation completion からは呼ばない。
- `editing_origin` は編集 owner の来歴であり、再読込や clone に書込権限を与えない。
  新しい実編集・undo/redo だけが未保存 intent を進め、保存成功時にその intent を消費する。
  保存失敗時は保持する。古い live recovery の再保存は後の durable 本文より新しくならず、
  本当に新しい未保存 undo-null は古い同一 null より優先される。
- 保全本文の閲覧は既存 Operation Log の明示 action と専用 Copy で行う。
  receipt が 0 件でも到達でき、操作ログ保存失敗後の再起動でも利用できる。
  閲覧は現在の conflict に適用せず、普通の receipt 選択・Copy の意味を変えない。
  attachment の session / visit / path / worktree と request generation を固定し、
  実 gitdir の owner を再確認してから読み出す。古い完了を別 owner へ表示・コピーしない。
  inspection は読込済みの構造化本文を保持し、表示・専用 Copy の翻訳だけを現在の
  language で構築する。language 切替で repository を再読込したり owner を付け直さない。

