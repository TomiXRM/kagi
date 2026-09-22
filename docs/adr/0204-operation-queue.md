# ADR-0204: operation queue と遅延の説明 — 並べるのは承認済み plan ではなく intent

- Status: **Draft**（設計のみ。#355 は未実装。本 ADR の時点でコードは 1 行も変えていない）
- Date: 2026-09-23
- Base: `fix/issue-355` @ `3ab32af4235887563ee36425a81b0b4253ef1dc0`（fetched `origin/main`）
- Related: [#355](https://github.com/TomiXRM/kagi/issues/355)（親 #359）、ADR-0196（lifecycle 契約）、
  ADR-0197（session-owned UI state）、ADR-0182（session identity / close は実行取消ではない）、
  ADR-0183（session-owned read）、ADR-0086（busy snackbar）、ADR-0093（active modal 1 slot）、
  ADR-0153 `## Consequences` 71 行（`gh pr checks --watch` の #355 繰延）
- 適用範囲（提案）: `src/app/session.rs`・`src/app/flow.rs`、`src/ui/busy.rs`、`src/ui/operations/*`、
  `src/ui/render.rs`。**新しい manager / service / crate / worker は作らない**（ADR-0182 の方針を継続）。
- Draft である間、ADR-0153 の繰延は**解除されていない**。本 ADR は解除の条件だけを定義する。

## 0. アーキテクチャレビュー（5 点）

1. **既存コードに属するか。** 属する。直列化の真実源は `Sessions`（`src/app/session.rs:222-241`）で、
   admission の route は既存の **2 本** — 承認を消費する `begin_write`（`src/app/flow.rs:297-327`）と、
   guard を直接取る `write_lease`（`src/app/session.rs:472-499`。`Busy` / `NeedsReconcile` /
   `reserve_lease` の判定は同じ）。queue は「まだそのどちらにも到達していない意図」を `Sessions` 内に
   `SessionId` keyed で足す層である。UI 側は、**適格 family の entry point が（idle のときも含め）
   owner 付きの typed intent を作って列に入れる**形にする。既存の `op_latched` / `reject_if_busy`
   （`src/ui/operations/mod.rs:198-217`）は bool を返すだけで intent も owner も持たないので、
   その helper を「積む」に読み替えるだけでは足りない。**適格でない family はその既存 guard の
   まま**で、本 Draft は広い source 改変を求めない。**3 本目の admission route は作らない**。
2. **境界。** app 層 = 順序・調停・chain 判定（pure、`Context` 不要）。UI 層 = 表示と confirm modal。
   実行 lifecycle（lease / reconcile / supervisor / oplog）は ADR-0196 のまま**変えない**。
3. **代替案と却下。** 楽観的 UI = preflight を飛ばす（#355 §7 却下済み）／並列 write = #283・
   ADR-0182「保守的な global busy は維持する」／`QueueService` 新設 = 直列性の真実源が 2 つになる／
   `TabUiState` 配置 = ADR-0197 決定 1 で調停は operation-owned。
4. **失敗モード。** 確認待ちの先頭が他 session を巻き込む（決定 1）、古い承認が新しい plan を通す
   （決定 4）、「取り消した」と言って process が走り続ける（決定 3）、trip 後に二度と動かない
   （決定 3 の gate reset）、queue の無限成長（決定 7）。
5. **受入 oracle。** 決定 8。Draft の本 ADR はどれも満たしていないし、満たしたとも主張しない。

## 1. 現在の baseline（実測、base `3ab32af4`）

| 事実 | 位置 |
| --- | --- |
| ユーザーに見える busy は bool 1 個（ラベル 1 本）。`busy_snackbar_label` が `write_busy_op` → `remote_write` → `planning` を or して 1 語を返す | `src/ui/busy.rs:45-50`、描画は `src/ui/render.rs:35` |
| その facade の背後は**独立した 3 つの latch**。`op_may_start(has_leases, remote_write, planning)` | `src/ui/operations/mod.rs:198-204`、`src/ui/busy.rs:37-43` |
| 3 つの意味: `has_leases` = 実 lease（write の真実）、`remote_write` = lease を取れない唯一の write（SSH pull）、`planning` = plan 中の latch | `src/ui/busy.rs:9-36` |
| `write_busy_op` は lease の**表示 mirror にすぎず**、gate は読まない | `src/ui/busy.rs:29-36`、`settle_write_busy` は `src/ui/busy.rs:60-64` |
| 弾き方: footer を `Msg::OpInProgress` にして `true` を返すだけ。再試行の予約も列も残らない | `src/ui/operations/mod.rs:209-217` |
| その入口は 10 file / 20 行（定義・doc を含む grep 実測） | `render.rs` `render_header.rs` `github.rs` `github_issues.rs` `conflict_abort.rs`、`operations/{mod,merge,conflict,conflict_skip,worktree}.rs` |
| admission は **global single-writer**。`begin_write` は scope 別ではなく `s.has_leases()` で `Busy` | `src/app/flow.rs:305-307`、方針は ADR-0182「RepoId の導入は並行 write 解禁ではない」 |
| lease 予約は scope 別ではなく **空の map であること**を要求する（`if !leases.is_empty() { return Err(Busy) }`） | `src/app/session.rs:500-509` |
| plan slot も **window に 1 つ**。`state: PlanState` / `plan_owner: Option<SessionId>` / `revision: RequestId` は `Sessions` に 1 組しかなく、session 別ではない | `src/app/session.rs:232-235`、ADR-0182「単一 plan slot に `plan_owner`」 |
| 承認は one-shot。`approved.revision != s.revision` または `PlanState::Approved` でなければ `StaleApproval` | `src/app/flow.rs:298-300` |
| 完了の配送鍵は `OwnerStamp { session, visit, operation }`（path ではない） | `src/app/session.rs:126-141` |
| `detach` は表示だけを捨て、`operations` / `leases` / `settled` / `reconcile` に触れない | ADR-0182「close は実行取消ではない」、`src/app/session.rs:338-345` |

**#355 の記述との差分（訂正ではなく現況）**: issue が挙げる `busy_op` は ADR-0196 Wave 3（#698–#702）で
削除済みで、#284（tab scope が無い）と #289（解放保証が無い）は `OwnerStamp` routing と
supervisor / reconcile が構造的に閉じた（ADR-0196 決定 5、2.4）。残っているのは **UI facade が
依然として bool 1 個である**ことで、#355 が体感として指すのはそこである。#288（UI thread の
commit walk）は対象外で、queue 化の前提条件にもしない。

## 決定 1 — queue は `SessionId` が所有し、実行の直列性は既存 admission が持つ（(a)）

```rust
// src/app/session.rs（Sessions のフィールドとして。新 module でも新 service でもない）
pub struct IntentId(u64);                  // OperationId とは別。まだ承認されていない
pub struct QueuedIntent {
    pub id: IntentId,
    pub owner: SessionId,                  // 投入した tab の世代
    pub visit: u64,                        // 投入時の滞在。**診断・表示のみ**。承認の根拠にしない
    pub worktree: WorktreeId,              // attach 時に凍結された対象
    pub kind: OperationKind,
    pub request: IntentRequest,            // 決定 4。plan でも Approved でもない
    pub enqueued: Instant,
    pub state: IntentState,                // 決定 5 の表
}
pub struct IntentQueue {
    per_session: HashMap<SessionId, VecDeque<QueuedIntent>>,
    gate: HashMap<SessionId, ChainGate>,   // 決定 3。anchor は IntentId か走行中 write の OwnerStamp
}
```

- **列は session ごと。ただし session ごとの並行実行では断じてない。** 所有（誰の意図か、誰に
  確認を出すか、誰の chain が切れるか）が session 単位なのであって、実行資源は window に
  1 組しかない。`reserve_lease` は空の lease map を要求する（`src/app/session.rs:500-509`）ので、
  同時に走る write は **常に 0 か 1**。この ADR は `has_leases()` も `reserve_lease` も緩めない。
- **globally single な資源は 3 つ**あり、queue はそのどれも増やさない:

  | 資源 | 実体 | queue 側の規律 |
  | --- | --- | --- |
  | writer lease | `leases`（空でなければ `Busy`） | 同時に `Running` は 1 件 |
  | plan slot + `revision` | `state` / `plan_owner` / `revision`（`src/app/session.rs:232-235`） | `Planning` / `AwaitingConfirm` は **window 全体で 1 件**。さらに `Running` と**重ねない**（`op_may_start` が lease と planning を相互排他にしている、`src/ui/operations/mod.rs:198-204`） |
  | modal slot | `active_modal`（ADR-0093、ADR-0197 決定 1） | confirm を出せるのは前面 session の 1 件だけ |
- **調停の規則（これだけ）**: 候補は各 session の先頭 1 件。候補を `enqueued` の古い順に見て、
  最初に「今すぐ前進できる」ものを 1 件だけ進める。前進できない候補は理由付きで `Waiting` に
  落とし、**次の候補を見る**。したがって 1 つの session の事情が他 session を止めない。
- **前進できる条件（前面 / 背景の分担）**: 背景 session の先頭は **plan slot を取らず**
  `Waiting { NeedsConfirmation }` で待つ。背景で先に replan して confirm だけ後で出す、はしない —
  plan slot は 1 つなので背景の先読みが前面の確認を締め出す（#492 の「A の計画が B に向く」と同じ形）。
  前面 session の先頭だけが `Planning` → `AwaitingConfirm` に入り、その間だけ slot を占有する。
  **pipeline に載る intent は window 全体で常に 1 件**（`Planning` / `AwaitingConfirm` /
  `Admitting` / `Running` の合計 ≤ 1）。現 `op_may_start` は lease と planning を相互排他にして
  いるので、write の実行中に replan を始めることも、確認 modal を開くこともしない。その排他を
  queue が緩めることはしない（緩めれば「走っている write の最中に、その結果を知らない plan を
  見せる」ことになる）。
  **進行の保証は条件つきである**: admission（lease / reconcile）・owner（前面の tab）・modal slot・
  read revalidation の gate が**すべて整った時点で、必ず 1 件が前進する**（決定 5 の調停は
  候補全体を見直すので、整った gate が無駄にならない）。逆に、未解決の `Unknown` が残っている、
  ユーザーが確認に答えない、といった状態では `Waiting { reason }` に**留まるのが正しい**。
  時間切れで gate を外す仕掛けは置かない。**永久停止の回避は「理由が消えれば必ず再評価される」
  ことであって、「いつか必ず動く」ことではない**。confirm modal を開いたまま別 tab へ移れば
  `close_window_slots_of_departing_tab`（`src/ui/operations/modal_state/window.rs:17`）が
  slot を閉じ（ADR-0197 決定 5 S6）、未回答の intent は `Queued` に戻って他候補が再評価される。
- **reconcile**: 同一 `WriteScope` に reconcile entry があれば `begin_write` は `NeedsReconcile` を
  返す（`src/app/flow.rs:302-304`）。その候補は `Waiting { NeedsReconcile }` になり、
  順番を他 session に譲る。解除は既存の reconcile 完了だけで、queue は近道を作らない。
- **owner の lifecycle**:

  | 事象 | 実行中の operation | その session の queued intent |
  | --- | --- | --- |
  | depart（tab 切替） | 継続（ADR-0182） | 保持。ただし先頭は confirm を開けない（決定 4） |
  | close / detach | 継続。lease も reconcile も保持 | **全件 cancel** `CancelReason::OwnerGone` |
  | reattach（incarnation 更新） | 継続 | 全件 cancel（`SessionId` が別物になる） |
  | restore / 再 open（同 path） | — | 継承しない。別 incarnation は別の owner |

  detach が queued intent を捨てるのは ADR-0182 の例外ではなく定義そのものである: queued intent は
  lease も receipt も持たず、捨てても Git の状態も oplog も変わらない。逆に保持すれば、確認先の
  tab が無い intent を後から別の tab で確認させることになり、ADR-0196 決定 3 の owner 契約を壊す。

## 決定 2 — footer / operation strip は「自分の session の観測値」だけを出す（(b)）

- **`queued: N` の定義（1 文）**: N = active session の intent のうち **まだ write が始まっていない
  もの**の数 — すなわち `Queued` + `Waiting` + `Planning` + `AwaitingConfirm` + `Admitting`。

  | 状態 | N に入るか | どこに出るか |
  | --- | --- | --- |
  | `Queued` / `Waiting { reason }` | 入る | 行として reason 付きで列挙 |
  | `Planning` / `AwaitingConfirm` | 入る（**まだ何も書いていない**ので待ち行列の一部） | 先頭行。phase ラベル付き |
  | `Admitting` | 入る（admission 判定中で、まだ lease も `OperationId` も無い。前進すれば同 frame で `Running` になる短命状態） | 先頭行 |
  | `Running` | **入らない** | 「実行中」の別行 + ADR-0086 snackbar |
  | `Settled` / `Cancelled` | 入らない | 履歴 / cancel 一覧（決定 3） |

  他 tab の件数は足さない（ADR-0197 決定 1、背景 tab の状態を前面に混ぜない）。他 session が
  writer を握っている事実は `blocked_by_other_tab: Option<TabLabel>` の 1 行として別に出す
  （件数も intent 名も出さない）。
- strip が描くのは `IntentQueue` と `Sessions` の観測値のみで、**repo snapshot の投影は出さない** —
  「この push の後 ahead は 0 になります」は書かない。楽観的 UI の却下（#355 §7）は実行順序だけでなく
  **表示**にも適用する。intent 状態は決定 5 の表の値をそのまま出し、`Unknown` を「失敗」と
  書き換えない（ADR-0196 決定 2.1）。
- ADR-0086 の busy snackbar は**そのまま 1 本**（今走っている 1 件のラベル）。spinner を 2 個目に
  増やさない。`write_busy_op` / `remote_write` / `planning` の 3 latch は strip 上では
  「今走っている 1 件」の phase として 1 行に畳む（語彙は決定 6 と共通）。
- **警告を件数で上書きしない。** `Unknown` の未解決、reconcile 待ち、`Recording::Failed`（変わったが
  記録できていない）の表示は、queue の件数表示より**優先して残す**。strip は `queued: N` を
  足すだけで、これらの警告行や AppNotice を隠したり置き換えたりしない（#501 の「成功 footer で
  上書きしない」規律と同じ）。

## 決定 3 — 「削除」と「停止」は別物。偽の取消を出さない（(c)）

| 操作 | 対象 | 何が起きるか | oplog |
| --- | --- | --- | --- |
| 1 件の削除（`RemoveOne`） | `Queued` / `Waiting` **のみ** | 列から外すだけ。plan も lease も無いので副作用ゼロ | 記録しない（何も走っていない） |
| 先頭（`Planning` / `AwaitingConfirm`）の取り消し | pipeline に載っている 1 件 | intent を `Cancelled` にし、**自分の** confirm modal を閉じ、`invalidate_plan` で revision を進めて遅れて届く plan 結果を破棄する。plan job が止まったとは主張せず、`planning` latch はその job 自身の終端 callback（`finish_planning`、`src/ui/operations/mod.rs:245-263`）だけが解放する。次の dispatch はその lifecycle の終了を `Waiting { PlanSlotBusy }` で待つ | 記録しない（write は始まっていない） |
| cancel all | 自 session の待機中全件（`Queued` / `Waiting`）＋ 先頭が `Planning` / `AwaitingConfirm` ならその 1 件も上記の手順で | **`Running` の write には触れない**（決定 3 の stop 規律） | 記録しない |
| running の cancel | 実行中 | **提供しない**（本 Draft の決定。理由は下記） | — |
| chain trip（`&&`） | 先頭が非 Success、または先頭が **cancel された**（plan error / ユーザー拒否 / identity 不一致） | 自 session の残り全件を cancel し、**一覧で見せる** | 記録しない |

- **running cancel を提供しない理由。** 停止証明は process group が空であることであり
  （ADR-0196 決定 2.4 と `Termination` 表）、証明できない停止要求の結果は `Unknown` になる。
  `Unknown` は **必ず reconcile を要求する**。`Termination::Stopped` なら lease は解放されるが、
  その scope は reconcile entry がある限り `begin_write` に `NeedsReconcile` で塞がれ
  （`src/app/flow.rs:302-304`）、`Unaccounted` なら lease 自体が保持される（ADR-0196 決定 2.1 / 2.4）。
  「キャンセル」と書かれたボタンが、実際には「結果不明の write を 1 件作り、その scope への次の
  write を reconcile 完了まで止める」ものであってはならない。lease を勝手に解放する
  **見せかけの取消は禁止**（#706 の unobservable release は remote の既知 family 限定の別 tier で、
  取消の手段ではない）。running cancel は決定 8 の条件を満たしてから、別 issue で扱う。
- **chain の anchor（先行者）は intent とは限らない。** `ChainGate` は
  `anchor: Option<ChainAnchor>` を持ち、`ChainAnchor = QueuedHead(IntentId) | ActiveWrite(OwnerStamp)`。
  **queue 経由でない write が走っている最中に、その同じ session で最初の intent を投入した場合**、
  gate はその走行中 operation を `ActiveWrite(OwnerStamp)` として anchor に記録する（#355 の
  「実行中に次を積む」はまさにこの状況である）。新しい列が独立 chain として先に走り出すことは
  なく、anchor が settle するまで先頭は `Waiting { WriteRunning }` に留まる。
  anchor の結果は `apply(Completion)` の **一回性**（`settled` set、ADR-0196 決定 2.5）を通じて
  ちょうど一度だけ gate に配られ、`OperationId` / `OwnerStamp` が一致した receipt だけを見る。
  「busy なら enqueue して新しい chain を始める」形は**禁止** — 先行者を失えば `&&` が成立しない。
- **anchor にできるのは「追跡できる先行者」だけ。** 条件は `OperationId` と `OwnerStamp` を持ち、
  終端が `apply` の権威ある completion として届くこと。**lease を持たない legacy な
  `remote_write`（SSH pull、`src/ui/busy.rs:9-27`）はこれを満たさない** — `apply` の receipt が
  届く保証が無いので、anchor を**でっち上げない**し、「queue が包んでいる」とも書かない。
  この拒否が働くのは **自 session に write が走っていて、その先行者の identity / 所有を
  権威ある形で解決できない場合だけ**である（legacy remote の例）。**走っている write が無ければ
  `anchor = None` が正常な状態**で、idle の列への投入はそのまま受け付ける（適格 family の
  entry point は idle でも typed intent を積む、§0）。守れない連鎖を見せるより弾く方が誠実だが、
  先行者が存在しないことを拒否の理由にはしない。
- **別 session で走っている write は anchor にしない**。追跡できている writer であっても、それは
  global な blocker（`Waiting { WriteRunning }`）にすぎず、他 tab の失敗が自分の列を cancel する
  ことはない。
- **次に進む条件（footer の見た目ではなく receipt で判定する）**。anchor が終わったとき、gate が
  `Armed` のままでいられるのは次の 4 つが**すべて**成り立つときだけで、1 つでも欠ければ trip する:

  | 条件 | 値 | 欠けたときに trip する理由 |
  | --- | --- | --- |
  | 意味的結果 | `SemanticOutcome::Success { after }` | `Refused` / `Failed` / `Partial` / `Unknown` は後続の前提を保証しない（ADR-0196 決定 2.1） |
  | 実測された postcondition | `ExecutionEvidence::Verified { .. }` | `Unverified` を `Success` に畳み込むことを ADR-0196 決定 2.3 が禁じている。適格な family は verify 経路を持つ（決定 4）ので、queue 経由の write に `NotApplicable` は出ない |
  | 記録 | `Recording::Appended { .. }` | `Recording::Failed` は「変わったが記録できていない」。成功 footer で上書きしない規律（ADR-0196 Wave 3 メモ / #501）を chain にも適用する |
  | admission の解放 | `apply` が settle し、reconcile requirement が無い | `Unknown` は reconcile 必須で、`Stopped` は lease を解放しても scope が塞がり、`Unaccounted` は lease ごと保持される（同 2.1 / 2.4）。塞がったまま次を始められない |

  **判定源は `ExecutionReport` と `apply` の結果だけ**で、footer / snackbar の文字列は入力にしない。
- **cancel された先頭も tail を trip する。** chain の意味は「直前が**成功した**こと」なので、
  先頭が plan error / ユーザー拒否 / identity 不一致で `Cancelled` になった場合も後続は前提を
  失っている。`CancelReason::ChainTripped { by }` で同じ一覧に入れる。
- **削除の方針は `RemoveOne`（選んだ 1 件だけを消す）。** suffix を巻き添えにする `CancelSuffix` は
  採らない: 削除はユーザー自身の明示的な決定であって失敗ではなく、`&&` は「**失敗**したら以降を
  止める」規則だからである。残りは順序を保って繰り上がり、各 intent は先頭に来た時点で決定 4 の
  フルパイプライン（replan → 確認 → preflight）を通るので、消した 1 件に依存していた前提は
  **確認画面と preflight で可視化される**。queue が依存関係を推測して巻き添えにはしない。
  列ごと消したいときは `cancel all`（自 session の待機中全件）を使う — 2 つの操作の意味は
  「1 件を降ろす」と「この tab の待ち行列を空にする」であり、曖昧な中間はない。
- **trip からの復帰（gate reset）。** `Tripped` は自動前進だけを止める。cancel 一覧をユーザーが
  破棄した時点、または対象 session の列が空になった時点で gate は `Armed` に戻り、以後の投入は
  普通の chain として動く。**空の列に投入された intent は常に `Armed` で始まる** — これがないと
  一度 trip した tab が永久に動かない（#355 §6 最終項の再来）。
- **「受理された」は「完了した」ではない。** server 側が非同期に完了させる family では、
  成功した submission が **postcondition を意味しない**: GitHub merge queue に入った PR は
  `PrMergeLocalReason::Queued` = 「`gh` は受理し GitHub は queue に入れた。まだ何も merge されて
  いないので後始末する対象も無い」（`crates/kagi-domain/src/plan_note/github.rs:202-204`、
  ADR-0202、ADR-0153 決定 3）。したがって chain の前進判定は「呼び出しが成功したか」ではなく
  **「この family の postcondition が `ExecutionEvidence::Verified` で確認できたか」**で行い、
  受理どまりの結果は trip 扱いにする。受理を根拠に後続 intent を走らせない・依存する後片付け
  （local branch 削除など）を自動で起こさない・「merged」表示にしない。
- **cancel は stop request であって補償操作ではない。** 停止要求は走っているプロセスを止める
  試みに限られ、abort / 巻き戻し / revert のような **Git 状態を戻す write を暗黙に発行しない**。
  それらは従来どおり各 family の plan → confirm → write として、ユーザーが明示的に選ぶ。
- **cancel 一覧の保持規則（明示）。** cancel した intent は `CancelReason` を持ったまま自 session の
  strip に残る（#355 §2「キャンセルされた操作を列挙できる」が誠実さの根拠なので、toast で流して
  消さない）。**寿命の出口は 3 つ**: (1) ユーザーが一覧を破棄したとき、(2) owner の detach /
  reattach（intent は owner のものなので一緒に消える）、(3) プロセス終了。これとは別に
  **表示量の上限**として session あたり直近 32 件を保ち、超えた分は古いものから落とす。
- `;`（前が失敗しても続ける）は**採らない**。#355 §5 の論点に対する本 ADR の決定であり、
  前提が壊れた状態で次の write を走らせるのは Kagi の plan → confirm → preflight と矛盾する。

## 決定 4 — 積むのは intent。承認済み plan を凍結して使い回さない（(d)）

```rust
pub enum IntentRequest {            // ユーザーの決定入力。安定した対象 identity は持ってよい
    Checkout { target: RefName },
    Stage { paths: Vec<RepoRelPath> },
    Commit { draft: CommitDraftRevision },  // 本文は revision で凍結する（下記）
    Merge { source: RefName, into: RefName },
    CherryPick { commit: Oid },             // ユーザーが選んだ commit の OID は intent の identity
    // …適格な family ごと。非同期 server family は適格条件を満たさない（下記）
}
```

- **`Approved` / `PlanToken` / `OperationPlan` を queue に入れない。** 承認は one-shot で
  revision に束縛されており（`src/app/flow.rs:298-300`）、保存した承認を後で使うことは
  「古い承認が新しい plan を認可する」ことと同義になる。queue が持つのは *何をしたかったか* だけ。
- **「OID を持たない」ではなく「予測を持たない」。** ユーザーが明示的に選んだ対象の identity —
  cherry-pick / revert の commit OID、対象 ref 名、対象 file path — は **intent そのもの**であり、
  凍結して構わない（むしろ凍結しないと「どれを選んだか」が行 index の変動で失われる、#286 と
  同じ論点）。持ってはいけないのは **plan が計算した予測** — diff、merge base、ahead/behind、
  実行時に解決されるべき `HEAD` や upstream の値である。
  可変なテキスト入力（commit message draft）は **参照だけでは足りない**: 確定した内容か、
  内容に紐づく revision（`CommitDraftRevision`）として凍結し、先頭に来たときに元の draft が
  書き換わっていれば確認画面でその差を見せる。ID だけを持って後から読むと、
  「ユーザーが承認していない本文で commit する」ことになる。
- **owner の再解決は照合の後だけ。** 先頭処理では `QueuedIntent.owner` と `worktree` を現在の
  `Sessions` と照合し、**同じ `SessionId` かつ同じ `WorktreeId` のときに限り**その時点の
  `Attachment`（現在の `visit` を含む）を取り直す。`QueuedIntent.visit` は診断・表示のためだけの
  記録で、承認の根拠にも routing 鍵にもしない。照合が通らなければ別 owner に向け直さず、
  決定 3 の cancel 規則に従う。
- **投入できる family は「適格条件」で決まる（固定の列挙ではない）**: 次の 3 つを満たすと
  **その family について示せたもの**だけが queue に入れる。(1) 成功が **local に観測でき**、その
  family の verify 経路が `ExecutionEvidence::Verified` を返せる、(2) 「受理」と「完了」が
  分岐しない（server 側の非同期完了を含まない）、(3) 失敗時に後続を止めれば前提が閉じ、外部に
  取り残す状態が無い。**PR merge（GitHub merge queue を含む）は (2) で落ちる** — 成功が受理で
  あって merge 完了ではない（決定 3）。**remote に効果が残る family（push、remote pull など）は
  (1) を自動的には満たさない** — local snapshot は remote の状態を語らない（ADR-0196 決定 5 の
  `RemoteExpectation` と同じ理由）ので、その family 固有の検証手段を示せない限り適格ではない。
  （`fetch` は remote を書かず local の remote-tracking ref を更新する操作なので、この段落の
  対象ではない。適格性は family ごとの証拠で判定し、Git の意味論を一括で言い切らない。）
  **未判定の family は既定で対象外**。対象外の family は従来どおり単発で、実行中は現行
  `reject_if_busy` の拒否のまま（本 Draft はここを変えない）。
- **先頭に来たときのパイプライン（毎回フル）**:

  ```
  live replan → ユーザー確認（新しい modal）→ approve_run / approve → admission
               （`begin_write`。guard を直接取る既存 family は `write_lease` の同じ判定を通る。
                どちらの route も迂回しない）
              → preflight（live 再検証。凍結 request と今の repository の照合）
              → execute → verify（ExecutionEvidence）→ durable oplog（Recording）
  ```

  ADR-0196 のパイプラインそのままで、queue が決めるのは開始時刻だけ。preflight は #704 規範
  （ADR-0196 決定 4 の規範節）のとおり accepted read revision + Backend の live preflight で、
  **確認の後・実行の前に必ず通る**。ここで前提が崩れていれば `Refused` になり、chain は trip する。
- **「確認したのに、また確認するのか」への答え**: 先行する write が後続の前提を変えるから。
  `rebase onto origin/main` → `merge feature` では rebase が commit を書き換えるので、merge base も
  競合判定も投入時点の予測とは**別物になる**。`commit` → `amend`（対象 OID を先行が作る）、
  `checkout feat` → `delete branch feat`、`stage` → `commit`
  （commit に入る index の内容を先行が変える）も同じ形。**適格条件を満たさない PR merge も同じ原理を示す**: `--match-head-commit
  <SHA>` が常に付き（`crates/kagi-git/src/github_merge.rs:357-383`）、SHA は plan 表示時に凍結される
  （`src/ui/modals.rs:116-123`）。凍結値を持ち越せる設計にしていたら、head が動いた PR に対して
  「確認済み」の顔をした refuse を量産していた。
- **先頭処理直前の照合と、その結果の振り分け**: (i) owner が attach 済みで `SessionId` が一致するか、
  (ii) `Sessions::confirm_identity`（`src/app/session.rs:365`）で凍結 `WorktreeId` が今も同じものに
  解決するか、(iii) owner の read が revalidate 中か。**(i)(ii) の不一致だけが cancel**
  （`CancelReason::OwnerGone` / `IdentityChanged`）であり、**黙って別の対象に向け直さない**。
  (iii) の「read がまだ新しくない」は失敗ではなく待ちなので、`Waiting { NeedsConfirmation }` に
  留めて次の候補を見る（ADR-0197 決定 3 の activate 時 revalidation）。
- **modal を奪わない**: confirm を開く条件は (1) owner == active session、(2) `active_modal` が空、
  (3) `panes_revalidating()` が false — 既存 `pane_mutation_admitted`
  （`src/ui/operations/mod.rs:234-236`）と同じ形の gate。満たさない間は
  `Waiting { NeedsConfirmation }` に留まる（ADR-0093 の 1 slot、ADR-0197 決定 1）。

## 決定 5 — 状態遷移表

| 状態 | 意味 | 次 | 遷移条件 |
| --- | --- | --- | --- |
| `Queued` | 列に入った。まだ何も読んでいない | `Planning` / `Waiting` / `Cancelled` | 調停で選ばれた / 選ばれない / 削除・trip・owner 喪失 |
| `Waiting { reason }` | 走れない理由が名前を持っている。`reason ∈ { WriteRunning, PlanSlotBusy, NeedsConfirmation, NeedsReconcile, RemoteLatched }`（trip 後の intent は `Waiting` ではなく `Cancelled` になるので、chain 由来の待ち状態は存在しない） | `Planning` / `Cancelled` | 理由が消えた / 削除・trip |
| `Planning` | live replan 実行中。**plan slot を占有**（既存 `planning` latch） | `AwaitingConfirm` / `Queued` / `Cancelled` | plan 完了 / tab 離脱で slot ごと破棄 / plan error・identity 不一致 |
| `AwaitingConfirm` | confirm modal を前面に出している。**plan slot と modal slot を占有** | `Admitting` / `Queued` / `Cancelled` | ユーザー承認 / tab 離脱で modal が閉じる（未回答なので `Queued` に戻す） / ユーザー拒否・owner 喪失 |
| `Admitting` | `approve_*` → admission（`begin_write`、guard family は `write_lease`）。lease も `OperationId` もまだ無い短命状態 | `Running` / `Waiting` / `Cancelled` | 成功 / `Busy`・`NeedsReconcile` は `Waiting` へ差し戻し / `StaleApproval`・`Identity` は cancel |
| `Running` | lease 保持。`OperationId` と `OwnerStamp` が付いた。**ここから先は close しても取り消されない**（ADR-0182） | `Settled` | `apply(Completion)` |
| `Settled { outcome }` | 終端。lease / reconcile は ADR-0196 決定 2.1 のまま | — | 決定 3 の 4 条件を満たせば gate は `Armed` のまま、でなければ `Tripped` |
| `Cancelled { reason }` | 列から外れた。副作用なし | — | 一覧が破棄されるまで表示に残る（決定 3 の保持規則） |

`CancelReason ∈ { UserRemoved, UserRejected, ChainTripped { by }, OwnerGone, IdentityChanged, CapacityRejected }`。

**詰まりが名前を持つことの根拠（#355 §6 最終項）**: 前進できない理由は必ず `Waiting { reason }` と
して名前を持ち、各 reason には**それを解除しうる観測可能な事象**が対応する。下表はその対応であって、
その事象が必ず起きるという主張ではない（未解決の `Unknown` や、ユーザーが確認に答えない状態は
`Waiting` のままが正しい）。保証するのは**事象が観測されたら必ず候補全体が再評価される**ことである。

| reason | 解除する事象 | 観測点 |
| --- | --- | --- |
| `WriteRunning`（どの tab の write でも） | lease 解放 | `apply(Completion)` → `release_lease`（`src/app/session.rs:512`） |
| `PlanSlotBusy` | plan job が実際に終わること（`finish_planning` が自分の tag を解放する、`src/ui/operations/mod.rs:245-263`）。`invalidate_plan` は遅延結果を捨てるだけで slot を空けない | `finish_planning` の終端 callback、`invalidate_plan`（`src/app/session.rs:523`）、`close_window_slots_of_departing_tab` |
| `NeedsConfirmation` | owner tab が前面に戻り、**かつ** `active_modal` が空、**かつ** `panes_revalidating()` が false、**かつ** pipeline が空（`Running` 無し） | `active_session()` / `active_modal` / `pane_mutation_admitted`（`src/ui/operations/mod.rs:234-236`）/ `op_latched` |
| `NeedsReconcile` | reconcile の acknowledge | ADR-0196 決定 2.4 |
| `RemoteLatched` | pull の終端 callback | `remote_write` の解除（`src/ui/busy.rs:25-27`） |

`Admitting` の `Busy` / `NeedsReconcile` が失敗ではなく差し戻しであること、
`AwaitingConfirm` の tab 離脱が cancel ではなく `Queued` 復帰であること、この 2 点が
「詰まって永久に動かない」も「黙って消える」も避ける要である。bool latch にはこの対応表が無かった。

## 決定 6 — 2 秒を超えたら理由を出す。ETA は作らない

- 閾値 2s は git の `advice.statusAheadBehind` に合わせる（#355 §2）。計測の起点は
  **現在の phase の開始時刻**で、intent の投入時刻ではない（待たされた時間を計算時間として出さない）。
- 出すのは `{ 理由, 経過秒, 取れる手段 }` の 3 点。**残り時間・進捗率・「まもなく完了」は出さない** —
  分布を持っていないので、出せば捏造である（唯一の例外は GitHub 自身が返す
  `QueuePosition::estimated_time_to_merge`、`crates/kagi-domain/src/merge_state.rs:150-157`。出典が
  server なのでそのまま引用する）。
- **理由は phase から機械的に引く**。例: 「ahead/behind を計算中（commit 数が多い repository では
  時間がかかります）」「worktree を列挙中」「大きい diff を読み込み中」。分類済みでない phase は
  汎用文（「処理中」）+ 経過秒にし、**もっともらしい原因を推測して書かない**。
- **取れる手段も必ず書く**。skip できる計算なら skip を提示し、できないなら
  「この処理は中断できません（実行中の write です）」と明示する — 手段の無い待ちに
  「お待ちください」だけを出さない。表示できる手段が無い場合は、その理由（write の実行中など）を
  そのまま書く。
- **skip は「計算をやめて不明として表示する」だけ**（#355 §5 の論点への決定）。対象は**派生 read の
  表示値に限る** — ahead/behind、worktree 収集、大きい diff、Analyze。
  **plan / preflight / verify / identity 照合の入力は skip の対象外**: それらは表示ではなく
  admission と検証の根拠なので、飛ばせば「確認せずに実行する」ことになる（楽観的 UI の却下と
  同じ理由）。write そのものにも出さない（それは cancel であり本 Draft では提供しない）。
- **skip の結果は実行を認可しない。** skip した値は `Unknown` として描き、計算済みの値として
  cache せず、`Unknown` を根拠に mutation を始めることも chain を前進させることもしない。
  次の明示的な refresh でやり直す。skip は intent の状態を変えない（`Running` の write は走り続ける）。

## 決定 7 — 範囲外と端（Draft の既定値）

| 論点 | Draft の決定 | 理由 |
| --- | --- | --- |
| `;` 継続 | 入れない | 決定 3 |
| 並列実行 | 入れない | #283 / ADR-0182。global single-writer 維持 |
| 永続化 | しない（process 終了で消える） | 復元した intent は「最も古い前提」で再開する。決定 4 の live replan が毎回必要なら、保存する価値は列の順序だけで、安全性の負債の方が大きい |
| fairness | 候補（各 session の先頭）の中から `enqueued` 最古を選ぶ | 決定的で、飢餓しない。優先度も昇格も入れない |
| 容量 | 1 session あたり 16 件で上限。超過は `CapacityRejected` を即返す（既存 notice 経路） | 無限成長と、人間が追えない長さの chain を同時に防ぐ。16 は「画面に出せる長さ」から来た保守的な値で、測定後に上げてよい |
| coalescing | しない | 同種 intent を畳むと、trip 時の cancel 一覧が「ユーザーが投入した操作の集合」と一致しなくなる。誠実さ（決定 3）のコストの方が、節約できる 1 回の replan より大きい |
| 自動再試行 | しない | 非 Success は必ず chain を trip する（決定 3） |
| 非同期 server family（PR merge / GitHub merge queue） | 適格条件を満たさないので queue に入れない | 成功が「受理」であって「merge 完了」ではない（決定 3・決定 4）。kagi 側に server queue や watcher を新設もしない |
| 停止要求と補償操作 | cancel は stop request のみ。`--abort` / `reset` / revert は発行しない | 決定 3。戻す操作は各 family の plan → confirm → write |

## 決定 8 — ADR-0153 繰延の解除条件と受入 oracle（(e)）

ADR-0153 `## Consequences` の **71 行目**「`gh pr checks --watch` background job は #355 に繰延」だけが
本 ADR の射程である。**70 行目**（CODEOWNERS を base ref から読む）と **72 行目**（required approval
count の精密化）は別件で触れない。ADR-0153 は Accepted のまま、繰延記述も**現時点では真**である。

**Queue 側の基本条件（先に満たすもの）**

| # | 条件 | 測定方法 |
| --- | --- | --- |
| Q1 | 実行中に intent を投入でき、順に走る | Tier A: `IntentQueue` の reducer に fake completion を与え、遷移表（決定 5）の全行を assert |
| Q2 | 非 Success で後続が全 cancel され、一覧が残る | Tier A: `Refused` / `Failed` / `Partial` / `Unknown` の 4 値それぞれで tail が `ChainTripped` になること |
| Q2b | 進行判定が receipt に基づく | Tier A: `Success` + `Recording::Failed`、`Success` + `ExecutionEvidence::Unverified`、`Success` + reconcile requirement 有りの 3 例で **trip する**こと（footer 文字列は入力にしない） |
| Q3 | 列の内容が UI に出る（自 session 分のみ） | Tier B: 2 session fixture。A の strip に B の intent が現れないこと |
| Q4 | 詰まりが必ず名前を持ち、解除で再評価される | Tier A: 決定 5 の解除事象表の各行について、その事象が観測されたら**候補全体が再評価される**こと（当該 intent が必ず `Planning` に進むとは限らない — 別の gate が残れば別の reason の `Waiting` になる）。到達不能な `Waiting` が作れないことを網羅 match で固定。trip 後に列が空 → 新規投入が `Armed` で始まることも同じ test で固定 |
| Q5 | 古い承認が新しい plan を認可しない | Tier A: intent に `Approved` を持たせられない（型で不可能）＋ Tier B: 先行 write 後に先頭 intent が replan と再確認を経ること |
| Q6 | detach で queued が消え、実行中は消えない | Tier A（既存 `app_writer_admission_test` の隣）: detach 後 lease 保持・queued 0 件 |
| Q7 | 2 秒超で理由と手段が出て、skip が効く | Tier B: 遅延を注入した read で `{ 理由, 経過秒, 取れる手段 }` の 3 点が出ること、中断できない待ちではその旨が出ること、skip 後に値が `Unknown` になり cache されないこと。**plan / preflight / verify / identity 照合の入力には skip が出ず、`Unknown` の表示値が mutation も chain 前進も認可しない**こと |
| Q8 | GUI 目視（#355 §6 最終項） | native: strip / snackbar / cancel 一覧の実機確認。自動テストでは代替しない |
| Q9 | global な 1 slot を増やしていない | Tier A: 任意の遷移列の後で `Planning` + `AwaitingConfirm` + `Admitting` + `Running` の総数 ≤ 1 が window 全体で成り立つ（`op_may_start` の排他をそのまま保つ）。背景 session の先頭が `Planning` に入れないこと |
| Q10 | 確認中に他 session が永久に待たない | Tier B: A で confirm を開いたまま B へ切替 → A の intent は `Queued` に戻り、B の先頭が進むこと |
| Q11 | 受理を完了として扱わない | Tier A: 非同期 server family の受理（`PrMergeLocalReason::Queued` 相当）で chain が **trip** し、依存する後片付けも「merged」表示も起きないこと。適格条件を満たさない family は enqueue 自体が拒否されること |
| Q12 | 先頭 cancel が「止めた」と偽らない | Tier A: `Planning` / `AwaitingConfirm` の cancel で intent は `Cancelled`、自分の modal は閉じ、遅れて届く plan 結果は revision 不一致で捨てられ、**`planning` latch は job の終端 callback でのみ解放**され、次の dispatch は `Waiting { PlanSlotBusy }` を経ること |
| Q13 | queue 経由でない先行 write も `&&` の anchor になる | Tier A: 同 session で走行中の operation（`OperationId` + `OwnerStamp`）を anchor に持つ列が、その `apply` receipt 一回で判定されること。非 Success なら新しい列が全 cancel されること。**別 session の write は anchor にならず**、失敗しても自分の列は残ること。追跡できない先行者（lease を持たない legacy `remote_write`）では enqueue せず既存の拒否を返すこと |

**`gh pr checks --watch` 固有の解除条件（Q1–Q13 に加えて全て必須）**

| # | 条件 | 根拠 |
| --- | --- | --- |
| W1 | watch は **read-only** であり、write lease を取らない・`has_leases()` を true にしない・queue の順番を消費しない。watch 実行中でも write の `begin_write` が admit できる | `src/app/flow.rs:305-307` の global busy は write の直列化のためであり、poll read を止める理由は無い |
| W2 | watcher は起動時に **owner（`SessionId` + `visit`）、repository identity（`-R <host>/<owner>/<repo>` と同じ解決）、PR 番号、head SHA** を凍結する。結果はその stamp に一致する tab にだけ着地する | ADR-0196 決定 3、`src/ui/modals.rs:116-123`（head SHA の凍結）、`crates/kagi-git/src/github_merge.rs:370-378`（`-R` で repository を凍結する既存理由） |
| W3 | head SHA が凍結値から動いたら、その watch の結果は**破棄**し、古い緑を表示し続けない。`PrMergeStatus` は head SHA を持たない（`crates/kagi-git/src/github_merge.rs:71-77`: `node_id` / `state` / `queue` / `unresolved_threads`）ので、SHA の照合は watcher 自身の責務である | 上記構造の実測 |
| W4 | 寿命が有界であること: 明示停止・owner detach・window close・上限時間のいずれかで必ず終わる。プロセスは `gh` の子であり、停止は既存 supervisor の経路（ADR-0196 決定 5「終端未確定の出口」）に乗る | 放置された `--watch` は無限に回る |
| W5 | watch の結果を **merge の認可に使わない**。「checks 全緑」は merge eligibility ではない。merge は従来どおり plan → confirm → preflight を経て `--match-head-commit <SHA>` で実行し、その SHA は confirm 時に凍結したものを使う | `crates/kagi-git/src/github_merge.rs:357-383` の safety invariant、`MergeStatusView::show_admin_button` が常に false である理由と同じ ethos（ADR-0153 決定 6） |
| W6 | watch は `mergeStateStatus` を置き換えない。表示は「checks の観測」であって、merge 可否の判定は既存 `MergeStatusView`（`crates/kagi-domain/src/merge_state.rs:176-187`）のまま | 二つ目の merge 判定源を作らない |
| W7 | 2 秒ルールが watch にも適用され、skip は「監視をやめて **不明** と表示」。PR の状態を「成功」と言い換えない | 決定 6 |
| W8 | native case: 実 PR に対し (a) watch 中に write が admit できる、(b) 監視中に head が動いたら結果が破棄される、(c) tab close で watcher が止まる、を人が確認する | GUI / 外部サービスを伴うので Tier A/B では閉じない |

**Draft の正直な現状**: 上表はどれも未実施である。ADR-0153 の繰延が解除されたと書けるのは、
W1–W8 と Q1–Q13 が実装・実測されたときだけで、本 ADR の Status が Draft である限りその主張はしない。

## 未解決リスク（Draft）

1. **確認の回数**。先頭ごとに confirm が出るため、3 件積むと 3 回確認する。安全性の対価として
   受け入れるが、体感が #355 の目的に反しないかは native（Q8）で測る。「確認の束ね」は plan の
   束ね＝前提の共有になるので、安易な緩和策にはしない。
2. **`remote_write` latch の穴**（`src/ui/busy.rs:22-24`、既知）: remote pull は lease を持たず
   `may_close_host` に効かない。queue 化は悪化も改善もさせないが、`Waiting { RemoteLatched }` が
   この latch に依存する以上、lease 化（#703 の follow-up slice）と同時に見直すのが望ましい。
3. **16 件という上限**に測定根拠が無い（決定 7）。実測前の保守値である。
4. **cancel 一覧の表示量**。決定 3 の保持規則（破棄・detach・quit・上限 32 件）で寿命は閉じているが、
   32 件が実際に読める量かは native（Q8）で確かめる必要がある。
5. **plan slot の競合**。window に 1 件である以上、tab を行き来しながら複数の列を進めると確認の
   順番待ちが出る。停止は決定 1 の「離脱で slot 解放」で防げるが、体感は Q10 と native で測る。
   plan slot を session 別にする案は ADR-0182 の単一 slot 決定に触るので本 ADR では採らない。
