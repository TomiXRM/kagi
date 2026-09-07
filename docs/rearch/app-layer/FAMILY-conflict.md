# #484 conflict family application boundary — r1

状態: **設計案 r1**。コード変更・cargo・G/E/M 実行は本 docs PR に含めない。
対象: conflict Continue / Skip / Abort / resolution Save / directory-file resolution。
読解基準: `origin/dev@fe497327`、2026-09-07。

[DESIGN](DESIGN.md) §4/§5.1 の次 family を具体化するノートである。
#534 §2、#540 / PR #567、#569、ADR-0175/0176、および #574 で導入予定の
ADR-0182 `SessionId` を入力とする。

## 1. 結論

conflict mutation を一つの application boundary の後ろへ移し、公開 lifecycle を次に揃える。

```text
prepare -> approve -> run -> apply
```

application boundary は operation identity、承認、repository writer lease、終端分類、記録、
reconcile を所有する。`ConflictView` は GPUI entity と一時的な編集 gesture を引き続き所有するが、
operation を実行できるか、結果が何であったかの authority にはしない。

採用する中心契約は以下。

1. 全 conflict write は app の `SessionId`、canonical `WorktreeId`、
   `ConflictRevision`、編集を伴う場合は `BufferRevision` に束縛した不変の
   `PreparedConflict` を使う。
2. `approve` は prepared value を一度だけ消費し、同じ host turn で repository writer lease を
   reserve する。承認と予約の間に二重操作、stale view、別 writer を割り込ませない。
3. `run` は有限 typed `ConflictJob`。fresh Backend boundary を所有し、typed report を返す。
   `KagiApp`、`Window`、`View`、mutable tab を capture しない。
4. Backend が受理済み試行を記録してから report を返す。UI は outcome を推測せず oplog を書かない。
5. `apply` は表示 delivery を作る前に app state と lease を必ず settle する。tab の close、switch、
   reopen で捨ててよいのは表示だけで、receipt、終端性、reconcile state は失わない。
6. process の停止を証明できない結果は `Failed` ではなく `Unknown`。writer lease を保持する。
   repository read は reconcile の材料にはなるが、process 停止の証明や acknowledge の権限にはしない。
7. Continue / Skip / Abort は dispatch 時に承認対象の conflict revision を同期的に無効化する。
   古い `ConflictView` から in-flight 中や次 sequencer step への移行後に Save / Continue できない。

これは conflict 意味論の書き直しではなく ownership migration である。既存の pure plan、
resolution recovery buffer、sequencer 分類、feature 別 Backend logic を boundary 内で再利用する。

## 2. 範囲と根拠

### 2.1 対象 operation

| user intent | この family が所有する repository effect |
|---|---|
| Save resolution | 選択した resolution を worktree に書き、index を更新 |
| Resolve directory/file conflict | 選択した D/F resolution を index-only で適用 |
| Continue merge | resolution を stage し、既存 commit flow へ handoff |
| Continue rebase/cherry-pick/revert | resolution を stage して sequencer を進行 |
| Continue stash conflict | resolution を stage し、stash follow-up payload を発行 |
| Skip | recovery buffer を保持し、現在 patch を commit せず active sequencer を進行 |
| Abort | recovery buffer を保持し、guard 済み状態を復元して conflict session を終了 |

Continue merge は commit family を吸収しない。この job の成功は conflict staging の完了を意味し、
commit panel の提示を許すだけである。後続 commit は別 plan、別承認、別 receipt を持つ writer とする。
同様に stash Continue は stash を drop せず、stash family が扱う owner/full-OID 束縛済み follow-up を
発行するだけである。

### 2.2 現状の穴

- #534 では大きな rebase が 3 conflicts → 4 → 3 と進むこと自体は正常だった。一方、full reload 中に
  `ConflictView` が前 step の session を保持し、古い resolution buffer を新しい index へ渡し得る。
  path count や path 名の一致は session identity にならない。
- PR #537 は rebase Abort の復元基準を直し、sequencer progress を表示した。この Backend 意味論を維持し、
  application が別の abort base を再構成しない。
