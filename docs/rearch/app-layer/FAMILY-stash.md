# #484 family 2: stash push / apply / pop / drop

状態: **PR 1 local 4 op は #541、GUI follow-up は #550 で merge 済み・PR 2 remote は r4 設計案**。
実装境界と残る競合窓は [ADR-0176](../../adr/0176-app-stash-local-boundary.md)。
以下は採用済み設計 r3 と、PR 2 の未解決点を具体化した **r4** の契約。
PM Round 1 の (1)〜(5)/A〜E、
omp-plan 第 2 レビュー R1〜R4 と PM Round 2 の全件採用裁定を反映。
追加 D と C の完了範囲は §7 の調整案を PM 採用済み。
r4 の読解基準は `origin/dev@89327ce7`。2026-09-07。
コード変更・cargo・G/E/M 実行は本 docs PR に含めない。
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
    Error { error: PlanError, recording: Option<Recording> }, Approved,
    // None は採用済み error の記録待ち。未採用 completion は error evidence のみ。
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

fresh Backend が開く前の実行失敗と未実行 abandon は Backend 側 factory が entry 材料を
**同じ finalize** に渡すだけとし、独自 append はしない。run 開始後の catch/Drop/配送失敗が
factory 記録へ戻る経路を作らない。実行・verify の unwind を捕捉した後に一回 finalize し、
app/GUI は返った receipt を運ぶだけ。finalize 済み・append 失敗済みの試行は配送失敗でも
再記録しない。同じ OperationId の completion 再送も append しない。

**plan error の採用順序（R1）**: plan job は未記録の error evidence を completion に返す。
Sessions が revision を照合して error を採用した時点で、当該 revision を記録受理済みにし、
一度だけ Backend の共通記録処理へ渡す。未採用/superseded completion は append しない。
現行 remove の `PlanJob::run` 内先行記録→`apply_plan` の revision 照合という順序は、
この stash 契約に流用しない。採用済み error の重複配送から記録処理を再発行しない。
記録は host が所有する owned job として完了まで運び、採用後に次 revision へ進んでも
受理済みの一回の記録を取り消さない。通知だけは現在 revision/owner の表示 guard に従う。
これにより記録の完了と modal の寿命を分離し、UI が直接 append する必要もない。
blocker 付き確認の Refused は Backend boundary に渡し、一件記録・mutation なし。
Busy/StaleApproval は未受理の admission 結果で、実行 receipt を捏造しない。

`Recording::Failed { attempted, error }` は mutation 結果と独立。変更成功でも記録失敗を
error modal/toast に出し、再実行を促さない。record-only retry と mutation retry を混同しない。
stash は oplog 免除ではない。採番 id/parent と app OperationId は別、multi-process locking、
crash durability、裸 OID の GC 後保持を今回の receipt で保証したとは主張しない。

### 3.2 対象固定・verify・partial

現行 `run` の apply/pop/drop preflight は HEAD、stash count と worktree digest の照合。
**同じ件数で index の内容が置き換わる場合には不十分**なので、承認済み full OID と
ordered stash list fingerprint を stash plan に保持し、実行直前に再照合する。
fingerprint は **順序と重複を保持した full OID 列**から作り、count/選択位置/選択 OID を
同じ列に束縛する。set 化・short OID 化はしない（R2）。
この判定は **`kagi-git` の `preflight_check_stash` の拡張**に置く。既存の HEAD/count/
worktree digest 照合は残し、full OID/fingerprint を追加条件として要求する。app は凍結 plan を渡すだけで、
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
| Drop | executor の返却 OID と承認 OIDに加え、前後の ordered list（重複の個数を含む）、他 stash の残存、WT/index 非変更を確認。事前読取 OID の返却だけを実削除対象の証明にしない。観測不整合なら Verified にせず recovery/evidence に残す |

**pop の二段目（R2）**: apply → conflict 判定 → drop のうち、drop の直前にも同じ
Backend の stash identity 照合を行う。承認 OID/list と不一致なら drop を行わず、別 entry を
消さない。apply で WT/index は既に変更済みなので、未変更 Refused に戻さず、stash 保持と
観測済み apply 結果を Partial（証拠不足なら Unknown）として finalize する。
二段目は apply 前の worktree digest と一致することを要求しない。初段の既存 digest gate は
維持し、二段目は stash identity と apply 後の観測を検証する。
最後の照合と libgit2 の index 指定 drop の間には外部 Git との競合窓が残る。
前後 list の検証も全 ABA を証明できるものではなく、原子的削除や排他の保証とは呼ばない。

