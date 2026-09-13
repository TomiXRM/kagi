# ADR-0196: operation lifecycle を唯一化する — Wave 0 契約の固定

- Status: Accepted (Wave 0 契約; Wave 1–3 実装済み、Wave 3 の残件は #703 のみ — 決定 5 の表を参照)
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
| 3 | vertical cutover: legacy 17 file を `BeginWrite` / settle へ。`busy_op` 除去 | **完了（#703 を除く）**: run family / pull / pr-merge / branch-cleanup / plan latch すべて移行済み (#698 #699 #700 #701 #702 + 本 slice)。`busy_op` / `LegacyBusy` / `finish_op_on_main(_settled)` / `op_result_applies` を削除。残るのは下記「終端未確定の出口」の #703（abandoned executor の supervisor）のみで、これが閉じるまで受入条件（全 family の host-close / unknown / settle matrix 統一）は満たさない |
| 4 | UI state: `TabUiState` per session | 契約は **ADR-0197**（所有の 5 分類 / `TabStores` / entity retention / leak matrix oracle / slice 順）。実装は S1–S6 |
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
`op result dropped`）。**失敗時は reload しない**（reload の sweep が plan modal を
消す）— reads を stale にするだけ。記録失敗（`Recording::Failed`）時は family の
成功 footer で「changed but not recorded」を上書きしない（#501）。
載せ替え済み 20 family: checkout / cherry-pick / revert / checkout-tracking /
switch-to-latest / set-upstream / rename-branch / delete-remote-branch / push /
merge(+into) / commit / amend / create-worktree / rebase / reset-current /
force-with-lease-push / push-tag / branch-plan / delete-branch / discard。
**終端未確定の出口（全 local family 共通、2026-09-13 改訂）**: `run_git` /
`run_child` は `GitError::TerminationUnknown(kagi_git::Termination)` を型のまま返す
（`Other` へ潰さない）。`Termination` は 3 状態で、保持する状態は必ず出口を持つ:

| 状態 | lease | 出口 |
| --- | --- | --- |
| `Stopped { reason }` | 解放 | reconcile entry を read → acknowledge |
| `Unaccounted { reason, group }` | 保持 | `ReconcileJob` が process group の生存を probe し、空になっていれば `stop_proven` |
| `Abandoned { reason }` | 保持 | **無し（#703）**。kagi が自分の executor を見失った状態で probe する handle が無い |

stop proof は **observation より先**に立てる。group が生きている間は何も読まずに
unresolved を返す（実行中の snapshot を「停止後の観測」として acknowledge させない）。
proof は direct child の reap ではなく process group の空である（`run_child` は
`process_group(0)` で spawn し、deadline では group を kill する）。remote-writing
operation は承認時に凍結した `RemoteExpectation` を live `ls-remote` と比較して
`resolved` を決める（local snapshot は remote の状態を語らない）。`acknowledge` は
`stop_proven` かつ `resolved` の read だけを受け付ける。

**残件 (#703)**: `Abandoned`（task unwind）と probe を持たない platform（Windows）は
安全側の保持のまま — main の `write_lease` retention と同じ class。executor が
spawn した process tree を supervisor に所有させ、panic 後も stop proof を作れる
ようにするまで Wave 3 の受入条件（全 family の host-close / unknown / settle matrix
統一）は満たさない。

**pull（A' 採用 = 上記 (a) の改訂結果）**: `FamilyEvidence::Pull(PullReport)`。
`PullReport { steps: Vec<RunReport>, terminal }` は実際に走った child の receipt を
実行順で運び、settle は最後の step ではなく `terminal.decisive`（pull 失敗後に restore
成功なら pull）の outcome で行う。pull が `TerminationUnknown` のときは pop を実行せず、
stash OID を recovery context に残して reconcile を 1 回登録する。#625 の runtime
refusal は core が `Refused` の no-execute step として記録し、UI は entry を合成しない。

**(a)(b)(c) 完了（2026-09-13）**: (a) pull（上記メモ）。(b) pr-merge / branch-cleanup は
`write_lease`（`reserve_write`）／run family へ。(c) merge-plan / delete-branch-plan は
書き込みではなく planning の UI latch なので `planning` フラグへ分離。その結果:

- gate は `KagiApp::op_latched()` 一本 = `has_leases() || remote_write.is_some() ||
  planning.is_some()`。3 項の役割は固定で、混ぜてはならない:
  - **`has_leases()`** — lease を取れる全 write の真実。
  - **`remote_write`** — lease を取れない唯一の write（remote pull over SSH）の
    排他 latch。所有者はその operation だけ（下記「唯一の例外」）。
  - **`planning`** — 書き込まないが modal slot を占める plan task。
  - **`write_busy_op` は gate が読まない**。これは lease の *presentation mirror*
    （snackbar のラベル用）に過ぎず、`refresh_write_busy` が lease 消滅で retire
    する。gate に足すと lease を二度読むだけで何も足さず、lease-less writer の
    latch として流用すると refresh に消される（#708 review P1 がこれ）。
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

**唯一の例外 — lease を取れない write（remote pull over SSH）**:
`src/ui/operations/pull_push.rs`。`WriteScope::Remote(RemoteRepoId)` を作るには
`freeze_connection` + `probe_common_dir` の 2 往復が要る。remote stash family は
これを background の plan job で済ませてから `prepare_stash` で lease を取るが、
remote pull の plan は cached ahead/behind から**ローカルに合成**されるので相当する
job が無く、spawn 前の UI thread で probe するしかない = network I/O で UI を止める。
よって lease は取らず、専用 latch **`KagiApp::remote_write`** を持つ。

規約（#708 review P1、破ると排他が消える）:

- `remote_write` は **lease mirror ではない**。`refresh_write_busy` /
  `settle_write_busy` に渡してはならない。lease 数から retire すると、`render` →
  `poll_app_jobs` と全 admission preamble が呼ぶ次の refresh で消え、走行中の
  `git pull` に 2 本目の write（stage / fetch / conflict abort / 同じ remote への
  2 本目の pull）が admit されうる。
- 解放するのは remote pull 自身の terminal callback（success / failure / task
  panic）**のみ**。op 名の文字列判定はしない — field の所有者が 1 か所だから。
- `op_latched()` = `has_leases() || remote_write.is_some() || planning.is_some()`。
  `write_busy_op` は snackbar 用の lease mirror なので gate は読まない。
- 回帰テストは gui-e2e `remote_pull_latch`（停止可能な fake `ssh` を PATH に置き、
  `run_ssh` を実際に通す）。

残る差: `may_close_host()` は lease を読むので remote pull は quit を保留**しない**。
これは本 slice 以前からの状態で、write family 化（#703 と同じ後続 slice）で lease に
載せれば同時に解消する。

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
- `LegacyBusy` と `busy_op` の削除が Wave 3 の完了条件 — **2026-09-13 達成**（残るは #703）
- 本 ADR の時点で**コードは 1 行も変えていない**。固定したのは型・表・順序だけである
