# ADR-0067: Continue / Abort / Skip Safety Policy

- Status: Accepted(2026-06-13)
- 関連: requirements-conflict-ux.md §2.6 / ADR-0056 / 0066 / 既存 plan_conflict_continue/abort(W26)

## Decision

Conflict Mode 中の Continue / Abort / Skip / Mark resolved / Save は **直接実行せず Plan 経由**
(GitOperationPlan または ConflictOperationPlan → confirm → preflight → execute → verify → oplog)。

### Continue 前チェックリスト

UI は現在の buffer から unresolved / marker 等を投影して Continue を表示する。
確認の計画と実行は repository-bound な正規 checklist を再評価し、表示 cache を認可にしない。

- unresolved files count == 0
- conflict marker 残存なし(ADR-0066)
- session が扱わない unmerged index entry なし。session 内の明示解決は Continue で stage できる
- binary conflict 残なし
- required file が削除されていない
- commit message 非空(merge commit を作る場合)
- checklist(ADR-0043)blocker なし

### Continue の実行
- merge: 解決を index へ準備し、既存 Commit Panel で message と merge commit を確認する
- rebase/cherry-pick/revert: 解決を stage して sequencer continue(次の step へ)

### canonical index と prewrite の境界（#1721）

- `continue_blockers(repo, session, buffer) -> Result<Vec<ContinueBlocker>, GitError>` を
  plan / route / execute で共有する。長寿命 Backend が別 handle の Save 前の index を
  保持し得るため、canonical read は `index.read(true)` で必ず disk から更新する。
- index の取得・再読込・conflict iterator の生成・各 entry の読取失敗を伝播する。
  失敗を空 checklist、古い cache、marker-only 判定へ置き換えない。
- 共有 `stage_conflict_resolution` が WT/index 準備の直前にも同じ checklist を再確認する。
  sequencer の公開 execute と merge Commit Panel 向け準備のどちらもこの境界を通り、
  読取不能または session 外の unmerged path があれば書込前に拒否する。
  Save 済みだから通す例外や、通常の coverage 判定の緩和はしない。
- session の operation が現在と異なる場合、または解決済み path の既存 file identity
  （canonical gitdir / operation context / stage の有無・OID・mode）が live index と
  一致しない場合は、既存 `ObservationChanged` として書込前に拒否する。同じ path が
  次 conflict に存在するだけでは旧 Result の権限にならない。旧 session の path が
  既に stage 0 なら再 materialize せず再読込を要求し、fresh な空 session の Continue は許す。
  空 session でも元の repository / strict operation context と現在値の一致を先に検証し、
  不明・別 repository・別 operation なら index/tree 作成前に拒否する。
  現在の context を取得できることだけでは元の承認の根拠にならない。
- text / raw の直接 Save も fresh index と同じ file identity を書込前に照合する。
  recorded Save の既存 `BufferRevision` は payload と producer の file identity を
  一緒に凍結し、plan / run の global observation 検証に加えて照合する。現在の
  observation を旧 payload に添えても新 identity を無条件に与えない。
  無関係な index entry の更新は per-file identity を変えない。

### Worktree-byte provenance for Save / Continue (#1733)

- The revision stale check also includes the conflicted path's worktree bytes,
  captured when the live file is built. This is a separate SHA-256 fingerprint
  from the synthesized zdiff3 marker fingerprint used by #1730's hunk restore.
  Save freezes it in the request and checks it at planning and immediately
  before mutation; Continue checks every resolved path before staging.
- A mismatch or unknown source refuses with `File changed outside Kagi` /
  `ファイルが外部で変更されました`; neither worktree nor index is changed.
  Watcher worktree events re-detect open conflicts even when the index is unchanged.
  The original fingerprint is retained across watcher refreshes even before the
  first hunk choice; an untouched file is not permission to rebase its source.
  Current regular-file bytes exactly equal to the frozen Text draft are the
  sole idempotent exception: Partial Save/Continue may have already written them
  before staging failed. Retrying those identical bytes cannot overwrite an
  external edit; differing bytes and symlinks remain refused.
- Source hunks always remain index-derived, never parsed from arbitrary
  worktree markers. Matching materializations retain the draft and its original
  byte fingerprint, not fresh write authority; unknown/changed materializations
  remain in backward-readable recovery inspection.