現状 `pop_outcome_for` は UI で Partial にするが、backend の mapper は一般の Ok を Success に
するため、その UI 分岐だけ残して「receipt は正しい」としない。stash mapper に同じ意味を
移し、JSONL/receipt/footer が一致するようにする。apply の conflict も Unit を成功と
決めつけず観測を report に含める。既存 conflict detection/continue/abort を維持する。

verify 失敗は予測 after を Verified とせず、観測済み副作用は Partial、結果不明は Unknown。
executor 内 unwind は記録より内側で捕捉し、副作用直前から外側に残す stash progress
（対象 OID/実行段階/観測値）から分類する。未変更が証明できる失敗だけ Failed。
auto-snapshot が生じた場合も開始証跡/復元材料を落とさない。panic 後の再 append は禁止。

## 4. UI 移行表と追加処理

| PR 1 実装前の入口（4ad4ad28） | app API | 撤去する glue / 残す表示 |
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
既存 conflict の観測を別 Drop の conflict evidence に流用せず、payload の生成・置換は
当該 Apply/Pop が新たに生成した stash-kept conflict に限定する。
continue 後の opt-in drop prompt は reload の modal clear 後に開く。
PR 1 は旧 `pending_stash_drop = Some(0)` を撤去した。完了 report の
**canonical worktree + 元 operation id + full OID** を、
Sessions 内の owner 別 in-memory conflict 継続 payload に **表示 guard より前に保存**する
（R3）。modal/active view だけに置かず、inactive owner の completion でも保持する。
common-dir が同じ別 linked worktree には流用しない。#485 follow-up（#557）は tab close 時に
その owner の active/follow-up payload を clear し、close→reopen へ持ち越さない。

continue 成功時は payload を一度だけ follow-up 待ちへ移し、その owner の reload 後に
**新しい drop plan と新しい承認**を要求する。旧 pop/apply の承認は再利用しない。
follow-up の full OID 候補が **一件だけ**のときに同定し、0 件/複数件なら prompt を出さず
pending payload を破棄する。同 OID の別 reflog entry を最初の一致で選ばない。
この小さい移行で occurrence-level identity の新機構は作らない。

abort 成功、対象 conflict の終了（continue 成功による上記移管を除く）・置換、
prompt 取消/完了で対応 payload を clear/consume する。continue/abort 失敗では、
同じ conflict が残ると確認できたときだけ保持する。別 owner の操作では消費しない。
外部 Git 起因・再起動後など出所不明の conflict は解決自体を許すが自動 drop prompt は
出さない。index 0、別 owner、重複 OID の先頭への fallback は不可。
conflict executor 全体は移さず、この小 payload の所属/寿命だけを接続する。
payload は in-memory で tab close / 再起動を跨いで永続化しない。

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
app は typed transport capability を呼ぶだけ。remote pull や一般 SSH backend trait の導入は
しない。receipt 型は GPUI 非依存の共通 backend 契約を利用し、host が actor を固定する。

omp-plan の未解決点に対する r4 の裁定は次のとおり。

| # | 決定 | 理由 |
|---|---|---|
| 1 接続 identity | `ssh -G` で security-relevant effective config を凍結し、PR 2 の write は direct SSH profile に限定する | alias clone では向き先変更を防げず、Proxy/ControlMaster の実経路を agentless MVP で再現・停止証明できないため |
| 2 remote key | 凍結接続 + remote physical common-dir を `RemoteRepoId` とし、main/linked root を同一 key にする | 選択 root を key にすると同じ ref/index writer を別 root 経由で同時実行できるため |
| 3 stop-unknown | common-dir 内の operation marker と valid `done` を停止証明にし、同 scope の read→ack だけで解除する | local ssh child の終了・再接続だけでは remote worker の停止も mutation 有無も証明しないため |
| 4 verify | 一個の remote worker が preflight/drop/post-read を実施し、versioned fixed frame を outcome matrix で分類する | 複数 SSH job の間で drift 窓を増やさず、stdout の OID と実際の前後 state を同じ証拠へ束縛するため |
| 5 実 SSH M | direct Linux host の main/linked fixture、alias drift、実 timeout→Unknown→marker reconcile まで必須とする | fake は argv/異常応答を決定的に網羅できるが、OpenSSH の process/切断挙動と host 上の path identity は証明できないため |