- PR #539 は full reload より先に conflict mode を detect し、Continue / Skip の体感 latency を改善した。
  operation ownership と stale-session 排除は追加していない。
- #540 / PR #567 は repository state から Skip を `Finished` / `Advanced` / `NoProgress` /
  `Unclear` に分類した。この分類を維持し、外側に lease と recording contract を追加する。
- #569 には二つの safety gap がある。`GitError::TerminationUnknown` が generic error に包まれて
  `Failed` と記録されること、Continue / Skip / Abort が legacy busy を見るだけで owner lease を
  reserve しないこと。
- 現 UI は一部 conflict receipt を自分で書く一方、D/F executor は Backend 経路で書く。同じ accepted
  attempt の recording owner が button によって異なる。

### 2.3 前提と migration 中の互換

実装は ADR-0182 / PR #574 の `SessionId` を前提とする。本 docs は #574 より先に merge 可能だが、
最初の code slice は merge 済み ADR-0182 API の上に置く。

conflict family は `TabId`、incarnation、`SessionId` を生成しない。ADR-0182 に従い `KagiApp` が
session を生成・attach し、この family はその identity を消費する。path や active tab index で代用しない。

段階移行中は既存 legacy-busy bridge を両方向で維持する。新 conflict lease は未移行 writer を止め、
legacy writer は新 conflict approval を止める。bridge の撤去は後続の全 writer 移行で行い、
この family 単独では行わない。

## 3. identity と authoritative state

### 3.1 owner、repository、二つの revision

`SessionId`
: 表示/session owner の `{ TabId, incarnation }`。completion が今 attach されている view を
  更新してよいかを決める。同じ path を close → reopen しても別 owner になる。

`WorktreeId`
: session attach 時に凍結した canonical repository-write identity。lease、Backend target、receipt、
  reconcile state を scope する。

`ConflictRevision`
: action を prepare した conflict state の opaque Backend fingerprint。見た目が同じ後続 conflict に
  古い buffer や approval を適用させない。

`ConflictRevision` は少なくとも以下を lossless な equality semantics で含めるか hash する。

- canonical worktree identity。
- repository operation kind（merge / rebase / cherry-pick / revert / stash）。
- `HEAD` と利用可能な operation-specific identity（`MERGE_HEAD`、`REBASE_HEAD`、
  `CHERRY_PICK_HEAD`、`REVERT_HEAD`、stash owner/OID など）。
- sequencer kind と step identity。表示用 step count だけにはしない。
- 全 conflicted path の ordered unmerged-index stage OID/mode。
- relevant operation-state contents（rebase の todo/done、msgnum/end など）の digest。mtime は使わない。

rebase step N と N+1 が同じ path、同じ conflict 数でも revision は変わらなければならない。
timestamp、entity ID、conflict count、path list のいずれも単独では revision にしない。

`BufferRevision`
: `ConflictView` が渡す owned resolution draft の fingerprint。conflict revision、選択、resolved bytes
  または content digest、mode、recovery-buffer generation を含み、semantic edit ごとに変わる。
  scroll、focus、pane geometry は含めない。

application が全 keystroke を mirror する必要はない。view は `prepare` 時に owned `ConflictDraft`
snapshot と revision を渡し、Backend preflight が conflict/draft identity を mutation 直前に再検証する。

### 3.2 session state

`Sessions` は `SessionId` ごとの conflict projection を持ち、lease/reconcile は `WorktreeId` を使う。

```text
ConflictState
  Absent
  Observed { revision, summary }
  InFlight { op_id, revision }
  NeedsReconcile { op_id, termination, last_read? }
  Settled { revision? }
```

この state を遷移させるのは app layer だけ。`ConflictView` は snapshot を描画できるが、in-flight や
stale revision を executable に戻せない。

session-keyed value は表示/intent の projection である。既存 operation table は引き続き
`OperationId` key で in-flight job を所有し、reconcile は repository scope とする。detach により
`ConflictState::InFlight` が表示不能になっても authoritative operation は削除しない。

