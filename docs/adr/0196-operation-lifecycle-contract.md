# ADR-0196: operation lifecycle を唯一化する — Wave 0 契約の固定

- Status: Accepted (Wave 0 契約; Wave 1–4 実装済み、Wave 3 は #728 で完了 — 決定 5 の表を参照)
- Date: 2026-09-12
- Related: [#643](https://github.com/TomiXRM/kagi/issues/643)（A0 / A1 / A2）、ADR-0104（run pipeline）、ADR-0149（oplog）、ADR-0177（TerminationUnknown）、ADR-0183（session-owned read）、ADR-0195（FailureCode）
- 適用範囲: `src/app`、`src/ui/operations/*`、`crates/kagi-git/src/backend/*`、`src/remote/*`

## 文脈

Kagi の write は**二系統**で動いている（#643 A0）。

| | 移行済み family（remove / stash / conflict C1） | legacy family |
| --- | --- | --- |
| 入口 | `Sessions::write_lease` → `WriteGuard` | `busy_op = Some(...)` を立てて task を投げる |
| 終端 | `Sessions::apply(Completion)` が settle（`settled` set で一回性、lease 解放、reconcile 登録、Delivery 配布） | `finish_op_on_main` が `busy_op` を無条件解放し、`repo_path + switch_generation` 不一致なら**表示を捨てる** |
| Quit / close 保留 | `may_close_host()` = lease の有無 | **守られない** |
| Unknown | reconcile entry に入り、`stopped == false` なら lease を保持 | `settle_conflict_write` の bridge が一部を救うだけ |
| 記録 | `Recording` が `ExecutionReport` に乗って戻る | `record_op_impl` が append 失敗を `klog!` に出すだけだった（#689 で toast 化） |

legacy 側は 17 file / 約 30 call site に及ぶ（`branch` 6、`pull_push` 5、`mod` 4、`merge` 3、
`cherry_revert` 2、`worktree` / `tag` / `reset` / `remote_branch` / `rebase` / `history` /
`force_lease` / `discard` / `commit` / `checkout` / `github` / `branch_cleanup` 各 1）。

この二系統が実際に欠陥を生んだことは、2026-09-11〜12 の修正で確認されている:

- #675: `stage_file` と `stage_files` の**二系統に同じ誤った推論が重複**していた（sparse でデータ喪失）
- #646: pull の fetch は記録し、単独 fetch は記録しない
- #689 / #690 / #691: 「結果」と「記録できたか」を 1 つの値に畳み、receipt を捨てる経路が 3 つ

**Big Bang は Git の意味論を書き換えない。** 置き換えるのは、上に載る「誰が許可し、誰が終端を確定し、誰が結果を配るか」の配線である（#643 §4）。本 ADR はその配線の型と状態表を、**コードを動かす前に**固定する。

## 決定 1 — lifecycle 型（OperationKind ごとに同一形）

```
PlanRequest → PlanCompletion → Approved → RunningWrite → ExecutionReport → Delivery
```

| 段 | 型 | 状態 | 備考 |
| --- | --- | --- | --- |
| PlanRequest | family ごとの request（例: `StashRequest`） | 既存 | adapter が `ResolvedAttachment` を添えて core に渡す |
| PlanCompletion | `flow::PlanCompletion { revision, state, error_job }` | 既存 | `is_current(sessions)` で revision 照合。古い完了は捨てる |
| Approved | `flow::Approved` + `PlanToken { revision }` | 既存 | **one-shot**。`invalidate_plan()` で revision が進めば無効 |
| RunningWrite | `session::WriteGuard { scope, id }` | 既存（名称は `BeginWrite` API として統一する） | Drop は解放**しない**。`complete` / `complete_git` のみが解放 |
| ExecutionReport | `flow::ExecutionReport { recording, evidence }` | 既存だが `FamilyEvidence` が 4 family のみ | **全 family の variant を持つ**ように拡張する（決定 2） |
| Delivery | `session::Delivery` | 既存 | `Completed { id, attachment, report }` / `Invalidate` / `RemovedTarget` / `RemoteCompleted` |

### 新設: `OwnerStamp`

```rust
/// 承認時に凍結される配送先。実行中に再解決しない。
pub struct OwnerStamp {
    pub session: SessionId,   // tab + incarnation
    pub visit: u64,           // 離脱で進む。古い visit への proposal は作らない (#557)
    pub operation: OperationId,
}
```

`Attachment` は既に `session` / `visit` を持つ。`OwnerStamp` はそこに `OperationId` を束ね、
**completion の routing 鍵**として `Delivery::Completed` に載る。legacy の
`repo_path + switch_generation` 比較（path 文字列への退化）はこれで置き換える。

### `BeginWrite` の契約（A0 決定）

```rust
BeginWrite { owner: Attachment, scope: WriteScope, kind: OperationKind }
    -> Result<RunningWrite { operation_id, owner_stamp, lease }, AdmissionError>
```

- **task spawn の前に一回だけ**呼ぶ。`AdmissionError::{Busy, StaleApproval, NeedsReconcile, Identity}` は既存
- 現状 `write_lease` が `LegacyBusy` を引数に取るのは、legacy `busy_op` との相互排他のため。**最後の family 移行で削除**する

## 決定 2 — 有限状態表

### 2.1 `SemanticOutcome`（= `oplog::OpOutcome`、変更なし）

| 値 | 意味 | lease | reconcile |
| --- | --- | --- | --- |
| `Refused { blockers }` | plan 時 blocker で実行せず | 解放 | 不要 |
| `Failed { error }` | 実行したが失敗。**再試行可能** | 解放 | 不要 |
| `Partial { after, error }` | 副作用の一部が適用 | 解放 | 不要（recovery handle は `after.dirty` と `recovery`） |
| `Unknown { after, evidence }` | 終端または副作用が確定できない。**再試行禁止** | `stopped == false` なら**保持** | **必要** |
| `Success { after }` | 完了 | 解放 | 不要 |

`Unknown` は `Failed` の別名ではない（#643 §3 不変）。`TerminationUnknown` と
`StashIdentityUnverified` は必ず `Unknown` に落ちる（`WriteGuard::complete_git`）。

Skip の C0 adapter は `Ok(SkipOutcome)` だけでは lease を解放しない
（#569 (1)）。`conflict_skip.rs` 内で `SkipProgress` を先に分類し、
`Unclear` は停止済み・sequencer の効果不明として既存 guarded settlement へ渡す。
`GuardKind::Sequencer` の既存ルールが lease を保持し、`UnaccountedWrite` の
drain が reconcile requirement を登録する。元の `SkipOutcome.after` と表示用
evidence は維持する。typed `TerminationUnknown` の停止証拠は変更せず、
既知結果の解放も維持する。これは Skip の分類順だけの修正であり、conflict family
全体の移行や read/ack の変更ではない（#569 (2)(3) は別 scope）。

**Continue post-read（#569 (2)）**: `execute_conflict_continue` の stash stage 後、
merge commit 後、sequencer 実行後の `current_state_summary` 失敗だけを
`TerminationUnknown(Termination::Stopped)` とする。実行は戻ったが after は不明で、
既存 `settle_conflict_write` と Sequencer guard により lease を保持し reconcile に登録する。
実行前の拒否・通常 Err・既知の `Staged` は従来どおり解放する。session の汎用 lifecycle、
Abort、read/ack は変更しない。実 stash conflict と壊れた ref を使う回帰で、stage 済みの
post-read 失敗、Unknown・lease 保持・登録、および実行前拒否／既知結果の解放を確認する。

### 2.2 `Recording`（変更なし）

| 値 | 意味 | 呼び出し元の義務 |
| --- | --- | --- |
| `Appended { path, entry }` | oplog に 1 行書けた | — |
| `Failed { attempted, error }` | 書けなかった | **ユーザーに伝える**（#689 の toast / staging の notice）。黙って捨てない |

`ExecutionReport.recording` は**省略不可**。`Result<OperationOutcome, GitError>` だけを返す
経路（`Backend::run`、旧 worker reply、旧 `remote_stash_drop`）は receipt を捨てる経路として
**新規追加を禁止**する。既存の `Backend::run` は test 用 convenience として残すが、
production の adapter は `run_recorded` を使う。

### 2.3 `ExecutionEvidence`（family ごと）

| 値 | 意味 |
| --- | --- |
| `Verified { .. }` | `verify_X` が実行後の状態を読み直し、plan の予測と照合した |
| `Unverified { reason }` | 読み直せなかった。**`Success` に畳み込まない** |
| `NotApplicable` | verify が意味を持たない operation（例: 読み取りのみの補助 write）。**明示**する |

`PlanProjection`（予測）と `ExecutionEvidence`（実測）は別型にする（A2 決定）。
receipt の `after` に予測を書かない。

### 2.4 `ReconcileRequirement`

| 条件 | 値 |
| --- | --- |
| outcome が `Unknown` | `Required { stopped }` → `Sessions.reconcile` に登録 |
| それ以外 | `None` |

`stopped == false` の間、scope の lease は解放されず、同 scope への `BeginWrite` は
`NeedsReconcile` を返す。`ReconcileRead` が `stop_proven` を持って初めて解放する。

**CLI 子孫が残る場合の repository admission（#1780 / #1746）**:
上記の per-`Sessions` lease と、別 Kagi process も排除する repository advisory
lock は別の所有権である。Continue / Skip を含め、repository lock 内の CLI が
`TerminationUnknown` で戻っても、生きた process group がある間は後者を解放しない。
`write_lock.rs` の同一 fd の共有所有権は `Arc` とし、`proc.rs::run_child` は保持中の
repository lock の再入を禁止して**既存の janitor/reaper** へ共有所有権を渡す。
新しい独立 monitor は作らない。quarantine 中の admission を次の command が再利用
することはできない。weak entry は残し、既存 outer guard 内の別 child にも同じ fd を
保持させるため、一つの group の停止で別の生存 writer の lock を解放しない。

共有所有権の移譲は process wait または output collection が Err の未確定実行に限る
（`!group_stopped && (status.is_err() || io.is_err())`）。stdio を閉じた background
helper を post-commit hook が残しても、CLI と collector が正常終了した既知の Success
は本 ADR の settlement 表どおり解放し、repository admission だけを quarantine しない。
その helper の group handle は既存の sweeper が引き続き管理する。

janitor は Sessions reconciliation と共通の group-liveness evidence を用い、
実行中の supervisor job を retire しない `proc::group_alive` で空を確認するまで lock を保持する。親 CLI の return / reap や stdout /
stderr の EOF だけでは解放しない。停止証明が得られなければ fail-closed のままであり、
timeout で admission を再開しない。quarantine 中の repository lock の解放は process の停止証明に、
Sessions lease の解放と reconcile の解決は既存の lifecycle 条件にそれぞれ従う。
この保護は監視する Kagi process が存続し、writer が監視対象 group 内にいることを
前提とする。Kagi の強制終了や group を脱出した任意の外部 writer まで保証しない。

Continue / Skip の draft flush・preservation は repository lock の前に完了し、
draft lock と入れ子にしない。D/F recovery snapshot の ref write はこれと異なり、
repository admission と preflight の**後**に同じ repository lock 内で行う。
有限モデルの `LiveWriterKeepsRepositoryLease` と early-release negative control は
unknown return、pipe EOF、停止証明、Sessions reconciliation を別 transition とする。

**観測不能な remote write の明示解放（#706、2026-09-23）**:
通常の `acknowledge` は引き続き `stop_proven && resolved` だけを受け付ける。
別 tier の `prepare_unobservable_release` → audit job → `acknowledge_unobserved`
は、停止証明済みの既知 remote-write family にだけ提供する。承認時の expectation
が空の場合に加え、remote の観測では frozen remote 名の不在、`ssh -G` の失敗・
timeout、不正 URL を対象とする。未知の family、live writer、観測不一致、通信失敗は対象外。
batch の一部が観測不能でも、別の promise の不一致・通信失敗を優先して拒否する。

UI は既存 `AppNotice` で EN/JA の「remote 側の結果を Kagi は確認していない」
警告を示し、解放準備 → 最終確認の二段階を要求する。Esc・別 modal への置換は
準備状態だけを破棄し、requirement は残す。最終確認後も先に
`reconcile-release-unobservable` の oplog append を行い、元の operation 名・id・
観測不能の英語理由・未検証である事実を `Unknown` outcome に記録する。
append 失敗時は制限を解除せず、永続化エラーと再確認の入口を残す。
元の write の `Unknown` receipt は変更せず、再実行・local snapshot による成功認定も行わない。
`may_close_host` は緩和しない。

SSH identity は既存の pure `repo_identity` と分離した I/O-aware resolver を
PR fetch の remote 選択で使う。`ssh -G` は既存 runner の 60 秒 deadline で実行し、
出力の `hostname` だけを読む。remote-stash の write-profile parser は使わず、
ControlMaster・ProxyJump・agent-only 認証を理由に host 識別を拒否しない。
host 解決の前に既存の pure identity parser で owner/repo を比較し、
異なる path の候補は除外する。同じ path の未識別 alias は一意性を証明できないため拒否する。
SSH command override 下の remote-ref は Git 自身の `ls-remote` を試み、
その失敗は `Err`（通信失敗 tier）のまま保持する。PR fetch の非 alias 候補は
literal host の読みを維持する。override 自体を観測不能の根拠にしない。
未定義・削除済み alias でも `ssh -G` は exit 0 / hostname=alias を返し得る。
その状態を検出したとは主張せず、後続の DNS・接続失敗は解放不可とする。

### 2.5 settle の遷移（`flow::apply`、既存の挙動を契約化）

```
Completion 到着
  ├─ id ∈ settled            → 何もしない（一回性）
  ├─ owner が operations に無い → 何もしない（既に終端化済み）
  └─ それ以外:
       settled.insert(id)
       stopped なら release_lease(scope, id)
       outcome == Unknown なら reconcile.insert(id, ..)
       family ごとの Delivery を生成（Invalidate / RemovedTarget / Completed）
       invalidate 対象の worktree を stale に入れる
```

**受入条件（A0）**: checkout / commit / branch を含む全 family が、host-close / stale-tab /
success / refused / unknown / duplicate-completion の同一 matrix を通る。

## 決定 3 — identity 表

| 型 | 定義場所 | 意味 | path 文字列で代用してよいか |
| --- | --- | --- | --- |
| `TabId` | `app::session` | UI の tab。閉じたら再利用しない | 否 |
| `SessionId { tab, incarnation }` | `app::session` | tab の**世代**。close/reopen で incarnation が進む | 否 |
| `visit` | `Attachment.visit` | 同 session 内の**滞在**。離脱で進む | 否 |
| `OperationId` | `app::session` | 承認された 1 回の write | 否 |
| `RequestId` | `app::session` | plan の revision。`invalidate_plan` で進む | 否 |
| `RepoId(PathBuf)` | `kagi-domain::remove` | 共通 gitdir で同定した repository。lease の scope | **locator としてのみ** |
| `WorktreeId` | `kagi-domain::remove` | worktree の同一性（frozen） | 否 |
| `RemoteRepoId` | `kagi-domain::remote::stash` | `<host>:<root>` | 否 |
| `WriteScope::{Local(RepoId), Remote(RemoteRepoId)}` | `app::session` | lease の単位 | — |
| `Attachment { session, path, worktree, visit }` | `app::session` | 配送先。plan 時に凍結 | `path` は locator |
| **`OwnerStamp`（新設）** | `app::session` | `Attachment` + `OperationId`。completion の routing 鍵 | 否 |

**禁止**: frontend adapter が target identity を path string に退化させること（現状の
`op_result_applies(repo_path, switch_generation, ..)`）。

## 決定 4 — differential manifest の schema

Big Bang の受け入れは「cargo test が緑」ではなく、**前後で観測可能な状態と receipt が
一致すること**で行う（#643 §4 parity oracle）。manifest は P0 harness の fingerprint を
そのまま使う:

```
DifferentialManifest {
  fixture:   FixtureManifest              // seed / files / commits / depth / head
  before:    Fingerprint                  // benchmark::fingerprint (root / entry / kind)
  after:     Fingerprint
  receipt:   Option<OpLogEntry>           // recording.entry()
  category:  Normal | Refused | Drift | Partial | Unknown
  allowlist: Vec<PathGlob>                // ADR が許可した前後差（例: .git/index の stat）
}
```

- `Fingerprint` は SHA-256 + metadata、`.git` entry・private gitdir・commondir を role 付きで走査（`00-harness.md`）
- **同一 topology の 5 category を legacy 経路と新経路の両方で取り、allowlist 外の差を不合格**とする
- `index` の stat cache 更新（ADR-0193）は allowlist に入れる。これは状態ではなく cache

### 規範 — in-progress operation の所有（#704, 2026-09-13）

> Repository の in-progress operation は session-owned read model の観測値である。
> View / Entity / pane / modal の存在は lifecycle、action availability、admission の
> 根拠にしてはならない。UI は `Reads` の観測から action を提示し、mutation は
> その revision を凍結した request と Backend の live preflight が一致した場合にのみ
> 開始する。

read の owner は ADR-0183 の `Reads<TabViewState>` **ひとつだけ**である。freshness
guard を持たない経路（repo path しか凍結しない legacy detector など）が同じ観測を
書けると、古い結果の marshal-back が admission を取り消しうる。#704 はこれを破っていた実例で、
merge が resolve されると `ConflictView` が破棄され、abort が構造的に到達不能になった
（`MERGE_HEAD` は残ったまま）。migration README の Conflict C2 から本節を参照する。

## 決定 5 — 移行の順序と所有（#643 §4 Wave 表を採用）

| Wave | 内容 | 本 ADR での位置 |
| --- | --- | --- |
| 0 | 契約固定（本書） | **完了** |
| 1 | core reducer: fake completion で admission / settle / reconcile / OwnerStamp の全遷移 | **完了** (#693: `OwnerStamp` / `begin_write` / `RunningWrite`) |
| 2 | report boundary: 全 family が `ExecutionReport`、UI 側 append ゼロ | **UI 側は完了** (#694 #695 #696 #697、下記メモ) |
| 3 | vertical cutover: legacy 17 file を `BeginWrite` / settle へ。`busy_op` 除去 | **完了**: run family / pull / pr-merge / branch-cleanup / plan latch すべて移行済み (#698 #699 #700 #701 #702)。`busy_op` / `LegacyBusy` / `finish_op_on_main(_settled)` / `op_result_applies` を削除。task unwind は #703a の process supervisor が閉じ（`Abandoned` は supervisor 経由の `Unaccounted` / `Stopped` に置き換わり、型から削除）、Windows の probe は #703b の job object が閉じた。**Windows probe の blocking CI 化で留保を解除** (#728、下記「終端未確定の出口」を参照) |
| 4 | UI state: `TabUiState` per session | **完了** (#714 #716 #717 #718 #720 #721 #722 #724)。契約は **ADR-0197**（所有の 5 分類 / `TabStores` / entity retention / leak matrix oracle / slice 順）。`reset_per_repo_ui` は削除され、tab 切替に残るのは window-global 1 slot（plan slot と repo-scoped modal）の close だけ |
| C2 Abort | read-model-derived availability / entity-independent admission / typed report / reconcile | **完了** (#704。Wave 4 の前倒しではなく、ADR-0183 の read ownership と Wave 3 / C2 mutation lifecycle の correctness slice) |
| 5 | crate 抽出（境界安定後のみ） | |
| 6 | cleanup | |

**Wave 2 実装メモ（2026-09-12）**: legacy family は全て `*_blocking` から
`RunReport` を返し、`KagiApp::finish_recorded`（非同期; settle → `async: <op>
finished|failed` 契約行 → `present_recorded` → `on_done(Result<&OperationOutcome,
OpFailure>)`）か `KagiApp::present_report`（同期 4 サイト）で**backend の receipt
そのもの**を提示する。`src/ui` に bare `Backend::run` は無い。UI が entry を
合成するのは (a) plan blockers による `Refused`、(b) repository が開けず何も
走らなかった場合、(c) ADR-0149 の non-run ops（`record_op_persist`: fetch 失敗、
conflict 解決、terminal 起動、PR merge、worktree 操作）のみ。pull は
stash → pull → pop の複合結果 `PullBlockingResult`、discard は `DiscardReport`
で `RunReport` を運ぶ既存形のまま。`RunReport` を `FamilyEvidence::Run` に包んで
`begin_write` / `apply` に載せるのは Wave 3 の cutover で行う（`RunReport` は
`#[derive(Debug)]`、全 field Clone なので `Clone` 付与は 1 行）。

**Wave 3 実装メモ（2026-09-12）**: `Planned::Run(RunRequest)` /
`RunJob` / `Completion::Run` / `FamilyEvidence::Run(RunReport)`（`src/app/run.rs`）
が legacy の `Backend::run` 系 write 全部の family。plan はモーダル側にあるので
`approve_run`（owner attached・凍結 worktree 一致）が plan slot の `approve` の代わり、
`prepare_run` が `begin_write` で承認を 1 回消費。UI は `KagiApp::finish_run`
（admission → job → `apply` → stamp の指す tab へ提示、違えば
`op result dropped`）。完了通知は foreground modal を置換せず、durable receipt を
oplog に提示した上で `AppNotice` queue の末尾へ送る。失敗時の reads は stale にし、
記録失敗（`Recording::Failed`）時は family の成功 footer で「changed but not recorded」
を上書きしない（#501）。
載せ替え済み 20 family: checkout / cherry-pick / revert / checkout-tracking /
switch-to-latest / set-upstream / rename-branch / delete-remote-branch / push /
merge(+into) / commit / amend / create-worktree / rebase / reset-current /
force-with-lease-push / push-tag / branch-plan / delete-branch / discard。
**終端未確定の出口（全 local family 共通、2026-09-13 改訂）**: `run_git` /
`run_child` は `GitError::TerminationUnknown(kagi_git::Termination)` を型のまま返す
（`Other` へ潰さない）。`Termination` は **2 状態**で、保持する状態は必ず出口を持つ
（#703a で `Abandoned` を型から削除。旧 3 状態目の経緯は下記）:

| 状態 | lease | 出口 |
| --- | --- | --- |
| `Stopped { reason }` | 解放 | reconcile entry を read → acknowledge |
| `Unaccounted { reason, group }` | 保持 | `ReconcileJob` が process group の生存を probe し、空になっていれば `stop_proven` |

**task unwind（旧 `Abandoned`）**: spawn した child と group は
`kagi_git::proc::supervisor` が **task の外**で所有する（job 登録 → executor thread が
`enter` → `run_child` が group を登録し、空だと証明できた時点で解放）。panic しても
registry は道連れにならないので、abandonment は
`Termination::from_abandoned_job(reason, live_groups)` で終端を作る: live group が
残っていれば probe 可能な `Unaccounted`、1 つも無ければ「この job のプロセスは何も
走っていない」＝ `Stopped`（唯一の writer だった executor thread は unwind 済み）。
reconcile の stop proof は `supervisor::group_stopped` を通り、requirement が名指す
group だけでなく **同じ job の全 group** が空であることを要求する。

stop proof は **observation より先**に立てる。group が生きている間は何も読まずに
unresolved を返す（実行中の snapshot を「停止後の観測」として acknowledge させない）。
proof は direct child の reap ではなく process group の空である（`run_child` は
`process_group(0)` で spawn し、deadline では group を kill する）。remote-writing
operation は承認時に凍結した `RemoteExpectation` を live `ls-remote` と比較して
`resolved` を決める（local snapshot は remote の状態を語らない）。`acknowledge` は
`stop_proven` かつ `resolved` の read だけを受け付ける。

**Windows (#703b)**: process group の代わりに **job object** を使う。spawn 直後に
`CreateJobObject` + `AssignProcessToJobObject` で child を専用 job に入れ、子孫は
その membership を継承する。`QueryInformationJobObject` の `ActiveProcesses == 0`
が「group が空」と同じ意味を持ち、deadline では `TerminateJobObject` が
`kill_group` の役割を果たす。supervisor の所有と probe の形は unix と同一で、
`Termination::Unaccounted { group }` が運ぶ `u32`（child の pid）を **何に問うか**
だけが platform ごとに違う。job を持てなかった pid（assign 失敗、query 失敗、
job を retire 済み）は従来どおり **alive** と答える — 証拠が無いことは「消えた」では
ない。unix / Windows のどちらでもない platform も同じ安全側 fallback のまま。

**Wave 3 完了（2026-09-23, #728）**: 実装は #703a / #703b で揃い、Windows の probe は
**blocking** になった。`.github/workflows/ci.yml` の `build (Windows)` job は
`continue-on-error` を外して `blocking-ci.needs` に入っており（必須集合の唯一の定義 —
ADR-0077 決定）、Windows job が落ちれば `blocking-ci` が fail し、既存の release CI gate
（`check-release-ci`）がその SHA を拒否する。実際に走るのは
`cargo test --release -p kagi-git --lib proc::job::` = `crates/kagi-git/src/proc/job.rs` の
`#[cfg(all(test, windows))] mod tests`（6 test: suspended child の resume、job に残る
descendant、deadline kill 後の `ActiveProcesses == 0`、retry を跨ぐ proof、sweep 後の
handle、pid 再利用）。根拠は
[#728 の job 単位集計](https://github.com/TomiXRM/kagi/issues/728#issuecomment-5781578431):
latest 20 run（20 distinct SHA）の Windows job が **20/20 success**（failure / cancel / skip は
ゼロ）、各 log が `running 6 tests … 6 passed; 0 failed`。

**残る未検証**: 実ユーザーの Windows 実機および「全 family の host-close / Unknown /
settle matrix」の実機統合は今回の検証に含まれない。#728 は blocking CI の選択肢で
完了とする。

**#355 段階 0（2026-10-04）**: 上の「全 family の matrix」のうち、
background task で動く guard writer（単独 fetch、remote branch fetch、
PR ref fetch、Editor 保存）の **task unwind → Unknown receipt → reconcile →
acknowledge → 次の write** を Tier A の実経路で確認した。`WriteGuard` の Drop は
引き続き解放しない。job 外に保持した abandonment が supervisor の停止証拠と
`UnaccountedWrite` を届け、停止済みでも結果不明の write は確認前に再実行させない。
単独 fetch の結果は開始時の `session + visit` 以外へ表示せず、古い visit の
失敗 receipt は永続記録する。Busy は plan revision を消費しない。remote SSH pull
の lease 化は #989 の範囲であり、実機 Windows と全 family の matrix は引き続き
未検証である。

**#992 review（2026-10-04）**: dirty Pull の fetch waiter も要求時の
`session + visit` を保持し、離脱済み visit への確認提案を破棄する。
fetch 失敗は waiter ごとに記録せず、fetch 自身が一度だけ永続化する。
remote branch fetch の通常失敗も、表示中 tab ではなく開始時の frozen repository
へ必ず永続化する。PR ref fetch の通常失敗は既に同じ境界で記録している。
ただし旧 visit の fetch に新 visit の dirty Pull が相乗りした場合は、
新 waiter に結果を配送する。失敗なら新 visit に短い footer/toast のみ表示し、
fetch 自体の receipt は重複させない（#992 review 4175367910）。
remote branch fetch の `[kagi] fetch-remote-branch: ok` / `failed` 行は
表示先の有無に関係なく完了時に出し、footer/toast のみ current visit に限定する
（#992 review 4175420136）。PR ref fetch は既に早期 return より前に
terminal klog 行を出している。
離脱後の fetch / remote branch fetch / PR ref fetch の失敗 receipt は、永続化と
Operation Log panel への追加を同時に行い、panel の自動表示と footer/toast
のみ current visit に限定する（#992 review 4175461885）。ADR-0200 の
retention 契約（#1119）では activation は PR mode・predicate・accepted rows・
opened panes・draft を保持し、旧 visit/request の publication 権だけを失効する。
新 visit の `enter_tab` → `refresh_github_prs` による GitHub L1 refresh が
`sync_open_pr_tabs` → `reload_moved_pr_tabs` → `fetch_pr_for_open` を通して
retained pane の head を再取得する（repository read の pane revalidation ではない）。
旧 visit の完了は新 fetch の loading latch と data を触らず、もし新 visit の
fetch が走っておらず head が未読なら `fetch_pr_for_open` で一度だけ再開する
（#992 review 4175505337）。

**pull（A' 採用 = 上記 (a) の改訂結果）**: `FamilyEvidence::Pull(PullReport)`。
`PullReport { steps: Vec<RunReport>, terminal }` は実際に走った child の receipt を
実行順で運び、settle は最後の step ではなく `terminal.decisive`（pull 失敗後に restore
成功なら pull）の outcome で行う。pull が `TerminationUnknown` のときは pop を実行せず、
stash OID を recovery context に残して reconcile を 1 回登録する。#625 の runtime
refusal は core が `Refused` の no-execute step として記録し、UI は entry を合成しない。

**(a)(b)(c) 完了（2026-09-13）**: (a) pull（上記メモ）。(b) pr-merge / branch-cleanup は
`write_lease`（`reserve_write`）／run family へ。(c) merge-plan / delete-branch-plan は
書き込みではなく planning の UI latch なので `planning` フラグへ分離。その結果:

- gate は `KagiApp::op_latched()` 一本 = `has_leases() || planning.is_some()`（加えて
  repository session を持たない clone の実行中も拒否する）。
  - **`has_leases()`** — remote pull を含む全 write の真実。
  - **`planning`** — 書き込まないが modal slot を占める plan task。
  - **`write_busy_op` は gate が読まない**。lease の *presentation mirror*
    （snackbar のラベル用）であり、`refresh_write_busy` が lease 消滅で retire する。
- `LegacyBusy` を全 admission signature から削除。`begin_write` / `write_lease` /
  `prepare_*` は lease と reconcile だけを根拠に `AdmissionError::Busy` を返す。
  planning 中の write 拒否は UI 側の `op_latched()` が担う（`reserve_write` /
  `reserve_stage_write` / `dispatch_job` / `finish_run` / `finish_pull`）。
- legacy completion helper `finish_op_on_main` / `finish_op_on_main_settled` と
  `op_result_applies` / `classify_op_result`（`repo_path + switch_generation` 比較）を削除。
  planning は `KagiApp::finish_planning`（launch 時に凍結した session+visit の owner stamp、
  自分の tag のときだけ `planning` を解放）へ。
- `KagiApp::busy_op` フィールドを削除。契約行 `[kagi] op panicked: {} — busy_op cleared`
  は**文言不変**（意味は「latch を解放した」）。

### Modal slot arbitration（#718、2026-09-14）

`active_modal` が一つであることだけでは、非同期 completion が後から user input を
上書きできる。遷移を modal 型ではなく発生源と鮮度で分ける:

1. 同期 user intent だけが `replace_modal_from_user` で slot を置換できる。
   押しのけた `AppNotice` は queue 先頭へ戻す。
2. async plan は `offer_plan_from_async` で空 slot にだけ confirmation を出す。
   競合時は生 plan を queue せず、`PlanToken` / one-shot approval を失効し、
   planning latch を解放して fresh plan の再実行案内を queue する。
3. 実行済み write の terminal outcome は oplog receipt を真実とし、現在の文脈内
   footer を基本に一か所で提示する（#1698、下記）。記録済み結果を閉じるだけの `AppNotice` は作らない。
   `Unknown` / `Partial` の acknowledge / inspect capability がある場合だけ、その操作を
   `AppNotice` queue 末尾へ送り、foreground modal は置換しない。
4. async in-place update は expected variant と modal generation / request identity が
   current の場合だけ payload を更新する。新規 modal を作る能力は持たない。

`finish_run` の family callback は `KagiApp` を受け取らず typed `RunPresentation` を
返す。`finish_planning` も typed `PlanningPresentation` を返し、owner/visit、vacancy、
失効、retry notice を中央で処理する。raw `active_modal` mutation は
`operations/modal_state*` の storage helper に限定し、`modal-slot-storage` gate は
その module boundary を補助する。


**remote pull（#989）**: cached ahead/behind から preview を合成し、background plan job
で通常の SSH options による `ssh -G` と common-dir probe を行う。cached
branch / upstream / HEAD OID / dirty と host の live 値が違えば、
確認を開かず `RemotePreviewStale` で再読み込みを促す。
同じ plan-time SSH probe は `GIT_OPTIONAL_LOCKS=0` で index の任意書込を止め、
staged index (`git ls-files -s -z`) と worktree status
(`git status --porcelain=v2 -z --untracked-files=all`) の binary stream を取得し、
SHA-256 fingerprint を凍結する。#1014 では実行時の `ssh -G` はローカルで経路だけ照合し、
承認後の remote 接続は **1 回**にする。その 1 本の remote POSIX `sh` 内で common-dir /
物理 worktree / branch / HEAD / upstream / URL / fetch refspec / staged index /
worktree status を凍結値と比較し、すべて一致した場合だけ
`exec git -c branch.<name>.mergeOptions= -C <physical-worktree> pull
--no-rebase --ff --no-autostash --no-recurse-submodules` に進む。
host の rebase / ff-only / branch merge options / autostash / submodule.recurse は
確認内容を上書きしない。fast-forward 可能なら行い、分岐時のみ通常の merge にする。
検査不能・不一致は pull 前に理由つき `Refused` を記録し、lease を解放する。
system `ssh` と remote POSIX shell は Windows クライアントでも同じで、
Kagi 管理の ControlMaster/socket は作らない。認証拒否は既知の Failed、
切断や実行到達が不明な場合は Unknown / Partial と lease / reconcile を維持し、
検査抜きの別 SSH 接続へ fallback しない。別接続の間隙はなくすが、
host 上の他プロセスによる最終照合直後の変更まで原子的に禁止するものではない。
`Planned::RemotePull` は `begin_write` で `WriteScope::Remote(RemoteRepoId)` を取り、
`OperationId` / `OwnerStamp` によって配送される。Success / Failed は通常解放し、
Unknown / Partial / job abandonment は停止済みの reconcile requirement と lease を保持する。
remote の結果を観測できないため、確認後の明示的な unobservable-release audit によってのみ解放する。
**Limitation:** ssh-agent-only 接続では connection identity の identity files が空で、
identity-file 接続より reconcile の再現可能性が弱い。remote stash の凍結接続 policy は変更しない。

**SubAgent 規律**: family / module / report は単独 owner。shared schema・router・ADR・
migration summary は integration owner 専有。子 agent は evidence packet（revision、
command、fixture manifest、raw artifact、result、limitation、affected API）を返す。

## 変更しない境界（#643 §3 より再掲）

- domain は pure
- backend は write 実行時に trust / preflight を再評価する
- safety pipeline の省略不可。trust / preflight / verify を off にする flag を足さない
- receipt は global tail から読まない
- `Unknown` は failure の別名ではない
- `plan` が worktree / index / object を変更しない family では、それを正しさとして検証する

## 帰結

- Wave 1 以降の PR は本 ADR の型名・状態表・identity 表を引用する。**逸脱は本 ADR の改訂を伴う**
- `FamilyEvidence` に variant を足さない family は移行できない（Wave 2 の完了条件）
- `LegacyBusy` と `busy_op` の削除が Wave 3 の完了条件 — **2026-09-13 達成**（#703 の Windows stop proof は #728 の blocking CI 化で完了）
- 本 ADR の時点で**コードは 1 行も変えていない**。固定したのは型・表・順序だけである

## Amendment — 2026-10-13: actionable notice dismissal (#1366)

Escape or a dismissal/Close route acknowledges the **presentation**, not the
underlying reconcile requirement. Remove the active AppNotice from the immediate
presentation queue instead of putting it back for the next render. Dropping its
payload also drops `release_armed` and the old reconciliation read; it must never
release a lease, acknowledge the requirement or record an unobservable release.
The existing #1292 alert-focus lifecycle restores the valid opener (or the root
if the opener is no longer drawn).

A fresh explicit mutation admission refusal is a recurring condition and may
offer Inspect again through `blocking_reconcile`. Planless writers (Fetch,
Editor Save and the other `reserve_write_for` callers) share the existing
capability-aware refusal presenter rather than enqueueing a plain dead-end
message. Ordinary renders and reads do not recreate dismissed notices. The
Operation Log retains the original durable detail; this fix does not add an
OpLog Inspect action or a new mapping between receipts and in-session operation
IDs. Reinspection uses the existing refusal route and obtains a fresh read
before either release step.
An explicit enqueue into a session already waiting on reconciliation is also a
recurring write request, even when the queue never retries admission. Its
`Enqueued` effect offers the blocking Inspect notice unless the same requirement
already has an active or queued Inspect/Acknowledge notice. Queue ticks and
renders do not present it again after Escape; the original queue wait and write
blockade remain unchanged.

Return keeps the existing non-destructive primary action (Inspect, ordinary
Acknowledge or the first arm step). Handled modal Return is consumed before
default text input delivery, so it cannot insert a newline into the retained
Input that opened the notice. Root Return on the armed destructive release
has no default action and retains the armed presentation. Escape or Cmd-. dismisses
it without releasing the write blockade. Unread notices already queued may present next; a notice displaced
by another user-opened dialog is still unread and remains queued under #718.


## Amendment — 2026-10-15: Fetch authentication failures (#1561)

Fetch retains a typed authentication failure with the attempted remote name and
complete CLI diagnostic. Its Failed outcome is a single-line localized EN/JA
summary identifying the remote and asking the user to check credentials. The
optional receipt `technical_detail` holds the unchanged full CLI diagnostic,
rendered in expanded detail and Operation Log Copy, never in the toast or row
summary. The wire codec omits this field when absent, so ordinary/old receipt
bytes remain unchanged; old lines read it as `None`. A non-string optional
detail is ignored as `None` without losing the receipt. Manual, quiet, departed
and remote-branch menu Fetch attempts retain the same detail and exactly one receipt.
The existing bounded toast/footer presenter and frozen session/visit ownership
remain the only delivery path. Other failure and Unknown wording is unchanged.

Non-interactive Git sets `GIT_ASKPASS` and `SSH_ASKPASS` to empty strings, retains
`core.askPass=` hardening and `GIT_TERMINAL_PROMPT=0`, and launches no synthetic
helper. Global credential helpers remain available; this does not add credential
storage, automatic authentication retry or a success fallback. A loopback HTTP
Basic 401 fixture covers the backend and real native Fetch presentation in both
languages and Apple themes, with unchanged refs and full diagnostic Copy.

## Amendment — 2026-10-21: truthful stopped / unrecorded outcomes (#1699)

Entering Conflict Mode is not completion of merge or rebase. Typed conflict
outcomes produce an informational stopped notice and neutral footer, with the
observed unresolved-file count. Merge supplies its conflict paths; rebase reads
the actual index conflicts inside its existing background completion, before
the write lease settles. If that observation fails, the notice explicitly leaves
the count unknown rather than inventing zero. Existing receipts and klog lines
continue to describe the unchanged execution contract.

Recording failure is independent of operation outcome: notices distinguish
success, failure, refusal, partial application and uncertainty in EN/JA. An
unchanged failed operation is never called completed or impossible to undo.
Attempted Unknown/Partial evidence is retained in the in-memory log even when
the durable append fails; the recording diagnostic is supplementary, not a
replacement operation result.
Success and partial-application notices also explain that the unrecorded operation
cannot be undone or restored from its Operation Log record immediately, not only
that the history will be absent after restart. The in-memory entry has no trusted
ref-move evidence for those recovery actions. Failure/refusal notices do not claim
that an unchanged operation needs undo.
When conflict-stopped execution and recording failure coincide, the required
recording acknowledgment is the only primary result surface. Include the observed
stopped context in that acknowledgment instead of adding a stopped Info notice,
ordinary result toast or result footer. Merge/rebase starts and a typed
`ContinueOutcome::Staged` must not claim whole-operation completion. Borrow that
presentation context only for the original successful outcome; never downgrade
Unknown or replace the original receipt. The in-memory row remains non-durable;
presentation never retries append or manufactures a persisted receipt.

Current strict-FF Pull uses the existing typed `NoOp(PullUpToDate)` presentation
after its successful freshness fetch. Equal or locally ahead tips show the same
up-to-date notice as ordinary Pull. Exact upstream identity is retained and
structural blockers take precedence, so a no-op never masks conflict/worktree
refusal or fabricates a write receipt.

## Amendment — 2026-10-21: one request, one result (#1698)

- 既存の receipt presenter / session owner / `ToastStack` が表示を所有する。
  通常の現在の Graph 操作は footer のみ。window が非 active、別画面、別 tab、
  または開始から 2 秒以上の run / Pull / Fetch は toast のみで通知し、完了後に Busy footer を残さない。
  background owner の結果は現在の tab の footer / panes を変更しない。
  明示的な SSH Pull / Stash は既存の長時間・network 操作の通知 lane で一枚の toast を使う。
  通常の remote snapshot が所有する Busy / read-only footer と分離し、結果 toast の通常の
  表示寿命を refresh で短縮しない。別 visit への帰還も離席として扱い、foreign footer の変更や refresh はしない。
  footer から toast へ移しても成功対象 / 有用な成功詳細を失わない。操作固有の Success 文は
  実際に append 済みの Success receipt にだけ利用し、Unknown / Partial / 記録失敗の提示を上書きしない。
  Clone の Open Repository は、その呼出しが実際に作った / 置換した toast id にだけ結び付ける。
  append failure や toast を作らなかった完了から、以前の別操作の Details を書き換えない。
  local / remote Stash は既存 session に加えて要求 visit を照合する。
- Pull の fetch と一時 stash push / pop は内部手順であり、独立の成功 toast を出さない。
  backend が記録した全 child receipt と順序 / id はそのまま既存 Operation Log に挿入し、
  workflow の要求名 **Pull** と最終結果だけを一度提示する。記録失敗した child を成功で隠さない。
  freshness fetch を共有した場合も各要求の開始時刻を引き継ぎ、最終 no-op / write の提示で
  経過時間を判断する。decisive receipt が Unknown なら workflow の Partial より優先する。
  hunk Stage / Unstage 成功も file staging と同じく静かにし、失敗時だけ原因を提示する。
- 同じ repository + 要求操作 + outcome の toast は既存 card を置換する。
  件数、close focus、詳細 action を増やさず、repository は操作名の後に短い名前だけを示す。
  local preflight が no-op と判定した Pull / Push は write receipt を捏造しない。
  実際に実行した SSH request が no-change を返した場合は、backend の元 receipt を一件だけ保つ。
  receipt と表示 owner の repository locator に末尾 separator の差があっても同じ key とし、
  実行済み Pull から no-op Pull に続けて要求した場合も同じ成功 card を置換する。
  成功 Pull の primary は要求名を含む短い翻訳済み headline とし、未知の stdout 最終行を再掲しない。
  構造化された成功 target は保ち、内部 no-change と既知 Git CLI の no-change 表記は同じ意味で示す。
  元 stdout は receipt に保持し、この成功用 formatter で Failed / Partial / Unknown を再分類しない。
- Failed / Refused は EN/JA の短い原因と次の行動を表示する。Partial / Unknown は意味を保ち、
  Unknown は再試行禁止を明示する。raw stderr / exit code / 絶対 path は durable receipt の
  詳細に保持し、未知の診断は安全な翻訳済み要約にする。toast の操作ログ button と常設
  footer の操作ログ button から詳細へ進める。失敗だけでは panel を自動展開しない。
  詳細 action は receipt の確定 id に紐づき、Home / 別 tab から操作 owner を表示して展開する。
  owner tab が閉じられた場合も worktree の再 open / remote 再接続を要求せず、同じ既存 log panel を
  Home で表示できる。到達できると確認する前に toast / action を消さない。
  manual Fetch の途中で Home / 別 tab へ移動した場合も、一度記録した同じ failure receipt の
  結果 / 詳細 action を離席通知として届ける。quiet auto-fetch は単独通知を作らず、Pull の待機が
  あれば最終要求名 Pull として一度だけ提示する。
  Home / 別 repository / remote 表示では当該 local owner が実行画面にいないため、復元 / revert は
  翻訳済みの理由付きで無効化する。inspection / copy は維持し、操作 event の受信時にも owner を
  再確認してから既存の backend plan / preflight へ渡す。並列の所有 state は持たない。
  `index.lock` という path だけを lock 競合の証拠にせず、既存 lock の診断が無ければ安全な要約を使う。
  既存 `[kagi] bottom-panel: open (Failed auto-open)` は harness 互換の契約行として残るが、
  表示状態を変更する指示ではない。
- Conflict Save / directory-file resolution / Abort、worktree maintenance、History も同じ既存
  result owner を使う。通常の既知 Refused は短い原因と詳細導線で提示し、dismiss-only
  AppNotice を追加しない。既存 receipt、typed blocker、History の非永続化条件は変更しない。
  background の typed Continue 停止は保存済み receipt への Details 付き Info とし、別 owner の
  footer / panel を触らない。明示的な Details click 後は既存の owner-follow navigation に従う。
  Skip の commit 単位の成功と、Merge / Rebase 開始・Rebase Continue の要求全体の未完了を混同しない。
- 実行済み sequencer Continue の terminal completion は、現在の session / visit と frozen plan の
  `Arc` identity が一致する消費済み confirm だけを閉じる。KnownFailed / append failure /
  Unknown でも古い confirm から再実行を誘わず、既存の reconcile Inspect / admission guard と
  notice の自然配送を保つ。他 owner / 後発の modal や編集 draft は閉じない。
- 通常の Busy admission 拒否は既存 snackbar と近傍の Idle 理由だけとし、Error toast /
  Failed footer / dismiss-only AppNotice は作らない。reconcile が実際に必要なら Inspect を保つ。
  Quit / host-close guard は同一理由を active / queued 合わせて一枚に集約し、write / clone 完了後
  `may_close_host` が true になれば自然に取り除く。Quit / close 自体を自動再開しない。
- oplog append failure は別の「記録を保存できなかった」事実であり、既存の明示 acknowledge /
  inspect notice を唯一の primary とする。共通 recorder の前後に通常 result toast / footer /
  dismiss-only notice を追加しない。元の失敗診断・Unknown evidence・記録失敗の診断を失わず、
  既存 Failed / Refused / Unknown を append failure だけで別の outcome に変更しない。
  無言化、再 append、成功への置き換えはしない。既存の必須 reconcile inspection は維持する。
- Tier A の所有: `toast_stack_budget`（実 no-op Push 連打 / keyed 置換）、`stage_failure_notice` /
  `hunk_staging`、`pull_presents_backend_receipts` / departed-owner Pull、host dirty guard / clone close。
  `worktree_admin_result_owner` / `history_persists`、Continue refusal / stopped / backend-failure、
  `conflict_family_result_owner`、generic Merge failure と既存 Unknown panic / dismissed-confirm の
  consumer は、repo/request identity・実 backend state・canonical Details / 必須 ack を検証する。
  Clone / Stash visit 帰還と remote Pull の current / inactive / away / revisit consumer は、
  実 receipt と details identity、foreign footer 保護、必須 recording notice を検査する。
  remote 結果の保持は refresh interception だけでなく、所有 SSH adapter 経由の実 Git と
  通常の `remote_snapshot` 完走でも検証する。handler 境界と通常 OS 入力の証拠は区別する。
  Busy のこの実装は #1700 の統合依存であり、同 issue で第二の通知 owner は追加しない。
## Amendment — 2026-10-21: Pull terminal observation and retained refusal (#1720)

Pull's three successful-execution terminal paths read only the existing
`Backend::head_state().display()` for the `verified: pull after = ...` diagnostic.
They do not collect a repository snapshot, which recursively reads unrelated
linked worktrees and unnecessarily retains the finished write's lease. The FF
and merge execution paths already return their typed result only after safe
checkout and the branch-ref transaction commit. A receipt's predicted
`Success.after` is not an observed HEAD and is not substituted for this read.

This final read is display-only. Failure logs that the actual HEAD was
unavailable, without a fabricated `verified` prediction; it does not alter the
already determined result, receipts or settlement classification. Other callers
of the shared after-snapshot helper retain their existing contract.

A fetched strict-FF refusal is a blocked explanation, not an execution-ready
approval: its intentionally absent executable Pull identity cannot pass
preflight. Retention freezes the exact configured remote, full upstream ref,
local/upstream tips, current HEAD, dirty digest and owning tab visit. A matching
snapshot also requires current backend identity/state to match. A refusal is
replanned only to refresh a still-blocked explanation; an executable/no-op
replacement invalidates the old request rather than starting it. Ready choices
retain their original executable identity and preflight requirements. Slash
remote names, local upstreams and display shorthand never replace canonical
identity. Ordinary merge-capable Pull and explicit strict-FF remain different
policies; this change does not rewrite Git configuration or their admission.


## Amendment — 2026-10-21: dismiss progress, not the request (#1719)

- The existing Busy snackbar owns its own accessible EN/JA close Button. Closing
  it changes presentation only: no cancellation, Skip probe, lease, approval,
  durable receipt, final result, recording-failure notice or Quit guard changes.
  Reuse the existing toast-control focus adapter and removal event, including
  Settings; repair focus when the control disappears or work finishes.
- Hidden read state belongs to the existing per-tab tracked read. Hide every
  read already represented by the card, including simultaneous unfinished reads,
  not just the oldest slow one. Phase changes do not reset dismissal. A read
  admitted afterward and another tab's own read remain independently visible.
- Global work uses its existing admitted operation identity. A local Pull's
  `(session, visit, request_started)` follows the freshness waiter and confirmation
  into its distinct execution lease, including non-current branch fast-forward.
  Explicit conflict Review retains that same origin through the existing merge
  planner, merge confirmation and admitted merge execution. A new independent
  Merge still starts with its own planning request; cancellation retires it.
  Remote planning retains its admitted request identity into approved execution;
  clone retains its current card's start identity. No operation-name matching,
  notification controller or parallel delivery store is introduced.
- Retire suppression with the actual owner. A new admission cannot inherit a
  completed same-clock request's hidden state; joining the same live waiter does
  not resurrect it. An unrelated Fetch must not erase a still-owned Pull's
  dismissal. With no current-visit waiter, global progress keeps a dismissed
  identity from that same flight rather than reverting to an older undismissed
  waiter. A new current-visit request takes precedence; another owner's new read
  stays visible. The existing independent toast entity stays mounted for later
  results, without retaining the closed Busy card's paint or pointer target.
- Closing progress never dismisses terminal feedback. Failed/Unknown/Partial
  and oplog append failure keep the one-request/result and inspect/ack contracts
  above. Natural completion retires the work; hiding is not completion evidence.


## Repository-scoped conflict write admission (#1746)

The process-local Sessions lease is not cross-process exclusion. Conflict Save,
directory/file resolution, Continue (including merge staging), Abort, Skip and
merge finalization additionally use the backend's repository advisory lock at
`<common-gitdir>/kagi-write.lock`; linked worktrees share that repository scope.
On Unix this is an fd-based `flock`, not `O_EXCL` or a PID marker. Never unlink
the persistent sidecar. Closing the final descriptor (including process death)
releases ownership in the kernel.
Both draft and repository admission use `kagi_git::advisory_lock::lock_exclusive`;
the caller supplies the distinct path and timeout, not a second flock implementation.

Read-only conflict plans probe the same lock with nonblocking attempts, retrying
for at most 100 ms. Generic application admission uses one zero-timeout probe,
never a lock sleep on the UI thread. The confirmation modal retains no lock.
The mutation owner reacquires before final preflight and retains the descriptor
through execution and verification; the approved full revision is checked again
under that lock. Reads do not acquire it. Nested executor calls on the same
thread reuse the descriptor; unrelated background writers do not share admission.

Every local writer's Sessions admission also probes that repository lock, so
stage, pull, commit and background fetch cannot start while another process is
inside a conflict mutation. These generic probes never append an admission
receipt themselves. User-facing dispatch owns a single `Refused` receipt under
the requested operation's name (Pull, Commit, checkout, manual Fetch, snapshot,
Editor save, etc.); a Pull refused before its preparatory Fetch is named `pull`,
not `fetch`. Continue, Skip and staging retain their existing receipt owners.
No `conflict-write-admission` receipt is synthesized. A waiting queued intent
records its initial refused attempt once; probes and automatic re-contention
do not append repeated receipts. Recording failure retains the existing required
acknowledgment instead of presenting an ordinary refusal as durably recorded.
Continue and Skip planning likewise return their blocker to their existing UI
receipt owner. Legacy Abort/merge planning records its early refusal under the
observed `<kind>-abort` or `merge-commit` name and carries the original attempted
`Recording` through planning errors. Its UI owner presents that recording once;
a failed append activates the required acknowledgment, never a second row or an
ordinary refusal toast. Admission receipt states use the translated `unobserved`
grammar; every admission-error variant has a bounded EN/JA summary while Details
and Copy retain the full stored diagnostic.

Silent auto-fetch and periodic PR-ref refresh skip contention without a toast or
receipt and retry on their next ordinary tick. A manual PR opening remains a
user attempt named `fetch-pr`. The PR fetch intent survives pending and stale
retries; a quiet admission refusal releases loading so an unchanged next list
tick can try again. A queued head waits with the same
localized repository-busy explanation and one error toast per intent. Its retained,
cancellable retry task probes with timeout zero on the background executor after
250 ms, then 500 ms, 1 s and at most 2 s between probes. Contended probes neither
replan nor renew approval nor emit another toast. Successful availability returns
that specific head to Queued for a fresh plan. Before emitting availability the
task removes and detaches its own handle, so reentrant admission that loses to
another process can retain a new retry for the same head. A non-blocker probe
error also returns the head to normal planning/admission, which reports the actual
failure rather than waiting indefinitely as contention. Closing its owner or
removing the head drops the task; late results cannot revive it. Contention never
claims an identity change or cancels successors.

Draft preservation is a preceding phase: capture live input and complete the
background writer's `flush()` barrier (or blocking backend `autosave()`) and
release `buffer.json.lock` before acquiring `kagi-write.lock`. `flush()` drains
queued captures, not uncaptured live input. Never hold both locks, in either order.
After preservation, reacquire repository admission and repeat preflight; a changed
approval refuses without mutation. This is advisory coordination among Kagi
instances, not exclusion against arbitrary external Git/worktree writers.
The integrated #1731/#1760 preservation policy is fail-closed: a failed capture or
durable draft flush refuses Abort, Skip and directory/file replacement before
repository admission. The formal `preserve-abort`, `preserve-skip` and
`preserve-dir-file` guarantees now remain HOLD alongside repository-wide admission.
Directory/file recovery snapshots are separate from this draft barrier: their ref
write follows repository acquisition and preflight while the lock remains held,
before destructive index/worktree changes.

Contention is `Refused`, never `Failed` or `Unknown`: the durable English blocker
remains "Another Kagi window is operating on this repository". Display-only
refusal previews use short, punctuated EN/JA cause and retry-later sentences that
fit the bounded toast beside its action; Unknown/Partial previews omit retry advice.
Successful generic/stage admission clears only an Idle/Failed footer containing
that repository-busy cause (including a localized staging preview), retaining
unrelated statuses. Operation Log and bounded-toast delivery remain unchanged;
no new modal or retry queue is added.