### 5.1 SSH 接続 identity の凍結

plan はユーザーが選んだ alias に `ssh -G -- <alias>` 相当の read probe を一度実行し、
次の **security-relevant effective values** を typed `RemoteConnectionId` に正規化する:
`hostname`、`user`、`port`、`hostkeyalias`、`identityfile` / `certificatefile` の順序付き列、
`userknownhostsfile` / `globalknownhostsfile` の順序付き列と内容 digest、`hostkeyalgorithms`。
元 alias とこの構造体の fingerprint を plan evidence に保持する。実行直前にも alias を
`ssh -G` で再解決し、fingerprint が違えば marker 作成前の Refused / replan とする。

PR 2 の write transport は **direct SSH だけ**を受け入れる。effective config の
`proxyjump` / `proxycommand` が `none` 以外、`controlmaster` が `no` 以外、
`controlpath` が `none` 以外、または connection/known-host path に未展開 token が残る場合は
「安全に固定不能」として plan error にする。`CanonicalizeHostname` による解決後の hostname は
記録できるが、実行で canonicalization を再適用しない。ControlPersist socket の再利用、
ProxyJump/ProxyCommand の多段経路を「同じ host」と推測しない。

実行 argv は可変 alias ではなく凍結値から組み立て、`-F /dev/null`、
`BatchMode=yes`、`StrictHostKeyChecking=yes`、`CanonicalizeHostname=no`、
`ControlMaster=no`、`ControlPath=none`、`ProxyCommand=none`、`IdentitiesOnly=yes`、
`IdentityAgent=none`、`PubkeyAuthentication=yes`、`PasswordAuthentication=no`、
`KbdInteractiveAuthentication=no`、`GSSAPIAuthentication=no`、
`HostbasedAuthentication=no`、`VerifyHostKeyDNS=no`、`UpdateHostKeys=no` と literal host/user/port、
凍結した identity/certificate/known-host/host-key algorithm を明示する。
`HostKeyAlias` が有効ならそれも明示する。これにより再照合直後の config edit が実行先を
変えない。鍵ファイルの内容まではコピーせず、変更で認証不能なら非対話で失敗する。ssh-agent
のみを使う接続は read のままにし、PR 2 の write では明示 identity file を要求する。
known_hosts は plan 時に内容 digest を取り、mode 0600 の operation-owned snapshot を作って
実行 argv の User/GlobalKnownHostsFile に明示する。通常 key と `@cert-authority` の判定は
その snapshot を使う OpenSSH に委ね、Kagi は鍵を取得・追加・自動承認しない。原本の変更は
再照合で Refused、snapshot の変更/消失は実行不能。direct profile を満たさない既存 read 接続は
引き続き読めるが、Drop は不可。

### 5.2 remote resource key と common-dir

local Backend を開けないので `WriteScope::{Local(RepoId), Remote(RemoteRepoId)}` の有限和を
使う。`RemoteRepoId` は §5.1 の `RemoteConnectionId` と **remote host 上の physical
common-dir** の組。Target/Attachment はユーザーが選択した root を別に保持し、表示対象と
lease/reconcile key の等値性を混ぜない（R4）。別 lease table は作らず、既存の全体直列化も
維持する。

common-dir probe は凍結接続で remote の `sh -c` 一回に root を positional argument として
渡し、shell 文字列へ連結しない。`cd -P -- "$root"`、
`git rev-parse --is-inside-work-tree`、
`git rev-parse --path-format=absolute --git-common-dir`、その結果への `cd -P` / `pwd -P` を
同じ probe で行い、versioned frame で返す。PR 2 はこの `--path-format` を持つ remote Git
2.31 以上と POSIX `sh` を対応範囲とし、bare repo、改行/NUL を含む root、symlink loop、
権限不足、古い Git、malformed frame は fail closed。文字列上の `..` 除去だけや local 側の
path canonicalize で代用しない。