attach 時は Backend の現在 conflict state を observe する。detach は表示所有の conflict state と
未承認 prepared value を消すが、running operation、repository lease、receipt、reconcile record を消さない。
ADR-0182 の lifetime split に従う。

## 4. application contract

### 4.1 finite typed value

正確な Rust 名は既存 module convention に合わせてよいが、boundary は以下と等価な有限 owned value
だけを公開する。

```rust,ignore
enum ConflictRequest {
    Save { path: RepoPath, draft: ConflictDraft },
    ResolveDirFile { path: RepoPath, choice: DirFileChoice },
    Continue { draft: ConflictDraft },
    Skip { draft: ConflictDraft },
    Abort { recovery: ConflictRecoveryRef },
}

struct PreparedConflict {
    request_id: ConflictRequestId,
    owner: SessionId,
    repo: WorktreeId,
    revision: ConflictRevision,
    buffer_revision: Option<BufferRevision>,
    action: ConflictAction,
    plan: ConflictPlan,
    policy: ExecutionPolicy,
}

enum ConflictJob {
    Save(ApprovedSave),
    ResolveDirFile(ApprovedDirFile),
    Continue(ApprovedContinue),
    Skip(ApprovedSkip),
    Abort(ApprovedAbort),
    RecordRefusal(ConflictRefusal),
}
```

`PreparedConflict` と approved payload の field は app module 外から forge できない private value とする。
job は owned value だけを持ち、`Entity`、GPUI context、tab から借用した mutable Backend、
`KagiApp` callback を保持しない。

conflict 専用の第二 plan slot は作らない。既存 finite enum を
`Planned::{Remove, Stash, Conflict}`、`FamilyEvidence::{Remove, Stash, Conflict}` のように拡張し、
既存の単一 `PlanState` が `PreparedConflict` を所有する。上記 `ConflictState` は session の
observation/in-flight projection であり、prepared capability の別 authority ではない。

I/O を伴わない request/revision/progress/outcome の pure value は `kagi-domain`、repository observation
と execution は `kagi-git` に置く。app module は両者を compose するが、`std::process` や直接の
filesystem mutation を持たない。

### 4.2 `prepare`

`prepare_conflict(session, request, policy)` は次を行う。

1. attach 中 `SessionId` から凍結済み `WorktreeId` を解決する。
2. detached owner、既に in-flight の owner、非互換 app state を拒否する。
3. fresh Backend boundary を開き、現在の conflict observation を読む。
4. request の conflict revision と buffer revision を検証する。
5. 既存 typed Backend plan と user-visible summary を作る。
6. target、action、recovery requirement、`ExecutionPolicy` を `PreparedConflict` に凍結し、
   current plan revision と modal owner が一致する場合だけ単一 `PlanState` に採用する。

prepare は repository を mutate せず、long-lived writer lease も取らない。prepare と execution の race は
approve 時の atomic reserve と Backend preflight で閉じる。

既存 UX に confirmation がある action は opaque prepared value を host が保持・表示する。

- sequencer Continue は confirmation modal を維持する。
- Abort は二段階 armed confirmation を維持するが、二段目の authority は UI bool ではなく prepared plan。
- Skip は明示 destructive confirmation を維持する。
- Save、D/F resolution、merge staging、stash staging は明示 action click 自体を approval とし、
  追加 modal を作らない。

`KagiApp` の active modal は一つのまま。conflict prepare が独立 modal stack を作ったり、
remove/stash/別 plan を黙って置換したりしない。cancel は prepared capability を drop し、後続 approve を
invalid にする。

prepare failure は non-mutating。accepted direct user attempt の後なら app は record-only job を作り、
Backend が refusal/failure を一度だけ記録する。UI writer へ戻さない。

### 4.3 `approve`

`approve_conflict(token, legacy_busy)` は単一 `PlanState::Ready` の
`Planned::Conflict(prepared)` を一度だけ消費し、同じ同期 host turn で次を行う。

1. `SessionId` がまだ attach 中で `WorktreeId` が不変か確認する。
2. observed conflict revision と buffer revision が一致するか確認する。
3. 再利用 request ID / approval token を拒否する。
4. legacy busy と相互運用しながら repository writer lease を reserve する。
5. owner conflict state を `InFlight` にして古い action を無効化する。
6. 有限 `ConflictJob` を一つだけ返す。

