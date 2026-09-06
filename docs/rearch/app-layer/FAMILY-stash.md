# #484 family 2: stash push / apply / pop / drop

状態: **設計レビュー r2・未実装**。PM Round 1 の (1)〜(5)/A〜E を反映。
追加 D と C の完了範囲の整合は §7 の提案について PM 確認待ち。omp-plan 第 2 レビュー待ち。
読解基準は `origin/dev` の `4ad4ad28`
（#530 merge）。2026-09-07。コード変更・cargo・G/E/M 実行は本 PR に含めない。
PM 報告では 1a/1b の G/E は通過、M は PM 待ち。**1b の M 通過と PM の横展開許可、
および [#531](https://github.com/TomiXRM/kagi/issues/531) のマージ後の `dev` への
rebase を実装開始条件とする。それまでは実装しない。**
[DESIGN](DESIGN.md) §5.2/§7/§8 の次 family を具体化するノートであり、
DESIGN 冒頭の古い実装状況（1b 未実装）を現在の状況として引用しない。

## 1. 範囲と採用案

- local の 4 operation と既存 remote/SSH drop を移す。新しい stash 機能は追加しない。
- `KagiApp` 内の同じ `Sessions`、承認 revision、lease、dispatch/apply を拡張する。
  worker、新 crate、controller、registry、TabId/incarnation は作らない。
- **有限 family enum + 単一 active plan** を選ぶ。個別の remove/stash job は残す。
- `Backend::run` の戻り値は維持し、同じ実装に委譲する `run_recorded` を追加する。
  verify と記録を一つの mutation boundary に収め、実 append receipt を返す。
- **2 PR** を推奨: local 4 op の縦断移行、remote drop の縦断移行。
  skeleton だけの先行 PR にはしない。両方の完了まで stash family 移行済みと呼ばない。

CLI/MCP tail 読みの撤去 #505、他 family の #492/#493/#510 全件、conflict executor
全体の移管、remote pull、session/read ownership #482/#488/#489 は対象外。
既存の Git trust、危険操作確認、auto-snapshot 方針、EN/JA 表示を緩めない。

## 2. 最小 skeleton 差分

### 2.1 型と所有

以下は型の関係を示すスケッチ（実装・確定シグネチャではない）。

```rust,ignore
enum Planned { Remove(RemovePrepared), Stash(StashPrepared) }
// Prepared は plan + request/owner + policy の整合した組。別々の enum を組み合わせない。
enum PlanState {
    Draft, Planning { revision: RequestId },
    Ready { token: PlanToken, prepared: Planned },
    Error { error: PlanError, recording: Recording }, Approved,
}
struct Approved { revision: RequestId, prepared: Planned } // private fields
enum Job { Remove(RemoveJob), Stash(StashJob) }
struct Completion { id: OperationId, report: ExecutionReport }
struct ExecutionReport {
    target: Target, result: MutationResult, verification: Verification,
    recovery: Vec<RecoveryHandle>, recording: Recording,
    evidence: FamilyEvidence, termination: Termination,
}
enum FamilyEvidence { Remove(RemoveEvidence), Stash(StashEvidence) }
// Delivery::Completed { id, attachment, report: Box<ExecutionReport> }
```

| 比較 | 判断 |
|---|---|
| `PlanState<P>` / `Approved<P>` / generic Job | 単独 family では型安全だが、異種 family を保持する Sessions/配送で結局 enum が必要。trait/controller の導入を伴う一般化はしない |
| 有限 enum | **採用**。remove/stash の exhaustive match だけ。private な approval、family ごとの payload と executor は維持。任意の `Operation` を後から差し替えられる API にしない |
| family ごとの plan slot | 不採用。非表示 modal の旧 Ready/承認を残しやすく、現在の ActiveModal 一つと一致しない |
| 単一 plan slot | **採用**。modal の開く/閉じる/置換、入力/policy 更新で revision を進める。in-flight operation は別 map なので modal が閉じても存続する |

`PlanToken` は revision と family の整合を承認時・prepare 時に確認し、一度だけ消費する。
Busy を返した後の再試行も state を勝手に Ready に戻さず、新しい公開 plan/承認列を通す。
遅着 plan は revision が違えば state を変更せず、同じ revision でも UI は現在の
modal 所属を確認してから描画する。非 stash modal を後着応答で置換しない。

変更は `PlanState` だけでは終わらない。現在 remove 固定の `InFlight.plan`、
abandoned channel、`Sessions::apply`、reconcile の `(RemovePlan, stopped)`、
`ReconcileJob` も有限 enum にする。owner/operation id/lease の処理は共通化し、
family 差分は invalidation 対象と read-reconcile capability に限定する。
remove の admin fingerprint、progress、target_exists、RemovedTarget、停止不明 evidence を
共通 report 化で捨てない。`Recording` は backend の共通小モジュールへ移し、remove 側は
再 export で互換を保つ。domain に I/O、app に GPUI/settings 読込を入れない。

### 2.2 window なしの公開 API 列

```rust,ignore
let job = plan_stash(&mut sessions, request, policy);
apply_plan(&mut sessions, job.run());
let token = sessions.ready_token()?; // 公開観測。token を外部で製造しない
let approved = approve(&mut sessions, token, policy)?;
let job = prepare_stash(&mut sessions, approved, LegacyBusy(false))?;
let completion = job.run();
let deliveries = sessions.apply(completion);
```

request は Push(message/include_untracked) / Apply / Pop / Drop の有限値。
plan job が Backend で canonical worktree/common-dir と stash full OID を解決し、
index は表示用 locator として保持する。owner は既存 Attachment、actor/auto_snapshot は
host が読んだ policy に凍結する。現在の push/pop/drop の Settings 適用を保持し、
apply の直接 open も明示 policy に揃える（既定値へ暗黙依存しない）。

GUI Enter/ボタンは同じ confirm intent → approve → `KagiApp::dispatch_job`。
spawn は host だけ、G は同じ job を inline 実行する。各 stash 関数で busy 判定・spawn・
stale guard を再実装しない。共通 bridge が prepare と legacy busy mirror を同一 turn に
確立する。1b の editor/staging/snapshot/fetch と同じ lease 表を使い、当面は独立 repo も
含む既存の全体直列化を維持する。identity 解決に Git trust を戻さない。

apply は表示 guard より先に終端化/重複排除する。active owner のみ footer/modal/reload、
inactive owner は stale、closed owner は対象付き通知。結果は現在 tab から再構成しない。
未実行 job の Drop は boundary の failed report を abandoned channel に届ける。
停止済みだけ lease を解放、Unknown は read→ack 前に新 write を拒否、停止未確認は
ack でも解除しない。close/Quit 保留の保証はアプリ内入口だけ（Dock/OS Quit は保証外）。

## 3. run-path の receipt と安全境界

### 3.1 sibling を選ぶ理由と記録の一意性

| 案 | 影響 / 判断 |
|---|---|
| `run` 自体を report 戻り値へ変更 | 全 operation、CLI/MCP、既存 test の同時変更が必要。family 単位を超えるため不採用 |
| `run_recorded(op, plan)` sibling | **採用**。旧 `run` は同一内部 pipeline の receipt を捨てて result を返す互換 facade。後続 CLI/MCP も sibling を使えるが今回 caller は変えない |
| 旧 `run` を呼んでから append | 禁止。二重 writer、verify 後の別 entry、global tail 読みで receipt を偽装することになる |

現在の `record_run_oplog` は append error を捨て、`run` は trust/preflight/一部拒否と
末尾で記録する。この各出口を **唯一の記録実装である共通の一回限りの finalize** に合流させ、
`append_oplog_receipt` が返した path/採番済み entry をそのまま保持する。
非 stash arm の既存 result/outcome（#524 の partial_after を含む）は変更しない。
stash だけ既存 executor dispatch → 実 verify → outcome 組立 → finalize にする。
一般 pipeline と stash 専用 pipeline を二重実装しない。

fresh Backend が開く前の失敗、plan error、未実行 abandon の記録は remove 同様に
Backend 側 factory が担当する。run に入った後は factory が追加 append しない。
同じ OperationId の completion 再送も append しない。編集 debounce の superseded plan は
実行試行と数えず、採用する plan error の通知/記録は revision 単位で重複排除する。
blocker 付き確認の Refused は Backend boundary に渡し、一件記録・mutation なし。
Busy/StaleApproval は未受理の admission 結果で、実行 receipt を捏造しない。

`Recording::Failed { attempted, error }` は mutation 結果と独立。変更成功でも記録失敗を
error modal/toast に出し、再実行を促さない。record-only retry と mutation retry を混同しない。
stash は oplog 免除ではない。採番 id/parent と app OperationId は別、multi-process locking、
crash durability、裸 OID の GC 後保持を今回の receipt で保証したとは主張しない。

### 3.2 対象固定・verify・partial

現行 `run` の apply/pop/drop preflight は HEAD と stash count の照合。
**同じ件数で index の内容が置き換わる場合には不十分**なので、承認済み full OID と
ordered stash list fingerprint を stash plan に保持し、実行直前に再照合する。
この判定は **`kagi-git` の `preflight_check_stash` の拡張**に置く。既存の HEAD/count
照合は残し、full OID/fingerprint を追加条件として要求する。app は凍結 plan を渡すだけで、
Git の照合・安全判定を持たない。既存 caller の移行も同じ Backend 判定に集約する。
相違時に index を再解釈して別 entry を実行せず、Refused と再planにする。
push は HEAD/status、message/include_untracked と対象状態を束縛する。
Git gate は fresh-open の trust と既存 preflight を維持。Drop の auto-snapshot 免除を維持する。
preflight 後の外部 Git との原子性まで app lease で保証しない。残る check/use race は
実装レビューで明記し、外部プロセスにも排他できるという受入条件にはしない。

| operation | 記録前の実観測と outcome |
|---|---|
| Push | status と stash list、新規 full OID、include_untracked=false の残存を確認。単に count 読み失敗を 0 にして Verified としない |
| Apply | 承認 OID が残ること、status/index/conflict を確認。dirty だけで conflict 無しと断定しない |
| Pop | Applied と ConflictedStashKept を分ける。前者は対象消失と復元内容、後者は対象 OID 保持・conflict paths を記録し Partial/非緑通知 |
| Drop | 実 executor が返した full OID と承認 OID、対象消失、他 stash の残存、WT/index 非変更を確認。recovery は実 OID |

現状 `pop_outcome_for` は UI で Partial にするが、backend の mapper は一般の Ok を Success に
するため、その UI 分岐だけ残して「receipt は正しい」としない。stash mapper に同じ意味を
移し、JSONL/receipt/footer が一致するようにする。apply の conflict も Unit を成功と
決めつけず観測を report に含める。既存 conflict detection/continue/abort を維持する。

verify 失敗は予測 after を Verified とせず、観測済み副作用は Partial、結果不明は Unknown。
executor 内 unwind は記録より内側で捕捉し、副作用直前から外側に残す stash progress
（対象 OID/実行段階/観測値）から分類する。未変更が証明できる失敗だけ Failed。
auto-snapshot が生じた場合も開始証跡/復元材料を落とさない。panic 後の再 append は禁止。

## 4. UI 移行表と追加処理

| 現行実入口（4ad4ad28） | app API | 撤去する glue / 残す表示 |
|---|---|---|
| `open_stash_push_modal` / `replan_stash_push` / `confirm_stash_push`（既に async） | `plan_stash(Push)` → 共通承認/prepare/run/apply | debounce 入力 flush は維持し即 revision 無効化。独自 busy/spawn、blocking core、finish_op_on_main、record_op を撤去 |
| `open_stash_apply_modal` / `confirm_stash_apply`（plan も UI 同期 open、確認は Enter・ボタンとも sync） | modal を `Planning` で開く → 背景 `plan_stash(Apply)` → Ready/Error → 同じ承認列 | UI 同期 plan/open/run/verify、record_op を撤去。stash 保持、失敗 modal、conflict 表示を維持 |
| `open_pop_modal` / `confirm_pop`（plan は UI 同期 open、Enter sync）/ `start_pop`（button async） | modal を `Planning` で開く → 背景 `plan_stash(Pop)` → Ready/Error → 同じ承認列 | sync plan/executor 二重入口、独自 busy/finish/record_op を撤去。`pop_outcome_for` の意味を report/表示 mapper に移す |
| local `open_stash_drop_modal` / `start_stash_drop` | `plan_stash(Drop)` → 同じ列 | Refused UI writer、busy/finish/record_op を撤去。danger modal、preflight の既存ローカライズ、full OID を維持 |
| remote `open_stash_drop_modal` / `start_stash_drop` 内分岐 | `plan_stash(RemoteDrop)` → 同じ列、transport job | remote 独自 busy/finish/record_op を撤去。remote refresh と対象付き失敗表示を維持 |

依頼書の `start_stash_push` / `start_stash_apply` は現在存在しない。上表が実コード基準。
`checkout_after` は `CreateBranchWithCheckout` / `operations/branch.rs` の分岐であり、
stash apply のフィールド/consumer ではない。この family で追加も削除もしない。

modal は既存 4 variant と accessor/render routing を再利用し、Enter/cancel/button の
網羅性を確認する。blockers と error を区別し、replan error では旧 plan の Enter/ボタンを
両方禁止。runtime refusal は具体的理由を通知し、plan blockers だったかのようなログにしない。
apply/pop の `Planning` 中は確認不可で cancel は可能。plan open 失敗も同じ modal の
Error と通知に反映し、旧 Ready を復活させない。cancel/置換後の遅着 plan は §2.1 に従い破棄する。

conflicted pop/apply → reload → `ConflictOp::StashConflict` の検出を保持する。
continue は解決を stage するだけ（commit しない）、abort は stash を保持する。
continue 後の opt-in drop prompt は reload の modal clear 後に開く。
現状 `pending_stash_drop = Some(0)` は深い index の pop と一致しない既知の欠落。
この接続では完了 report の **owner + full OID** を渡し、follow-up plan 時に同定する。
OID 不在/owner 不一致では他の stash を代用しない。conflict executor 全体は移さず、
継続用の小 payload だけを所属付きにする（再起動を跨ぐ永続 conflict session は #485）。

### 4.1 klog 契約（prefix `[kagi] `、下表はその後の本文）

既存行の文言・分岐内の順序を維持する。ただし **PM (3) の明示承認により pop Enter に
async wrapper 行を追加**し、ボタンと統一する（削除・改名・並べ替えではない）。
既存の raw eprintln 契約行を移動する場合も
`klog!` 経由にするだけで文言を直さない。backend は typed event を発行し、adapter が
順に既存文字列へ写す。実行ログをすべて配送完了後へ遅延させない。

| 経路 | 順序付き契約行 |
|---|---|
| push plan | `plan: stash-push blockers={} warnings={}` |
| push 成功 | `async: stash-push started` → `executed: stash-push message={:?}` → `verified: working tree clean after stash-push` または `verify: working tree NOT clean after stash-push` → `verified: stash count={}` → `async: stash-push timing stash={:.1}s verify={:.1}s` → `async: stash-push finished` |
| apply plan | `plan: stash-apply index={} blockers={} warnings={}` |
| apply 成功 | `executed: stash-apply index={}` → `verified: working tree dirty (stash applied)` または `verify: working tree NOT dirty after stash-apply` → `verified: stash count={} (entry preserved)` または `verify: stash count={} (expected >= {})` |
| pop plan | `plan: stash-pop index={} blockers={} warnings={}` |
| pop Enter / button 成功 | `async: stash-pop started` → `executed: stash-pop index={}` → conflict の時だけ `executed: stash-pop index={} — conflicts in {} file(s), stash kept` → `async: stash-pop finished`。Enter の started/finished は今回の契約変更で追加 |
| drop local | `plan: stash-drop index={} blockers={}` → `async: stash-drop started` → `executed: stash-drop index={} oid={}` → `async: stash-drop finished` |
| drop remote | `plan: remote stash-drop index={index} blockers=0` → `async: remote stash-drop started` → `async: remote stash-drop finished` |
| blockers | `refused: stash-push plan has blockers, not executing` / `refused: stash-apply plan has blockers, not executing` / `refused: pop plan has blockers, not executing` / `refused: drop plan has blockers, not executing`。実行行なし |
| async failure | 対応する started の後、`async: stash-push failed — {}` / `async: stash-pop failed — {}` / `async: stash-drop failed — {}` / `async: remote stash-drop failed — {err_msg}`。finished と両方出さない |
| plan/open errors | `replan_stash_push: repo open error: {}`、`plan: stash-push error: {}`、`open_stash_apply_modal: no repo_path set`、`plan: stash-apply repo open error: {}`、`plan: stash-apply error: {}` |
| apply verify errors | `verify: repo open error: {}` または `verify: snapshot error: {}`。既存の到達順を保持し、report は別途 verify failure を表す |
| menu | `stash-menu: open index={}`（plan より前） |

push の status 読み失敗では現在 verify 行を省略して timing に進む。新 report で失敗を
可視化しても、成功 verify 行を追加して偽らない。reload 共通ログは既存 reload 側のまま。
pop/drop の plan error は現状 footer で、上表に架空の契約行を追加していない。

**log profile は作らない（PM (3) 採用）**。pop Enter/ボタンは同じ async dispatch と
ログ列に統一し、失敗時も started → failed、finished は出さない。同期 confirm 本体は撤去する。
apply は既存の両入口共通のログ列を維持する。
実装 PR で `rg -n 'stash-pop|stash_pop|confirm_pop|start_pop' tests src/headless.rs` 等により
stash-pop の契約を参照するテスト/harness と assertion を列挙し、間接 helper/全出力比較も
確認する。**wrapper 行の不在を assert するテストが無いことを PR 本文に証拠付きで示す**。
存在した場合は勝手に削除・更新せず PM に報告して判断を受ける。本 docs PR はその調査の
完了を主張しない。E では双方の成功/競合/失敗の順序と、一度だけ実行されることを確認する。

## 5. remote/SSH drop

現状 `src/remote/mod.rs::remote_stash_drop` は **既に transport 内で append** している。
UI に writer がある前提で再実装しない。`remote_stash_drop_recorded` sibling に同じ
run_checked/記録処理を集約し、旧関数は result の互換 facade にする。
drop の実 stdout（full OID を含む）を receipt に保持し、UI の predicted summary で置換しない。

**この節の transport 変更と Local/Remote 和の導入は PR 2 のみ**。
local Backend を開けないので Target/lease key は `Local(RepoId)` と
`Remote(RemoteScope)` の有限和にする。RemoteScope は凍結した SSH 接続指定と remote root、
取得できる remote common-dir を持つ。`host:root` 表示文字列を local canonical path と
誤認しない。SSH alias が同一 host を指す完全な同定は保証せず、現行の全体直列化を維持する。
Attachment/Invalidate も remote scope を運べるようにする。別の lease table は作らない。

plan で remote HEAD/list/full OID を取得して危険確認に束縛し、実行直前に比較する。
remote safe.directory/権限/BatchMode/timeout/argv quoting を既存 transport に強制させる。
check と drop の間に外部 Git が競合し得る制約は local と同様に明記する。
drop 後は remote list を再読し、対象消失・他 entry 残存を verify、stdout の OID と照合する。
transport 失敗をすべて「未変更 Failed」にしない。送信前失敗は Failed、送信後の切断/timeout
で remote 終了不明なら Unknown、既知副作用後の verify failure は Partial/Unknown。
ローカル SSH child を reap しただけでは remote writer 停止証明にならないため lease を保持する。
remote の停止証拠を得られない限り read→ack による解除も不可。自動再送は禁止。

app は typed transport capability を呼ぶだけ。remote pull や一般 SSH backend trait の導入は
しない。receipt 型は GPUI 非依存の共通 backend 契約を利用し、host が actor を固定する。

## 6. 検証設計（実装 PR で実行）

G は tempdir を canonicalize し、専用 log dir を使う。公開 API 列から plan/承認する。
UI state を直接 Ready に書き換える seam は不可。完了前/配送前の停止点は手動 job runner
または有限 fault で制御し、sleep と子プロセスの長時間待ちを成否条件にしない。

| G | 必須 assertion |
|---|---|
| 4 op / stash 3 件 | 真ん中の index を対象に実 bytes/index/ordered full OIDs を確認。push の message/untracked、apply の保持、pop の消費、drop の WT 不変 |
| index drift | plan 後 push で件数変化、drop+push で同件数置換、対象 OID 消失。別 stash を変更せず一件 Refused。fresh trust 拒否も mutation 無し |
| conflict | pop の index conflict・stash kept、receipt/JSONL が Partial。continue は commit 無し、正しい OID の drop prompt、abort は stash 保持。apply conflict も確認 |
| receipt | drop の実 full OID から内容復元、auto-snapshot 無し。別 repo/同 op の append を応答前に挟んでも元 entry を返す |
| revision | 入力更新→replan error→旧 completion 後着、cancel/別 modal、policy 変更、double confirm。旧 token は使えず、新入力だけ実行 |
| tab/lifetime | A で job.run、B 切替/owner close/Welcome 後に apply。A の記録一件、B modal/footer 不変、stale/対象付き通知。重複 completion と未実行 job Drop |
| Busy 両方向 | stash と remove/editor save/staging/snapshot/fetch を両順序で予約。bytes 不変、再入拒否、正しい owner だけ解放。停止不明では保持 |
| failure | open failure、preflight 拒否、mutation 前/内部 panic、verify failure、abandon。Partial/Unknown の evidence を JSONL 再parseで確認 |
| append failure | log file の位置を directory にして決定的に失敗。変更済み+attempted entry+error、過去 tail 無し、二重実行なし |
| remote | typed fake transport で argv/凍結 scope/OID/actor、preflight drift、実 stdout、verify failure、切断/timeout、append failure。一件記録と停止不明 lease。実 SSH の証明は M |

fault は正常値 None、有限 enum の doc-hidden test API（integration test から到達可）だけ。
任意 callback で gate を迂回できる形にしない。#531 の強化後の uv fault gate を適用する。

E は実 KagiApp/modal/focus から 4 op の **raw Enter と実ボタン** の両方を駆動し、
receipt 一件と fixture の変更を確認する。pop conflict は非緑通知、Conflict Mode の表示、
continue 後の opt-in modal と取消時の stash 保持まで確認。replan error は双方実行不可。
ボタン bounds は現在の window を計測し、計測 canvas を button の上に被せない。
scenario ごとに window を remove、保持 Entity/input clone を drop、guard を復元する。
PASS 行だけでなく **runner exit 0・leak detector 通過**を必要条件にする。

既存 `tests/stash_conflict_test.rs`、`tests/stash_pop_test.rs`、
`tests/oplog_nonrun_ops_test.rs` と E `scenario_stash_drop_persists` の assertions を弱めない。
run wrapper の非 stash 互換も既存 backend run/partial tests で検証する。
実装時の専用 target/実行分担は PM 指定に従う。本 docs PR では cargo は一切実行しない。

M は PM: 大きい stash の応答性、Enter 連打、危険確認、EN/JA、conflict 解決→取消/Drop、
owner 切替/close、アプリ内 close/Quit 保留、remote SSH の成功/失敗通知を実機確認し画像を残す。
1b の実 remove 対 editor save の M は別の前提 gate であり、この E で代用しない。

## 7. 規模・分割とレビュー事項

`src/ui/operations/stash.rs` は基準 tree で **807 行**（`wc -l`、コメント/空行込み）。
実装後は open/replan/render intent と表示 mapper に絞り **250〜400 行程度**を目安にする。
以下は実測 after ではなく計画用概算。800 LOC/file・80 LOC/function を超える場合は
backend の stash boundary、app の stash job、UI adapter、tests の機能境界で分ける。

| PR | 内容 | 概算 |
|---|---|---|
| 1 local | enum/共通 report/receipt sibling + local 4 op + conflict follow-up payload + G/E + ADR | production 450〜750 行追加、旧 glue 350〜500 行削除、tests 450〜700 行追加、約 12〜18 files |
| 2 remote | typed scope/recorded transport + remote drop UI adapter + G/E/M | production 180〜320 行追加、旧 glue 60〜100 行削除、tests 200〜350 行追加、約 6〜10 files |

**PR 1 に含めないもの**: remote drop の plan/executor/記録/表示経路の移管・変更、
remote 用のファイル移動、`Local/Remote` lease key 和の導入。remote の既存経路は残し、
`LegacyBusy` と共通 lease の両方向排他のみ維持する。remote scope は PR 2 で追加する。

**PR 1 の完了条件（追加 C/D の整合案・PM 確認待ち）**:
`src/ui/operations/stash.rs` の **local 4 op 経路**から `finish_op_on_main` / `record_op` /
同期実行 confirm / blocking core 呼出しをゼロにする。`rg -n` の全ヒットと各 caller の
所属を PR 本文に載せ、remote 分岐の残存だけを明示する。単に関数を別ファイルへ移動して
「ゼロ」としない。実行しない薄い confirm intent の名前と、同期 executor 本体も区別する。
現行 remote 分岐（基準 tree の stash.rs:503/506/526）には finish/record 呼出しが残るため、
「remote を変更しない」と「PR 1 でファイル全体のヒットゼロ」は両立しない。
**PR 2 の完了条件**は、remote も移管した後に同ファイル全体で上記旧 glue のヒットゼロを
同じ方法で示すこと。これは D のスコープ調整提案であり、PM 承認済みとは扱わない。
他 family の helper は削除しない。実装開始は 1b の M 通過・横展開許可・#531 merge 後 dev
への rebase 後だけとし、共有 skeleton 変更で #531 を巻き戻さない。

PM Round 1 で (1) finite enum/単一 slot、(2) run 互換 sibling/唯一の finalize、
(4) owner+full OID と 2 PR 分割、(5) remote 停止不明の保留は採用となった。
(3) は r1 の log profile 案を撤回し、Enter とボタンのログ統一を採用した。
追加 A/B/C/E は本文に反映、D は上記スコープの PM 確認待ち。omp-plan 第 2 レビューは未受領。
実装 ADR で実際の採用範囲だけを記録し、ADR-0149/0175 と remote ADR-0097 の該当部分を
相互参照する。

読解根拠: [ADR-0087](../../adr/0087-stash-sidebar-actions-and-drop.md)、
[ADR-0097](../../adr/0097-remote-stash-drop.md)、
[ADR-0148](../../adr/0148-stash-conflict-resolution.md)、
[ADR-0149](../../adr/0149-oplog-in-backend-run-and-schema.md)、
[ADR-0175](../../adr/0175-app-remove-boundary.md)、
`src/app/{session,worktree}.rs`、`src/ui/operations/stash.rs`、`src/ui/blocking_ops.rs`、
`src/ui/{mod,reload,modal_renderers_stash}.rs`、`src/ui/operations/{branch,conflict}.rs`、
`crates/kagi-git/src/backend{.rs,/recording.rs}`、`crates/kagi-git/src/ops/stash.rs`、
`src/remote/mod.rs`。Issue #493/#510 と #531 の現在の本文も確認した。