例として選択 root `/srv/repo` と `/srv/w1`（後者の `.git` が
`/srv/repo/.git/worktrees/w1` を参照）は、どちらも physical `/srv/repo/.git` を
common-dir として返すため、同じ接続では同じ
`RemoteRepoId` になる。別 hostname/user/port/host-key identity なら path が同じでも別 key。
plan と worker 内 preflight の両方で root→common-dir を解決し、凍結値と違えば mutation 前に
Refused。取得不能時に root 別 key へ縮退せず、別 linked root から Busy / NeedsReconcile を
迂回できない。`host:root` は表示 label に限り local canonical path として扱わない。

### 5.3 operation marker と停止不明の回復

承認・lease 取得後、transport は app の `OperationId` に束縛した推測困難な 128-bit
`RemoteJobId` を生成し、`<common-dir>/kagi/remote-ops/v1/<remote-id>/` を mode 0700 で作る。
既存 path、symlink、owner/権限が
期待と違う場合は Git mutation 前に拒否する。bootstrap は stdin/stdout/stderr を marker 内の
file へ切り替えた detached worker を一度だけ起動し、remote 側の受理 id を返す。worker は
SSH channel の切断後も同じ id で完了を記録できる。凍結 connection と common-dir を使い、
`started` を atomic rename で確定してから preflight→drop→post-observation を一回だけ行い、
終了状態を `done.tmp` から `done` へ rename する。marker を作れない remote では PR 2 の
Drop を許可しない。自動再送はしない。

SSH の local spawn 前失敗と remote が job を未受理と証明する応答は Failed。remote の
`started` 後は、正常な `done` を読めるまで writer の停止を証明しない。SSH timeout、EOF、
`wait_with_output` error、local child reap、単なる再接続、`started` だけの marker は Unknown
で lease を保持する。現在の `RemoteError::Spawn` 名から phase を推測せず、typed transport
phase（LocalSpawn / Submitted / Started / Done）を report に残す。

read-reconcile は同じ凍結 connection と `RemoteRepoId` で `done` を読むだけで mutation を
再実行しない。valid `done` の operation id、remote job id、scope、protocol version、result digest が元 job と
一致して初めて「remote writer は終了済み」とする。その後に現在 state を read し、receipt と
照合した recovery summary を表示する。ユーザーの ack token は operation id、scope、done
digest、観測 revision に束縛し、exact lease だけを解除する。marker 不在/途中/malformed、
別接続・別 common-dir、観測失敗では ack を発行しない。worker 自体が marker 完了前に失われた
場合はその app session 内で安全に解除できず、手動調査が必要になることを既知制約として表示する。
terminal receipt の apply または read→ack 後だけ marker を best-effort cleanup する。cleanup
失敗を stash mutation の失敗や再実行理由にせず、残存 marker は operation id で監査できる。

### 5.4 一回の worker と verify 判定

plan は remote HEAD、ordered full stash OID 列（順序・重複を保持）、選択 index と OID、
common-dir、index fingerprint、worktree-state fingerprint を危険確認へ凍結する。
index は `git rev-parse --git-path index` が示す bytes（不存在 sentinel を含む）、worktree は
`LC_ALL=C git status --porcelain=v2 -z --untracked-files=all` の bytes を Git の hash-object で
fingerprint 化する。これは ignored file と同一 fingerprint に戻る ABA を証明せず、外部 Git
との排他も保証しない。

remote worker 一個が common-dir 再確認、HEAD/list/index/worktree の preflight、
`git stash drop "stash@{index}"`、直後の同じ観測を順に行う。check と drop を別 SSH job に
分けない。stdout は worker が捕捉し、C locale の実出力から得た full OID を plan OID と照合する。
応答/marker は magic/version と固定個数の NUL 区切り field（phase、exit、selected/stdout OID、
before/after HEAD/list/index/worktree fingerprint、error class）だけを受理する。値は enum・整数・
full hex OID/fingerprint に限定し、欠落、余分、short OID、不正 hex/UTF-8、version 違いを
成功として解釈しない。生 stderr は長さ制限した診断 evidence であり判定 field ではない。