- The header disables Save and offers `再読込` after the user deals with the
  external edit. Reload re-detects from the index; it does not bless the changed
  bytes or silently replace them. If the worktree still differs, Save stays
  blocked. Restore the reviewed source or stage the externally reconciled
  resolution explicitly. Showing the external diff in the source panes is not
  part of this decision.

### 確認済み Continue の revision（#1721）

- 同 file の編集を保持できることと、古い確認から書き込めることは別である。
  Continue は全 staged content を commit し得るため、確認を開く時の accepted full
  `ConflictRevision` を保持する。元の operation metadata と index の path/OID/mode、
  `flags` / `flags_extended` を含む revision を、Confirm 時の新しい mode や実行直前の
  観測から作り直さない。intent-to-add と skip-worktree が併存すると、path/OID/mode と
  short flags が同じでも intent-to-add の解除で commit tree が変わるため、拡張 flag も必須である。
- `Backend` の Continue plan / route / stage / execute は必須の expected revision を受ける。
  計画時と実書込前の両方で fresh index を使って既存の正規 observation と照合し、
  既知の不一致を Ready として提示しない。読取失敗は伝播し、None や旧 cache で通さない。
- sequencer modal は元の revision と operation kind を持ち、queued background dispatch
  まで同じ値を渡す。merge / stash の明示 plan も元の accepted revision を渡す。
  同 commit / step / total のまま別 base へ rebase を開始し直した場合や、同 operation の
  stage 0 本文だけが変わった場合も、古い承認からは WT/index/object/ref を変更しない。
- 無関係な index 更新で同 file の入力を破棄する必要はない。一方、staged content が
  変わった古い確認を再利用してはならず、現在の観測から新しく明示計画・承認する。
  正当な Save 後の fresh-empty session はこの新しい承認で引き続き Continue できる。
- full revision は active operation の保存済み入力も凍結する。`rebase-merge`、
  `rebase-apply`、`sequencer` と operation-owned `refs/rewritten` の state tree、および
  root の HEAD 記録・`MERGE_MSG` / `SQUASH_MSG` / `MERGE_MODE` / `MERGE_AUTOSTASH` を
  名前・有無・file type ごとに framing して読む。message / author-script / option の
  filename allowlist だけにせず、既存 state tree の追加・削除・本文変更を検出する。
  message / patch 本文は 8 KiB buffer で一度だけ streaming hash し、directory ごとに
  固定長 child digest を整列する。symlink / special file、読取不能、読取中の変更は拒否する。
- TODO は Git の実行を再実装せず、実際の commit / reset / merge / update-ref operand の
  名前と canonical resolver の解決先だけを追加で束縛する。rewritten label は Git と同じ
  優先順位で解決し、先行 pending `label` が生成する名前は外部 ref の欠損と扱わない。
  `squash-onto`、処理済み `update-refs` の最終 CAS 対象、rebase の `autostash` operand も
  含める。operand は 64 KiB 上限で閉じ、message / subject 全体をその buffer に溜めない。
  未知・解決不能・読取不能な operand で Ready を作らず、無関係な全 repository ref は走査しない。
  root `MERGE_AUTOSTASH` は conservative に束縛し、Git と同じく symbolic ref を拒否する。
  これは Kagi の Merge に autostash 復元を追加する決定ではない。