4 と 5 の間に `await`、process spawn、repository write、UI callback を挟まない。Busy/stale refusal は
job を dispatch せず、先に lease を持つ writer の state を変更しない。

UI は approved revision の全 mutation control を即時 disable する。draft、progress、recovery state の
表示は続けてよいが、その draft を新しい Save/Continue request として再送できない。

遅延 confirmation 中は resolution editing を freeze する。編集を再び許可する場合は prepared value を
drop し、新しい `BufferRevision` から prepare し直す。直接 Save/D/F は通常 prepare と approve を
同じ host turn で行う。

### 4.4 `run`

`ConflictJob::run` は frozen target の fresh Backend を解決し、family boundary を実行する。

```text
preflight -> execute -> verify -> record -> return report
```

trust、repository state、progress、verification、terminal classification、durable receipt の authority は
Backend。GUI reload を verify の代用にしない。

report は少なくとも以下を持つ。

- operation/request/owner/repository identity。
- frozen before evidence と observed after evidence。
- どの mutation boundary を越えたかを示す `ProgressEvidence`。
- process-backed action の `TerminationEvidence`。
- `Success | Refused | Failed | Partial | Unknown`。
- 安全に read できた場合の fresh `ConflictObservation`。
- recovery/autosave evidence。
- recording success または append failure。

accepted attempt は report が app layer に戻る前に記録する。append failure は report に残して表示し、
UI が二件目の receipt を合成しない。

GUI では `KagiApp::dispatch_job` だけが background spawn と main-thread `apply` 復帰を担当する。
process-backed Continue/Skip と大きな Abort は foreground を占有しない。小さな Save/D/F の scheduling は
最初の ownership slice では同期維持も許すが、同じ job/report/lease contract を通し、重い I/O の計測後に
adapter の dispatch 方法だけを変えられるようにする。

### 4.5 `apply`

`apply_conflict_completion` だけが完了後の host state を mutate する。

1. operation ID で重複排除する。
2. report と recording status を保存する。
3. verified conflict observation を適用するか `NeedsReconcile` に入れる。
4. typed termination evidence から writer lease を release/retain する。
5. app state を settle してから表示 work を作る。
6. 同じ `SessionId` が attach 中で expected operation/revision が view owner の場合だけ delivery を出す。

delivery は `ConflictAdvanced`、`ConflictCleared`、`OpenMergeCommit`、
`PublishStashFollowup`、`NeedsReconcile`、`ShowError` のような有限 value とする。UI は表示できるが
outcome、receipt、lease、revision を変更しない。

tab switch/close/reopen で old completion を別 owner に向け直さない。detached owner の completion も
repository operation と receipt は settle する。同じ canonical worktree の新 incarnation は fresh state を
observe し、旧 owner の toast/follow-up を受け取らない。

## 5. operation 別 contract

### 5.1 Save resolution

- prepare は exact path、conflict revision、buffer revision、result bytes、mode expectation を束縛する。
- preflight は conflict/index entry、draft、path、trust の drift を mutation 前に拒否する。
- execute は既存 resolution-save Backend path を使う。UI の filesystem write/staging call は残さない。
- verify は exact path の worktree content と index stage を読む。intended bytes/mode と unmerged stage の
  解消を Success の条件とする。
- worktree content を書いた後で index update/verify に失敗した場合は `Failed` でなく `Partial`。
- app が verified Success を apply するまでは resolution autosave を recovery evidence として保持する。

### 5.2 directory/file resolution

- prepare は conflict revision、target path、D/F choice、exact stage OID/mode を束縛する。
- execute は index-only のまま `ops/dir_file_conflict.rs` の plan/preflight/execute を再利用する。
- verify は intended index shape が D/F conflict を置換したことを証明する。
- common recorded boundary に統一し、現在の Backend persistent oplog と UI non-persistent record の
  split をなくす。

### 5.3 Continue

Continue plan は一つの typed route を返す。