| worker / 観測 | outcome と lease |
|---|---|
| marker 作成前、または `started` が無く未受理を証明 | Failed。mutation 無し、lease 解放 |
| preflight mismatch、drop 未実行、valid `done` | Refused。再plan、lease 解放 |
| exit 0、stdout OID=承認 OID、after list=before から選択 occurrence 一件だけ除去、HEAD/index/worktree 不変 | Success / Verified。receipt 一件、lease 解放 |
| non-zero、before と同じ post state を valid `done` が証明 | Failed。mutation 無し、lease 解放 |
| drop/参照変化を観測したが stdout/list/HEAD/index/worktree の verify が不一致または不足 | Partial（既知副作用を保持）。停止済みなら lease 解放、recovery を表示 |
| `started` 後に valid `done` が無い、または mutation 有無を分類できない | Unknown。lease 保持、§5.3 の read→ack のみ |

同じ OID が複数 occurrence に存在しても、before の index と ordered list によって削除位置を
束縛し、after はその位置一件だけを除いた列と比較する。最後の preflight と Git の ref update
間の競合窓、同一 bytes へ戻る ABA は残る。remote safe.directory/権限、timeout、argv quoting
は既存 transport より弱めない。

### 5.5 UI と保持範囲

pure な effective-config/frame parser、`RemoteConnectionId` / `RemoteRepoId` / response 型は
`kagi-domain::remote`、`ssh` spawn・known_hosts snapshot・remote marker I/O は新しい
`src/remote/stash.rs` に置く。`src/app/stash.rs` は有限な `StashJob::Remote` が持つ typed job の
run/report だけを調停し、`std::process` / `std::fs` / shell 文字列へ触れない。fake は remote
adapter の有限 test transport/fault enum とし、production が任意 callback を受ける port には
しない。UI から `crate::remote::remote_stash_drop` を直接呼ぶ現行分岐は撤去する。

RemoteDrop も §2.2 の plan→approve→prepare→run→apply を通り、UI は predicted summary で
receipt を作らない。active owner の footer/modal/refresh、inactive/closed owner の対象付き
通知、具体的 refusal/error、EN/JA、危険確認を local と同じ mapper で扱う。
Unknown は操作 id/scope と「再送しない」「remote operation の終了確認待ち」を表示し、
同じ key の linked root を含む write を止める。success/failed の toast だけで完了としない。

## 6. 検証設計（実装 PR で実行）

G は tempdir を canonicalize し、専用 log dir を使う。公開 API 列から plan/承認する。
UI state を直接 Ready に書き換える seam は不可。完了前/配送前の停止点は手動 job runner
または有限 fault で制御し、sleep と子プロセスの長時間待ちを成否条件にしない。

| G | 必須 assertion |
|---|---|
| 4 op / stash 3 件 | 真ん中の index を対象に実 bytes/index/ordered full OIDs を確認。push の message/untracked、apply の保持、pop の消費、drop の WT 不変 |
| index drift | plan 後 push で件数変化、drop+push で同件数置換、対象 OID 消失。別 stash を変更せず一件 Refused。fresh trust 拒否も mutation 無し |
| pop 二段目 | apply→drop 間に ordered list を置換。変更済み bytes/index を維持、Kagi の drop は走らず対象/他 stash を追加削除しない。receipt は Partial/Unknown で Success/未変更 Refused ではない |
| conflict | pop の index conflict・stash kept、receipt/JSONL が Partial。continue は commit 無し、正しい OID の drop prompt、abort は stash 保持。apply conflict も確認 |
| receipt | drop の実 full OID から内容復元、auto-snapshot 無し。別 repo/同 op の append を応答前に挟んでも元 entry を返す |
| revision | 入力更新→replan error→旧 completion 後着、cancel/別 modal、policy 変更、double confirm。旧 token は使えず、新入力だけ実行 |
| plan error 採用 | 旧 error が新 plan 成功より遅着しても旧 error の JSONL はゼロ。採用 error の重複配送は一件。採用後 revision 更新でも記録は完了し、古い通知は新 modal を変更しない |
| run 互換 / 一意性 | 旧 run と sibling を別々の fixture/試行で呼び、各一件。open failure/abandon/unwind/append failure/配送失敗でも同じ試行を再記録しない。同じ mutation の二重実行を比較試験にしない |
| conflict payload | 深い stash で A conflict→B 切替→A 復帰→continue は A の一意 OID だけを新規確認。別 linked worktree/出所なし/候補 0・複数では提案なし。abort/終了/置換/取消/tab close で owner payload を clear |
| tab/lifetime | A で job.run、B 切替/owner close/Welcome 後に apply。A の記録一件、B modal/footer 不変、stale/対象付き通知。重複 completion と未実行 job Drop |
| Busy 両方向 | stash と remove/editor save/staging/snapshot/fetch を両順序で予約。bytes 不変、再入拒否、正しい owner だけ解放。停止不明では保持 |
| failure | open failure、preflight 拒否、mutation 前/内部 panic、verify failure、abandon。Partial/Unknown の evidence を JSONL 再parseで確認 |
| append failure | log file の位置を directory にして決定的に失敗。変更済み+attempted entry+error、過去 tail 無し、二重実行なし |
| remote direct config | typed fake で `ssh -G` の全採用 field と凍結 argv。ProxyJump/ProxyCommand/ControlMaster/agent-only/未展開 token、config drift は marker/drop 無し。通常 key と CA known-host path、malformed config を網羅 |
| remote identity | main/linked root の physical common-dir は同じ key、別接続は別 key。同 key の実行中は Busy、Unknown の ack 前は NeedsReconcile。古い Git、common-dir/symlink解決不能、worker 再解決 drift は drop 無し |
| remote worker | ordered OID の中間 occurrence、stdout OID、HEAD/list/index/worktree の fixed frame を照合。欠落/余分/version違い/short OID、preflight drift、non-zero unchanged、known mutation+verify failure を §5.4 の全 outcome へ写す |
| remote recovery | local spawn 前 Failed、Submitted/Started 後 timeout、done marker 不在/途中/不一致、valid done の read→ack、replayed/wrong-scope ack。Unknown は同 key lease を保持し、自動再送・二重記録しない |