- この承認は **operation-state approval** であり、任意 exec、global `refs/replace`、
  config、hook、環境の全依存や最終 commit hash を凍結するものではない。
  Git 2.50.1 の [sequencer](https://github.com/git/git/blob/v2.50.1/sequencer.c)、
  [apply backend](https://github.com/git/git/blob/v2.50.1/builtin/am.c)、
  [commit message inputs](https://github.com/git/git/blob/v2.50.1/builtin/commit.c) を根拠とする。
  metadata が変われば、同 file の手編集を保持できても旧承認は拒否する。変更後の
  message / authorship / replay option / autostash を使うには、新しい明示 plan と承認が必要である。


### Abort（保持できる場合の安全弁）
- 確認ダイアログを出す。**保存済み resolution が失われる可能性**を明示
- `cleanup_state` + ORIG_HEAD 復帰(force/reset --hard/clean 不使用、ADR-0056/0057)。
- Abort / stash abort / Skip は、現在の resolution buffer の recovery copy を実際に
  保存できた場合だけ破壊的な repository 操作へ進む（#1731）。保存失敗を無視せず、
  typed blocker `DraftPreservationFailed` と `Refused` を返す。JA は
  「下書きを保持できないため実行できません」。index / WT / ref / operation metadata は変更しない。
- live conflict がない場合は空 buffer で既存 autosave を上書きしない。既存の保存済み draft は残す。
  計画は保持成功を無条件に約束せず、buffer の recovery copy と復元される WT の違いを明示する。
- D/F の namespace 上書きも必要な recovery snapshot が失敗したら index / WT 変更前に拒否する。
  この変更は保存形式を変更せず、従来の draft を引き続き読み込める。
- UI の manual / hunk autosave 失敗では live draft を残し、Failed の記録と Error toast を出す。
  Close / Quit / Relaunch / repository tab close（Cmd+W・tab ×、最後の tab → Welcome を含む）は既存の単一 dirty sheet で保留する。Retry は autosave 成功時だけ
  failure を解消し、Discard は二度目の明示確認を必要とする。Cancel / Enter / Escape は安全側に倒す。
  sheet は既存の `modal_card` / `modal_button` による compact warning とし、独自 geometry は追加しない。
- Abort の plan 表示前と Confirm の両方で、対象 session の live `draft_persist_failed` を検査する。
  保存先のアクセス権が直っても、古い disk buffer の保存成功を新しい live decisions の保存成功とは扱わない。
  live view の Retry 成功まで同じ typed blocker で拒否する。tab close の Retry / Discard も対象 session だけに適用する。
  Abort / Skip / D/F で共有する blocker の EN/JA は operation-neutral とし、保存先のアクセス権確認と再試行を案内する。

### Cherry-pick / revert sequence abort の開始点 (#1127)

`sequencer/head` が sequence 全体の開始 HEAD。先行 pick 済みの現在 HEAD や、
cherry-pick が更新しない古い ORIG_HEAD へ戻さない。開始記録を conflict revision
に含め、確認後の差替えを拒否する。live sequencer の開始記録が欠損・不正、
または commit を読めない場合は EN/JA の blocker で mutation 前に拒否する。
`sequencer/abort-safety` は現在 HEAD と一致することを要求する（Git の
`rollback_is_safe()` と同じ安全基準）。不一致なら手動 commit 等で HEAD が
移動したため巻き戻さないと EN/JA で拒否し、欠損・不正も mutation 前に拒否する。
安全基準も conflict revision に含め、承認後の差替えを拒否する。
sequencer を使わない単一 pick は現在 HEAD が開始点。復元は既存の touched-path
checkout・index 復元・ref CAS と buffer 退避・receipt を共有し、適用結果の
手編集 guard は現在 HEAD（先行 pick を含む）へ replay して再構成する。


### Rebase abort の比較基準 (#534)

復元先は ORIG_HEAD のまま。非競合ファイルの手編集保護に使う適用結果は、
`base = REBASE_HEAD の親`、`ours = 現在の HEAD` (移動先 + 適用済みコミット)、
`theirs = REBASE_HEAD` で再構成する。移動先から入った正常な差分を手編集と誤判定しない。
本当の staged / unstaged 編集は従来どおり abort を拒否する。
復元後は rebase directory に加え、libgit2 が残す `REBASE_HEAD` も削除する。

### Skip
- **rebase / cherry-pick / revert のみ**(merge では非表示)。sequencer の現 step をスキップ
- Skip は first click（arm）で accepted full `ConflictRevision` を凍結する。
  revision は operation kind、step commit / `REBASE_HEAD` / `CHERRY_PICK_HEAD` /
  `REVERT_HEAD`、`rebase-merge/msgnum` / `end`、`sequencer/todo` head と
  全 operation metadata / index を既存 `conflict_observe` の一つの観測で束縛する。
  plan はこの revision と operation を持ち、execute は新しい観測へ承認を更新しない。
  refresh 時の disarm は維持し、plan 時・buffer 保全前と `git <op> --skip` spawn
  直前にも同じ revision を確認する。別 step / revision は `SkipStepChanged` として
  mutation 前に拒否し、Refused receipt と EN/JA の最新状態確認案内へ渡す。（#1746）
  実行時の拒否でも typed blocker を表示経路へ保持し、英語の durable receipt と
  log contract とは別に footer / unattended toast を EN/JA で案内する。
- exit 0 だけでは進捗の証拠としない。operation / unmerged index が残るときは、
  前後で観測した marker / counter の移動が必要で、不変・欠損・矛盾は `Unclear`。
  UI は既存 Unknown の warning / partially-applied footer と照合保持を使い、
  Success にしない。非 zero でも次 conflict への観測済み移動は成功のまま。