`MergeStage`
: resolution draft を apply/stage する。verified Success の delivery が既存 commit panel を開いてよい。
  job 自体は merge commit を作らない。

`SequencerAdvance`
: draft を apply/stage し、該当 `--continue` を実行後に repository state を分類する。次 step が conflict
  なら新 `ConflictRevision` を持つ successful advance、sequencer 終了なら successful finish。

`StashStage`
: draft を apply/stage する。verified Success を app が settle した後に限り、既存 owner/full-OID 束縛済み
  stash follow-up payload を発行する。

Continue は sequencer command より前に stage するため、その後の既知 command failure を常に
`Failed` とはしない。staging が mutation boundary を越えていれば `Partial`。stage 後に termination が
unknown なら `Unknown` とし、§6 に従い lease を保持する。

fresh verified observation を full repository reload より先に app conflict state へ適用する。reload は
graph/status 表示更新には使うが、現在の conflict session を決める仕組みにはしない。PR #539 の latency
改善を維持しながら #534 の stale-session window を閉じる。

### 5.4 Skip

Skip は sequencer 起動前に resolution recovery buffer を保存する。#540 / PR #567 の分類をそのまま使う。

- `Finished`: `Success`、conflict state cleared。
- `Advanced`: `Success`、new conflict revision observed。
- `NoProgress`: attempt が repository を mutate していないと progress evidence で証明できる場合だけ
  `Failed`。
- `Unclear`: `Unknown`。`Failed` に変換しない。

I/O error や `GitError::TerminationUnknown` は typed termination cause のまま `kagi-git` → app report →
recording → UI message を通す。`GitError::Other` の文字列へ包むことを禁止する。

### 5.5 Abort

Abort は PR #537 の reconstruction を維持する。特に rebase abort は仮定した original base でなく現在の
mid-operation `HEAD` と比較・復元する。real mid-conflict edit の既存 refusal も維持する。

prepare は operation kind、conflict revision、recovery buffer、guarded refs/index、restore plan を束縛する。
execute は restore boundary ごとの progress を返す。worktree/index/ref restore 開始後の failure は
`Partial`、停止不明は `Unknown`。relevant operation state の消滅を verify した場合だけ app conflict state を
clear する。

stash-conflict Abort は stash-family semantics を維持し、drop follow-up を作らない。後続 stash action は
別に prepare した stash job とする。

## 6. outcome、termination、lease、reconcile

### 6.1 outcome matrix

| evidence | outcome | writer lease | app state |
|---|---|---|---|
| mutation 前の preflight refusal | `Refused` | release | conflict を保持/re-observe |
| mutation 前の known failure | `Failed` | release | conflict を保持/re-observe |
| intended effect を verify | `Success` | release | fresh observation を apply |
| mutation 済み、process 停止済み、intended effect 未検証 | `Partial` | release | safe read を適用、または reconcile |
| terminal effect 不明、process 停止確認済み | `Unknown(stopped)` | read + acknowledge まで retain | `NeedsReconcile` |
| process 停止未確認 | `Unknown(unconfirmed)` | retain | `NeedsReconcile`、ack disabled |

「conflict が無い」だけで uncertain attempt を Success にしない。intended operation の一部だけが effect を
作った可能性がある。逆に Continue 後の通常の next-step conflict は、sequencer advance が証明できれば
failure ではない。

### 6.2 termination evidence を潰さない

CLI/transport boundary は typed termination evidence を返す。error string は表示だけに使う。

```text
Exited(status)
StoppedAfterCancel(reason)
Unconfirmed(reason)
```

`Unconfirmed` に対して UI/app が PID や lock file から liveness を推測しない。bare PID は再利用や detached
descendant のため不十分。現 runner は process-tree supervisor を持たないので lease を保持し、ack 不可と
表示するのが安全側の実装である。#507 の将来 supervisor が stronger typed stop evidence を供給してよいが、
この family で弱い代用品を作らない。

`TerminationUnknown` を `Failed` に downgrade する、lease を捨てる、timer 経過を停止とみなすことを禁止する。

### 6.3 read と acknowledge

reconcile は二つの明示 app action に分ける。