fault は正常値 None、有限 enum の doc-hidden test API（integration test から到達可）だけ。
任意 callback で gate を迂回できる形にしない。#531 の強化後の uv fault gate を適用する。

E は実 KagiApp/modal/focus から 4 op の **raw Enter と実ボタン** の両方を駆動し、
receipt 一件と fixture の変更を確認する。pop conflict は非緑通知、Conflict Mode の表示、
continue 後の opt-in modal と取消時の stash 保持まで確認。replan error は双方実行不可。
深い stash の conflict で A→B→A と切り替えて continue し、A の正しい OID のみを提案する。
出所なし/重複 OID では別 entry の prompt が出ないことも確認する（R3）。
ボタン bounds は現在の window を計測し、計測 canvas を button の上に被せない。
scenario ごとに window を remove、保持 Entity/input clone を drop、guard を復元する。
PASS 行だけでなく **runner exit 0・leak detector 通過**を必要条件にする。
PR 2 は typed fake transport を使う `remote_stash_drop` の filtered scenario を追加し、
remote tab の modal Enter/実ボタン、危険警告、Busy/Refused/Unknown の EN/JA 表示、refresh、
一件 receipt、別 tab 非汚染を確認する。E は adapter 配線の証明であり、実 OpenSSH/common-dir/
切断挙動の証明には使わない。runner は必ず `KAGI_GUI_E2E_ONLY=<remote scenario>` で一件ずつ
実行し、filter 無しの full run は禁止する。

既存 `tests/stash_conflict_test.rs`、`tests/stash_pop_test.rs`、
`tests/oplog_nonrun_ops_test.rs` と E `scenario_stash_drop_persists` の assertions を弱めない。
run wrapper の非 stash 互換も既存 backend run/partial tests で検証する。
GUI の conflict continue 後の follow-up 提示は #550 で修正され、filtered stash E で検証済み。
window なしの G は owner/full OID payload から一意な新規 Drop plan が Ready になるまでを保証する。
実装時の専用 target/実行分担は PM 指定に従う。本 docs PR では cargo は一切実行しない。