1. `read_conflict_reconcile(repo, op_id)` は fresh Backend だけを通し、HEAD、index、sequencer marker、
   operation step、conflict fingerprint、repository lock を読む。
2. `ack_conflict_reconcile(repo, op_id, read_id)` は user が確認した exact read を一度だけ消費する。

`Unknown(stopped)` は fresh read 成功後に ack を可能にする。ack は reconcile item を archive して lease を
release するが、元 receipt を Success に書き換えず command を retry しない。

`Unknown(unconfirmed)` では read が表示 evidence を更新しても ack は disabled。将来 execution owner が
stronger typed stop evidence を出せる場合だけ状態を進め、その後にも fresh repository read を要求する。
repository consistency、lock 不在、経過時間、tab close は process 停止の独立証明ではない。

read/ack は operation-ID bound かつ idempotent。ある attempt の read で newer lease を release せず、
duplicate completion で acknowledged item を復活させない。app crash/restart と process-tree recovery は
この family の対象外で #507 に残す。app-controlled close/Quit は lease がある間保留する。

`Partial` / `Unknown` の conflict operation を自動 retry しない。

## 7. `ConflictView` と app layer の境界

### 7.1 `ConflictView` に残すもの

- GPUI entity/input state。
- render、focus、scroll、pane selection、keyboard gesture。
- 一時 resolution editing と diff presentation。
- owned `ConflictDraft` snapshot の構築。
- prepared plan、progress、reconcile evidence、notice、delivery の表示。
- typed intent の `KagiApp` への dispatch。

既存の child → parent deferred action pattern は維持する。entity callback は intent を enqueue し、
entity borrow の終了後に `KagiApp` が app API を呼ぶ。

### 7.2 app layer が所有するもの

- current view から `SessionId` / frozen `WorktreeId` への mapping。
- authoritative observed conflict revision と in-flight state。
- prepare/approval identity。
- writer admission と legacy-busy interoperability。
- finite job と completion apply。
- lease、reconcile item、receipt、display eligibility。
- verified merge success の commit panel routing と、verified stash success の follow-up routing。

### 7.3 Backend が所有するもの

- repository/trust 解決。
- conflict detection と revision 導出。
- plan、preflight、execute、verify。
- filesystem、index、refs、sequencer、child process interaction。
- progress/termination evidence。
- single durable recording boundary。

各 action の移行時に以下の UI behavior を削除する。

- direct `Backend` conflict writer call。
- `reject_if_busy` だけの admission。
- UI-side before/after 構築と `record_op(_persist)`。
- error text からの `Failed` 推測。
- full reload / `detect_conflict_mode` timing を session identity とする処理。
- app settlement より前の commit/stash follow-up 発行。

`ConflictView` が control を optimistic disable してもよいが app state を authority とする。delivery の
`SessionId` と revision を照合してから displayed entity を置換する。

## 8. 段階 PR 案

全 slice は cumulative だが、それぞれ自分の G/E と共に単独で release/merge 可能にする。後続 slice が
入らないと安全にならない中間状態を作らず、移行対象 action の legacy path だけを削除する。
ADR-0182 / #574 を最初の code slice の前提とする。

### PR C1 — Save / D/F の縦断実証

実装状況: `feat/conflict-c1-save-df` で完了。Save/D/F のみを有限 job に移し、後続 C2/C3 の
Abort/Continue/Skip は既存経路のまま残す。

- conflict request/revision/report type、finite `Planned`/`FamilyEvidence` variant、conflict owner state を
  `Sessions` に追加。plan slot は既存の一つだけ。
- Save と D/F に `prepare -> approve -> run -> apply` を接続。
- shared repository lease を reserve し legacy bridge を維持。
- Save/D/F を一つの Backend recorded boundary に置き、対象 action の UI receipt writer を撤去。
- exact conflict/buffer revision を束縛し fresh verified observation を apply。
- Continue/Skip/Abort は未変更。ただし C1 job 中は mirrored busy により拒否される。

GPUI entity や filesystem access を `src/app` に移さず、編集データを boundary 越しに運べることを証明する。

### PR C2 — Abort family

- merge/rebase/cherry-pick/revert/stash conflict の typed Abort prepare/job/report を追加。
- PR #537 reconstruction、mid-conflict edit guard、autosave、recovery を維持。
- progress-aware `Partial`/`Unknown` と Backend-only recording を追加。
- 二段階 Abort UI は UI bool を authority にせず app-prepared approval を運ぶ。
- reload 前に cleared/remaining conflict state を apply。unknown termination は owner lease を保持。

C2 は Continue/Skip を変えず、それらが legacy bridge を使う状態でも安全に ship できる。

### PR C3 — Continue / Skip / reconcile

- 全 Continue route と Skip を finite job set へ追加。
- #540/#567 `SkipProgress` を維持し、`GitError::TerminationUnknown` の消去を止める。
- typed process-stop evidence を保ち、`Unknown` lease retention と stopped Unknown の明示 read/ack を追加。
  unconfirmed termination は #507 まで ack 不可。
- verified next conflict revision を即時 apply。full reload は display refresh のみ。
- app settlement 後だけ merge success を commit、stash success を owner-bound follow-up へ route。
- 残る direct conflict writer、UI outcome inference、UI recording を撤去。

C3 後に本書の全 operation が boundary を使う。compatibility bridge の撤去は global writer migration 待ち。

各 PR は app-layer migration ledger と該当 ADR を更新する。実装で本書にない semantic decision が必要なら、
mechanical extraction に混ぜず code より先に ADR を追加・更新する。

## 9. 検証計画

### 9.1 G — window なしの公開 API test

G は public app method と finite typed fake を使い、private Sessions field や GPUI window に依存しない。

owner/admission 共通:

- prepare が `SessionId`、`WorktreeId`、conflict revision、buffer revision、policy を凍結し、いずれの
  drift も approval/preflight で拒否。
- 同じ conflict path の連続 rebase step が異なる revision となり、old Save/Continue を拒否（#534 shape）。
- double approval と duplicate completion を拒否/no-op。
- conflict job が editor/stash/remove/stage/fetch/他 writer を止め、代表 legacy writer も conflict approval を
  止める両方向 test。
- tab switch で progress を誤配送しない。same path close/reopen は old delivery を受けず、receipt/lease は
  正しく settle。
- Backend target mismatch は mutation 前に refusal。
- accepted attempt ごとに Backend が一度だけ receipt append を試み、record failure を report。
  UI の fallback writer は存在しない。

Save/D/F:

- Save Success で exact bytes/mode/index stage を verify。
- buffer/index stage drift、binary/symlink mode、same-path/new-revision を無変更で refusal。
- worktree-written/index-failed は recovery evidence 付き `Partial`。
- D/F choice ごとの intended index shape と stale choice refusal。

Continue/Skip:

- merge Continue は stage 後 `OpenMergeCommit` delivery。job 自身は commit を作らない。
- stash Continue は apply 後に owner/full-OID follow-up を一度だけ発行。
- rebase conflict count 3 → 4 → 3 で old revision を受理しない。
- sequencer Continue の finished / advanced-to-conflict / staging 後 no-progress (`Partial`) /
  termination-unknown。
- Skip の `Finished` / `Advanced` / `NoProgress` / `Unclear` と recovery data 保持。
- termination-unknown が report/receipt まで typed のまま残り string wrapping で `Failed` にならない。

Abort:

- 1,679 commits 相当の large-rebase fixture が current mid-operation `HEAD` を使って abort。
- genuine mid-conflict edit は無変更 refusal。
- 各 restore boundary の前後 fault が progress evidence により `Failed` / `Partial`。
- verified Abort は matching conflict session だけ clear し stash drop follow-up を作らない。

Unknown/reconcile:

- `Unknown(stopped)` は lease を保持し、fresh read + matching ack だけで release。original outcome は不変。
- `Unknown(unconfirmed)` は read、ack attempt、tab close、owner detach を跨いで lease を保持し、
  current-family API から停止確認を捏造できない。
- 別/旧 stopped attempt の read ID では current reconcile item を acknowledge できない。
- duplicate/late report が newer lease を release せず newer conflict revision を上書きしない。
- Continue/Skip/Abort の自動 retry 経路がない。