M は PM: direct SSH の Linux host（Git 2.31 以上。基準 fixture は Git 2.43）に main + linked
worktree と stash 3 件を作る。中間 index の成功、取消、実行前 stash drift、alias の HostName
変更による Refused、main で保持した lease が linked root を Busy にすること、EN/JA の
Success/Refused/Unknown/NeedsReconcile と remote refresh を実機確認し画像・marker を残す。
送信後切断は専用 host の `git` shim を stash drop の時だけ実 Git の前で sentinel/release file
待ちにし、他の probe は実 Git へ透過する。worker の `started` を別 control session で確認後、
Kagi transport の whole-command timeout を発生させる。Unknown・自動再送無し・同 key Busy を
確認してから release し、remote worker の valid `done`、read→ack、正しい一回の Drop/receipt、
lease 解放まで確認する。成功/失敗 toast だけ、fake だけ、local child reap だけでは M 完了としない。
M の host-key 経路は既知の通常 key 一組を必須とする。`@cert-authority`、malformed known_hosts、
Proxy/ControlMaster、agent-only の拒否は typed fake G の責務で、PM 環境への CA 構築は要求しない。
local の大きい stash の応答性、Enter 連打、危険確認、conflict 解決→取消/Drop、owner 切替/close、
アプリ内 close/Quit 保留も既存 M の回帰項目として残す。
1b の実 remove 対 editor save の M は PM 報告で通過済みの別の前提 gate であり、
この E をその代用とはしない。

## 7. 規模・分割とレビュー事項

`src/ui/operations/stash.rs` は r4 基準 tree で **460 行**（`wc -l`、コメント/空行込み）。
PR 1 で local glue は移管済みで、残る remote branch は 416〜455 行付近。
PR 2 後も open/replan/render intent と表示 mapper に絞り、現状規模を大きく増やさない。
以下の PR 2 数値は実測 after ではなく計画用概算。800 LOC/file・80 LOC/function を超える場合は
backend の stash boundary、app の stash job、UI adapter、tests の機能境界で分ける。

| PR | 内容 | 概算 |
|---|---|---|
| 1 local | enum/共通 report/receipt sibling + local 4 op + conflict follow-up payload + G/E + ADR | production 450〜750 行追加、旧 glue 350〜500 行削除、tests 450〜700 行追加、約 12〜18 files |
| 2 remote | typed scope/direct SSH freeze/recorded worker+reconcile + remote drop UI adapter + G/E/M | production 400〜700 行追加、旧 glue 60〜100 行削除、tests 400〜700 行追加、約 10〜16 files |

**PR 1 に含めないもの**: remote drop の plan/executor/記録/表示経路の移管・変更、
remote 用のファイル移動、`Local/Remote` lease key 和の導入。remote の既存経路は残し、
`LegacyBusy` と共通 lease の両方向排他のみ維持する。remote scope は PR 2 で追加する。

**PR 1 の完了条件（追加 C/D の整合案・PM 採用済み）**:
`src/ui/operations/stash.rs` の **local 4 op 経路**から `finish_op_on_main` / `record_op` /
同期実行 confirm / blocking core 呼出しをゼロにする。`rg -n` の全ヒットと各 caller の
所属を PR 本文に載せ、remote 分岐の残存だけを明示する。単に関数を別ファイルへ移動して
「ゼロ」としない。実行しない薄い confirm intent の名前と、同期 executor 本体も区別する。
現行 remote 分岐（r4 基準 tree の stash.rs:416/419/422/442）には
remote_stash_drop / finish_op_on_main / record_op 呼出しが残るため、
「remote を変更しない」と「PR 1 でファイル全体のヒットゼロ」は両立しない。
**PR 2 の完了条件**は、remote も移管した後に同ファイル全体で上記旧 glue のヒットゼロを
同じ方法で示すこと。この D のスコープ調整は
[PM の r2 裁定](https://github.com/TomiXRM/kagi/pull/532#issuecomment-5560797477)で採用された。
他 family の helper は削除しない。実装開始時は最新 dev を取り込み、PR 1/#550/#557 の
stash conflict と tab lifecycle を巻き戻さない。

PM Round 1 で (1) finite enum/単一 slot、(2) run 互換 sibling/唯一の finalize、
(4) owner+full OID と 2 PR 分割、(5) remote 停止不明の保留は採用となった。
(3) は r1 の log profile 案を撤回し、Enter とボタンのログ統一を採用した。
追加 A〜E は本文に反映、D は上記スコープで PM 採用済み。
omp-plan R1（採用後の記録）、R2（pop 二段目）、R3（conflict payload の寿命）、
R4（remote 資源 identity）は PM Round 2 で全件採用、r3 の §3〜§6 に反映した。
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