fault は prepare/preflight/mutation/verify/record/termination/read/ack の typed fake result で注入し、
timing sleep を使わない。

### 9.2 E — focused GUI adapter scenario

GUI runner は毎回 `KAGI_GUI_E2E_ONLY=<scenario>` で **一 scenario だけ**実行する。layout coverage が
macOS window を危険な数まで作るため full runner 実行は禁止。全 scenario で共通 unmount helper を使い、
leak detector を含め exit 0 を確認する。

`conflict_save_boundary`
: 実 keystroke → Save、bytes/index/receipt を検証。conflict revision advance 後に同じ path の stale save を
試し refusal を確認。

`conflict_dir_file_boundary`
: 実 control で代表 D/F choice を選び index/receipt を検証。owner lease 中は control が disable。

`conflict_continue_boundary`
: path/count shape を繰り返す deterministic multi-step sequencer を進め、full reload より前に new revision を
observe。old view が action を再送できないことを確認。merge-commit routing と stash-follow-up routing は
一 fixture に詰めず必要なら別 filtered scenario にする。

`conflict_skip_abort_boundary`
: 実 button/key から confirmation、Skip classification、Abort arming、recovery notice を通し、matching
session だけが更新されることを確認。

`conflict_unknown_reconcile`
: finite app/backend seam で `Unknown(stopped)` / `Unknown(unconfirmed)` を返す。footer/modal、writer disable、
app close/Quit hold、read/ack availability、stopped + fresh read 後だけの release を確認。migration 中は
unrelated repo も既存 global bridge 方針どおり Busy であることを確認する。

E が証明するのは adapter wiring、focus/confirmation、visible state、cleanup。race を作る slow shell child は
使わない。process/outcome matrix は G、実 process cancellation は platform behavior が重要な focused M とする。

### 9.3 M — latency の実機確認

C3 merge 前に #534 の large-rebase を実 app で再現する。Continue/Skip/Abort 中も render が応答し、
progress が current sequencer step を示し、full graph reload 待ちなしに next conflict view が出て、
owner tab close でも active writer が解除されないことを確認する。M は G/E の代替ではない。

## 10. 対象外と safety boundary

- diff algorithm、merge editor layout、commit panel、stash drop semantics、Git sequencer command の再設計。
- GPUI entity の app layer 移動、または filesystem/process access の `kagi-domain` / `src/app` への移動。
- repository path、tab index、entity ID、conflict path list を operation owner とすること。
- reload、conflict 不在、exit string、timeout 経過から Success/停止を推測すること。
- uncertain mutation の auto retry。
- unconfirmed writer の tab close による解除。application-controlled close/Quit は lease で保留する。
  GPUI が veto できない Dock/OS Quit、crash/restart recovery、process-tree supervision は DESIGN、
  ADR-0175、#507 のとおり本 family の保証外。
- 全 competing writer が shared app boundary に移る前の legacy admission 撤去。

## 参照

- [Application-layer DESIGN](DESIGN.md) §4 / §5.1
- [ADR-0175: application-owned remove boundary](../../adr/0175-app-remove-boundary.md)
- [ADR-0176: application-owned local stash boundary](../../adr/0176-app-stash-local-boundary.md)
- [ADR-0182 / PR #574: session identity and lifetime](https://github.com/TomiXRM/kagi/pull/574)
- [Issue #507: process-tree termination](https://github.com/TomiXRM/kagi/issues/507)
- [Issue #534: large rebase analysis](https://github.com/TomiXRM/kagi/issues/534)
- [PR #537: rebase abort and progress](https://github.com/TomiXRM/kagi/pull/537)
- [PR #539: post-continue/skip conflict refresh](https://github.com/TomiXRM/kagi/pull/539)
- [Issue #540](https://github.com/TomiXRM/kagi/issues/540) / [PR #567](https://github.com/TomiXRM/kagi/pull/567):
  sequencer Skip classification
- [Issue #569: conflict termination/admission gaps](https://github.com/TomiXRM/kagi/issues/569)
