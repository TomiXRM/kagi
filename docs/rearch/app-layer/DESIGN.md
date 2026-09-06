# #484 アプリケーション層 — 所有・実行・配送の設計案

状態: **レビュー用 Draft / 未合意・未実装**。2026-09-06。
読解基準: `origin/dev` = `85b0159a6d9455632db1c6cf758459d1b6e9558d`。
この PR は文書だけ。コード・ビルド・実測は含まない。

進め方は **設計レビュー → 1 family の縦断実証 → 横展開**。
目的はフロントエンドを交換しても安全契約と操作の所属が変わらないこと。
巨大な `KagiApp` を巨大な controller に移すことや、新 crate の作成自体は成果に数えない。

## 0. 今回決める案と、まだ決めないこと

| 項目 | 本文の推奨案（PM 合意待ち） | 実証・後続で決めること |
|---|---|---|
| 所有境界 | application は進行・所属・配送、Backend/transport は安全判定・mutation・verify・記録 | family ごとの最小 API 名と内部配置 |
| session | map が単独所有、active は `TabId`。tab と worktree session は別 | 全 `TabViewState` 移管の粒度・cache 容量 |
| 排他 | 初期は同一 common Git directory の全 mutation を直列化 | index-only を worktree ごとに並行化する利益と証明 |
| 実行 | GPUI 非依存の owned job と完了メッセージ。spawn は adapter host | worker 導入は #314 の独立実証後 |
| 記録 | mutation 境界からその実行の receipt を返す。UI は append しない | multi-process 採番・crash recovery journal は別設計 |
| 配置 | root `src/app/` の小さな session/family module から | 本物の第 2 consumer が必要になった時点の crate 抽出 |
| 第 1 family | worktree remove（#501） | 実証で境界が成立しなければ横展開しない |

`architecture.md` の古い「app に GPUI」「controller が oplog を書く」図を
実装済みの事実として継承しない。S5 deferred と ADR-0121 の pane 分離は維持する。

## 1. 責務所有表

パスはリポジトリルートから。提案型はすべて仮称。

| 層 | 所有するもの | 所有しないもの | 現行コードでの担当箇所・差分 |
|---|---|---|---|
| domain | `Operation`/outcome、identity の値、承認・進行の純粋な判定、conflict FSM | git2、GPUI、serde 依存、I/O、thread | `crates/kagi-domain/src/{operation,plan,resolution,history}.rs`。既存型を再利用。`OperationPlan` 自体は今は Git crate |
| application session | `RepoId`/`WorktreeId` と寿命、read state/cache、request revision、進行中 operation、follow-up の所属 | focus/scroll/Entity、Git primitive、永続記録の writer | 現在は `ui/{mod,tabs,reload,tab_view,worktree_wip}.rs` に分散。既存 `git::RepoSession` はこれではない |
| application family service | request→plan job、approval の束縛、admission、job と結果の対応、対象別 invalidation | trust の実装、preflight/verify の再実装、表示文言 | `ui/operations/*`、`blocking_ops.rs` の orchestration 部分。family ごとに小さく移管 |
| Git backend / execution boundary | repository 解決、trust 再評価、policy 適用、plan/preflight/execute/verify、復元材料、記録 receipt | active tab、modal、toast、window 寿命 | `git::Backend::{plan,run,run_history_move}`、`backend/recording.rs`、`ops/*`。non-run の欠落をここで閉じる |
| Git worker（後続） | Backend handle の thread 所有、直列実行、liveness、完了保証 | 承認 UX、session map、再実行の可否を推測すること | `git::{session,worker}.rs`。production submit caller は未配線。初回は使わない |
| GUI adapter / host | input、`WriteOrigin` 解決、承認表示、GPUI Task の実行・main-thread 復帰、`TabView`、通知 | per-handler busy/世代 guard、記録、安全 policy のコピー | `ui/operations/mod.rs`、`modal_state.rs`、pane event/seed seam。host は window より長寿命 |
| CLI adapter | argv、明示承認、stdout/stderr、exit code | 独自 operation resolver/plan serializer、global oplog tail の推測 | `src/cli_main.rs`。共通契約の同期 consumer |
| MCP adapter | tool envelope、接続ごとの plan store、承認 token の消費、protocol error | 独自安全 gate、再plan失敗後の旧承認再利用 | `crates/kagi-mcp/src/write.rs`、server の plan store |
| transport / filesystem capability | PR/SSH の実行・結果照会・記録、editor の明示 FS 操作 | GPUI、active tab、安全な Git operation と偽ること | `git::{github,github_merge}`、`src/remote/mod.rs`、`ui/editor_fs_ops.rs` と editor 内 FS job。後二者は順次 UI 外へ |

## 2. 識別・排他・寿命

### 2.1 path は locator、tab は表示先、repo は排他資源

| 識別子 | 解決規約 | 寿命 / 再利用 | 用途 |
|---|---|---|---|
| `RepoId` | Backend が解決した canonical common Git directory を process 内で intern | session/operation が参照中は存続。再open時は incarnation を検証 | linked worktree 間で共有する refs/ODB/admin 資源 |
| `WorktreeId` | `RepoId` + canonical per-worktree Git directory（main も明示）+ incarnation | 削除後は tombstone。同じ名前・path の再作成は別 incarnation | HEAD/index/status/conflict、書込対象 |
| `TabId` | process 内の単調 ID。配列 index/path ではない | close で廃止、再利用しない | 選択・scroll・modal の表示先 |
| `OperationId` | 実行試行を一意に区別する opaque ID | plan digest/entry の連番とは別。receipt と完了を束縛 | 二重配送防止、失敗の照会 |
| `RequestId` | session incarnation + read 種別 + 単調 revision | request 完了/置換で失効 | loading/data/error の一体所有 |
| remote identity | transport authority（接続先/user/port 等）+ remote 側の解決済み repo/worktree identity | alias 名だけで同一視しない。再接続は検証 | ローカル path canonicalization と混同しない |

Backend が canonicalize/discover する。UI の `canon(path)` の失敗時 raw-path fallback は
表示・過去ログ検索には残せるが、**新規 mutation の排他 identity には使わない**。
解決不能は refusal。削除予定の worktree の locator は dispatch 前に凍結し、完了時に再解決しない。
外部で同じ path を置換された場合に備え、preflight は admin identity・対象 ref/OID・config digest
も検査する。path の一致だけでは承認を復活させない。

| 資源 | 初期の排他キー | 理由 / 限界 |
|---|---|---|
| local Git mutation（index-only、fetch、snapshot 含む） | `RepoId` | HEAD/index は別でも refs/stash/ODB/admin は共有。remove と sibling commit も競合させない |
| worktree lifecycle | 管理元 `RepoId`、対象 `WorktreeId` は payload | tab の repo と削除される worktree は違う。管理元から記録・verify できる |
| PR/SSH mutation | transport authority + repository identity。local 更新もあれば local `RepoId` を併用 | 複数キーは順序固定し、一括 admission。network timeout だけでは解放しない |
| editor FS write | 所属 worktree の `RepoId`（移行後） | Git mutation と同じファイルを書き得る。plan を省略することと排他を省略することは別 |
| read | write lease を保持しない。read revision で整合を管理 | 作業中の status は stale と表示。mutation 前の read が後着しても fresh に戻さない |

busy は `HashMap<ConflictKey, OperationId>` 相当の admission 状態。キュー UX は作らず
競合 request を Busy として拒否する。独立 repo は並行可。承認待ち中は lease を占有しない。
これは **同一 process の協調排他**であり、別 CLI process・外部 Git を止める保証ではない。
Git の lock、preflight、lease/OID 条件は Backend に残す。cross-process transaction lock は未合意。

### 2.2 状態と close

```rust,ignore
// 配置/所有のスケッチ。実 API ではない。
struct Sessions {
    repos: HashMap<RepoId, RepositoryState>,
    worktrees: HashMap<WorktreeId, WorktreeSession>,
    operations: HashMap<OperationId, InFlight>,
}
struct GuiWorkspace {
    tabs: HashMap<TabId, TabView>, // selection/scroll/focus/Entity/modal
    order: Vec<TabId>,
    active: Option<TabId>,
    // Sessions は host 側に所有され、window の drop では落ちない。
}
```

| 状態 | owner / 保持 | 切替・close 規約 |
|---|---|---|
| immutable snapshot/graph rows/details | WorktreeSession。`Arc` 等で view と共有、active/cache の deep clone をなくす | A→B→A は参照変更。閉じた session の cache は最終参照解放時に破棄 |
| refs/history | RepositoryState + worktree 文脈 | refs 変更時は sibling snapshot も invalidation。undo は対象 worktree/branch/OID を再検証 |
| operation state | Sessions の operation map、対象 session は pinned | close は実行取消ではない。完了まで最小 identity/receipt/invalidation 所有を残す |
| selection/scroll/focus/editor buffers | TabView / 既存 pane Entity | 同一 tab 再選択は no-op。背景 tab close は active を触らない。dirty guard は閉じる tab のみ |
| modal | GUI の既存 `ActiveModal` + owner `TabId`/plan token | repo 離脱で repo modal と未dispatch承認を破棄。global settings modal は別分類 |
| conflict session / follow-up | WorktreeSession が意味状態、view が編集 chrome | stash 後続は `(WorktreeId, source stash OID, conflict revision)`。離脱/close/cancel で提案破棄。OID 不明なら提案しない（#485） |
| diff/history loading | request-scoped `LoadState<T>` | A→B は直ちに B request。A の後着は B の loading を解除も固定もしない（#489） |
| watcher/PTY | host の session resource + GUI view attachment | 最終 tab close で read watcher/cache/PTY を既存 guard 後解放。実行 job は切り離して存続 |

`TabViewState` のデータ/派生計算を再利用し、GPUI 部分と純粋 read model を段階分離する。
row-index cache は OID/path/options への全 consumer 移行まで既存の row-renumber invalidation を維持する。
`Selection` の値は domain に置けるが「どの行を見ているか」は tab が所有し、operation の対象には使い続けない。
linked panel の commit/amend/discard は `WriteOrigin::CommitPanel`、editor tree は
`WriteOrigin::EditorTree` の既存分岐を保存し、解決した `WorktreeId` を request に凍結する。
Cmd+Z の foreign-panel スキップは #476 の規約を維持し、cross-worktree undo は今回追加しない。

## 3. 操作ライフサイクル

application の状態は Backend の安全処理を「実行する別の controller」ではなく、その進行の投影。
preflight/verify を application callback に分割して差し込まない。

```text
Draft(revision) → Planning → Ready(plan, owner, revision) → Approved
                     ↓失敗       ↓入力変更/離脱             ↓admission
                  PlanError     Invalidated              Running
                                                        ├ trust/preflight
                                                        ├ snapshot/execute
                                                        ├ verify
                                                        └ record → Settled → read invalidation → presentation
```

| 事象 | 状態遷移・規則 | 記録 / lease |
|---|---|---|
| 入力変更 | revision++、旧 Ready/Approved を即時失効。Planning/PlanError は実行不能 | keypress ごとに mutation entry は作らない。現行 request の plan/open 失敗は boundary が失敗記録を返し、GUI は error modal とログに提示 |
| replan 後着 | owner/incarnation/revision が一致した結果だけ Ready にする | 旧 plan を参考表示しても execute token を持たせない（#510） |
| approve | plan digest・resolved request・対象・policy revision・承認主体を束縛 | 二段階確認を飛ばせない。UI は private な Approved 値を直接構築しない |
| confirm 再送 / busy | 受理済み token は再消費不可。競合は Busy、未dispatchなら再試行前に再検証 | 他 operation の lease を消さない。Busy は mutation entry ではない |
| admission 成功 | 所属と revision を確認し token を消費、lease と job を同時に確立 | spawn 失敗も terminal outcome。中途半端な busy を残さない |
| trust/config/HEAD/対象 drift | Backend が Refused/PreflightFailed を返す。必要なら新 plan を要求 | 受理した試行は mutation 境界で記録、lease 解放 |
| 成功 / 部分成功 / verify 失敗 | Settled。実行結果と確認結果を分離。verify 失敗を「未実行」にしない | 復元材料を含め一度記録。affected read を stale にする |
| 実行例外 / panic | unwind は mutation 境界で捕捉。未変更と証明不能なら `Unknown` | 使用 handle を再利用しない。停止が確実なら lease 解放、要照会状態は残す |
| タブ切替 / close | Draft/Ready の承認は失効、Running は継続 | 記録と lease は tab と無関係。表示配送は §4 |
| timeout / channel loss | timeout は実行停止の証拠ではない。Cancelling/Unknown の区別 | 生存不明 job の lease を時間だけで解放しない。自動 mutation 再送禁止 |
| 最後の window close | Welcome への tab close と process quit は別。host が完了を drain | Quit は進行中確認・待機。強制 kill/電源断の完了記録は保証しない |

```rust,ignore
enum PlanState<P> {
    Draft { revision: InputRevision },
    Planning { request: RequestId },
    Ready { token: PlanToken, plan: P },
    Error { revision: InputRevision, error: PlanError },
}
// PlanToken/Approved のフィールドと constructor は非公開。
// token は UI の表示文字列ではなく resolved request 全体へ結び付ける。
struct Approved<P> { owner: Target, revision: InputRevision, plan: P, policy: ExecutionPolicy }
enum LoadState<T> {
    Idle,
    Loading { request: ReadRequest },
    Ready { request: ReadRequest, value: T },
    Failed { request: ReadRequest, error: ReadError },
}
```

GUI の Enter/ボタン/command は同じ approve/dispatch に入る。`ActiveModal` の網羅的 match で
「処理した」「実行不可だが入力を消費した」を区別し、背面 checkout への fall-through を禁止（#492/#493）。
CLI/MCP も token を実行開始前に消費する。失敗後も同じ token で再実行せず、新規 plan を取得する。

## 4. 非同期配送 — 捨ててよいのは表示だけ

| 境界 / 宛先 | 契約 |
|---|---|
| mutation → recording | Backend/transport が success/partial/refusal/error と recovery を確定し append を試みる。UI 到着前に終了 |
| Backend open 前の失敗 | execution boundary の factory が凍結済み target/actor/OperationId で記録。Backend が存在しなくても UI writer へ戻さない |
| job → host | window の弱参照ではなく host-owned mailbox へ完了を送る。operation map は receipt を受理し、該当 lease だけ解放 |
| active owner tab | 既記録結果の toast/footer/oplog と必要な error modal。snapshot 更新は別 read request |
| inactive owner session | read を stale、receipt を owner 通知として保持。現在の別 repo の footer/modal/selection は変更しない |
| owner tab が close 済み | closed tab に callback は送らない。pinned operation を終端化して軽量な通知を host に残し、session を解放 |
| Welcome | repo 名付き完了/失敗通知と既記録ログへの導線のみ。閉じた repo を暗黙に reopen しない |
| 同じ path を reopen | 新 TabId/incarnation へ旧 modal/selection を適用しない。identity が確認できれば read を新しく取得、ログは明示照会 |
| 同じ completion を再配送 | OperationId で既受理なら no-op。UI ring-buffer にも重複追加しない |

表示の破棄条件は `(TabId, attachment generation, WorktreeId, RequestId)` の不一致。
tab switch の global generation を理由に **operation の終端化**を破棄しない。
read completion の stale 判定も application に集約し、pane は request stamp を seed/event で受け渡す。

記録の「必ず」は **すべての受理した試行が recording boundary に到達する設計**の意味。
現行 JSONL は disk-full・process kill に対する exactly-once durable transaction ではない。
append 失敗を successful receipt にしない。panic 直前の副作用を完全復元するには別の journal が必要であり、
本件の initial slice は通常結果・捕捉可能な unwind・UI 消滅の境界を対象とする。
停止確認済み Unknown は busy lease を解放しても、対象を `NeedsReconcile` として新 mutation を拒否する。
対象の実状態を再照会し、必要な復旧/新 plan を明示承認するまで通常状態には戻さない。

## 5. GUI / CLI / MCP の共通実行契約

### 5.1 resolver と policy

| 値 / 判定 | 共通の owner / 方針 |
|---|---|
| request 解決 (#509) | 文字列→typed request の純粋検証と、Backend による HEAD/OID/path 解決を分離。既存 `Operation` を使う。CLI/MCP の二つの match を一つにする |
| plan wire DTO | Git crate の GPUI 非依存 facade に一つ。既存 JSON field/意味を維持し、domain に serde を追加しない |
| actor | adapter identity から `Human/Cli/Mcp`。tool 引数で任意 actor に偽装させない |
| auto_snapshot | GUI は既存 Settings を host が読む。CLI/MCP は当面明示 policy（default on）、GUI 設定ファイルを暗黙に読まない |
| policy の凍結 | plan 時に適用値を提示し approval に束縛。承認後に安全関連設定が変われば再plan。worker の起動時 default に依存しない |
| trust | 評価は Backend/transport が **実行直前**に対象について行う。plan 時の評価は表示用。長寿命 Backend も再評価。application が trusted bool を権限として与えない |
| worktree config trust | repo owner trust とは別。表示した config SHA への明示承認だけ許可。grant 後も実行直前に digest を再検証 |
| 対象 worktree | `WriteOrigin`/argv/server scope から解決済み Target。実行時の active tab から引き直さない |
| verify | family の実状態確認は Backend の記録前。GUI reload 成功を verify 代わりにしない |

policy の値の組立は application/host、**適用と強制は Backend/transport**。
既存 `run`/専用 run を唯一の mutation owner に保つ。generic middleware の別安全 pipeline は作らない。
#509 の共有部分は既存 CLI/MCP が共に依存する `kagi-git` に置けるため、新 app crate の前提にはしない。

### 5.2 実行結果と記録 receipt (#505)

```rust,ignore
struct ExecutionReport<T> {
    operation_id: OperationId,
    target: Target,                     // 実行元 + 実際の対象 worktree
    result: MutationResult<T>,
    verification: Verification,         // Verified / Failed / NotRun / Unknown
    recovery: Vec<RecoveryHandle>,      // OID は省略しない
    recording: Recording,
}
enum MutationResult<T> {
    Success(T),
    Partial { value: T, error: ExecutionError },
    Refused(Refusal),
    Failed { stage: Stage, error: ExecutionError },
    Unknown { stage: Stage, evidence: Evidence },
}
enum Recording {
    Appended(RecordReceipt),             // append が確定した実 entry を含む
    Failed { attempted: OpLogEntry, error: RecordError },
    Exempt(RecordingExemption),         // §5.3 の明示例外だけ
}
struct RecordReceipt { operation_id: OperationId, entry: OpLogEntry, log_path: PathBuf }
```

現行 `append_oplog` は `PathBuf` だけ返し、`record_run_oplog` はエラーを捨てる。
変更時は **採番・append を行ったその関数が実 entry を返す**。呼出後の tail 読み直しは禁止。
receipt の OperationId と既存 `u64 id/parent` は別物。multi-process で id が重複し得る問題を
receipt の導入だけで解決したと主張しない。OperationId の永続 encoding/採番は #505 の実装 ADR で確定する。

`Success + Recording::Failed` は「変更済み・記録失敗」。CLI は区別できる非ゼロ終了、MCP は
構造化 envelope に変更済みを残し、GUI は再実行を促さない error modal を表示する。
記録の再試行を将来提供する場合も record-only に限定し、mutation を再送しない。
確認応答は成功時だけでなく refusal/partial/record failure も report を失わない。

### 5.3 public mutation の安全要件表

**以下は移行先の契約。現状すべて満たすとの主張ではない。**
T=実行時 trust、P=plan/承認、F=対象別 preflight、V=実状態 verify、S=復元手段、R=記録。
`run` に入る全 variant を列挙し、それ以外の facade/transport/FS 公開入口も下表で分類する。
`ops::*` の生 executor は製品の別公開契約にせず、caller 移行後に crate 内へ狭める。

| public family / 現行 API | T | P | F | V | S | R |
|---|---|---|---|---|---|---|
| `Commit`, `MergeCommit`, `Amend`, `UndoCommit`, `CherryPick`, `Revert`, `MergeBranch`, `MergeIntoConflict`, `MergeIntoBranch`, `ResetCurrentToHead`, `RebaseCurrentOnto` | 必須 | 必須 | HEAD/index/入力/対象 refs | commit/refs/status/conflict | 現行 auto_snapshot 判定 + family recovery | report 一件 |
| `Checkout`, `CheckoutCommit`, `CheckoutTrackingBranch`, `SwitchToLatestBranch`, `CreateBranchWithCheckout` | 必須 | 必須 | HEAD/dirty/対象 refs | HEAD/branch/status | 現行 auto 判定 | 一件、複合 step の partial 含む |
| `CreateBranch`, `RenameBranch`, `DeleteBranch`, `CreateTag`, `SetUpstream` | 必須 | 必須 | name/OID/使用中 worktree/upstream | ref/config | full ref OID、現行 auto 判定 | 一件 |
| `Pull`, `Push`, `PullBranchFf`, `PushBranch`, `PushTag`, `DeleteRemoteBranch`, `ForceWithLeasePush` | 必須 | 必須 | remote/ref/lease、local state | local/remote 観測。不明は Unknown | remote OID 等。WT snapshot は remote rollback ではない | 一件 |
| `StashPush`, `StashApply`, `StashPop`, `StashDrop` | 必須 | 必須 | HEAD/list/OID。index のみで同定しない | stash/status/conflict | 現行 auto 判定。ただし Drop は stash 自身の full OID（auto 免除） | 一件 |
| `Discard`, `RestoreSnapshot`, `ApplySuggestion` | 必須 | 必須 | path/digest/anchor/対象 snapshot | 内容・subset/変更範囲 | discard backup 維持、restore は必須 savepoint（auto toggle 外）、suggestion は既存 recovery | 一件 |
| `CreateWorktree`, `OpenWorktreeForBranch` | 必須 + config 承認 | 必須 | path/branch/config SHA | 登録/path/step 結果 | 作成物・branch OID、既存 auto 判定 | 一件、post-create failure を保持 |
| `run_history_move`、生 `execute_undo/redo` | 必須 | 必須 | branch/from/to OID | refs。index/WT 不変 | full from/to、WT snapshot 不要（ref-only） | 既存 owner 再利用 |
| `execute_remove_worktree` | 管理元 + 対象 + config | 必須 | 登録/dirty/lock/containment/config SHA | 削除/admin/optional branch | 対象 ODB backup + branch full OID。管理元 WT の auto snapshot で代用しない | Backend へ移す |
| `execute_lock/unlock/prune/repair_worktrees` | 必須 | 必須 | admin identity/状態 | 登録・lock 状態 | metadata before、WT 非変更のため snapshot 免除 | Backend へ移す |
| `execute_conflict_continue/save/abort/skip`、`execute_stash_conflict_abort`、`stage_conflict_resolution`、`execute_dir_file_resolution` | 必須 | continue/abort/skip は plan、save は明示編集承認 | conflict/index/buffer revision、path | index/session/内容 | 既存 conflict autosave/ODB/ORIG_HEAD。WT 破棄は保護必須 | 各 boundary の既存 writer を一つに集約 |
| `execute_delete_merged_branches` | 必須 | 必須 | 各 local/remote tip | 両側の結果を個別確認 | full deleted tips | 既存 Backend owner。partial を保持 |
| `execute_absorb` | 必須（現状欠落） | AbsorbPlan 必須 | distribution/HEAD/index | `verify_absorb` を完了条件にする | destructive 用 auto snapshot を適用 | 既存 recorder + receipt |
| `stage_file(s)`, `unstage_file(s)` | 必須 | 明示クリック/要求。modal plan 免除 | worktree/path/index/content 条件 | index の対象差分 | WT/refs 非変更なので自動 snapshot 免除 | index-only 例外として durable oplog 免除、失敗 report は返す |
| `fetch_remote`, `fetch_remote_branch` | 必須 | 明示操作/auto-fetch policy。modal 免除 | remote/refspec | 更新 refs | WT 非変更、自動 snapshot 免除 | fetch boundary に一件（auto は actor/source 明示） |
| `create_snapshot` | 必須 | 明示要求、plan modal 免除（ADR-0154） | 対象/保存 ID | ref/tree | 自身が復元点。再帰 snapshot 禁止 | 単独要求は一件、auto 子処理は親 report に束縛 |
| `delete_snapshot`, `prune_snapshots` | 必須（delete は現状欠落） | delete は対象確認、prune は cap policy の承認 | ID/ref/cap | 残存 ref | snapshot 再作成しない（保持上限が無効になる） | 単独一件 / auto は親に削除 ID を含める |
| `trust_repo`, `trust_worktree_config_for_worktree` | 未trusted状態からの grant を許す特別入口 | 明示 identity/config SHA 承認 | path/owner/SHA 再検証 | trust store | WT snapshot 対象外 | grant boundary で監査。mutation 成功と同一視しない |
| `github::merge_pr` | transport policy | PR/head/method の承認 | PR state/head/ruleset | remote merge state | merge OID。ローカル snapshot は免除 | transport へ移す。timeout は Unknown |
| `github_merge::enqueue_pr/dequeue_pr`（公開 API、production caller なし） | transport policy | PR node ID/queue 操作を明示承認 | node/repo/queue 状態 | queue membership | queue の before 状態。WT snapshot 免除 | transport に一件。Git ref mutation とは別結果 |
| `remote_stash_drop`, `remote_pull` | remote 側 gate | request/remote plan の承認 | remote HEAD/stash OID 等 | remote 状態 | stash OID/remote recovery | transport owner、drop の既存記録を維持 |
| editor save/overwrite・create/rename/trash/gitignore | FS capability の対象制限（Git owner trust の代用ではない） | 保存は直接意思、overwrite/trash は明示確認 | containment/.git 禁止、file/buffer revision、衝突 | 保存 bytes/移動先 | buffer/Trash。自動 Git snapshot 免除 | 通常保存は oplog 免除（ADR-0120）、他 FS も Git oplog 外の明示例外。typed FS report/通知 |

例外は enum/契約として明示し、`destructive: bool` 一つから全 gate を推論しない。
index/FS の例外でも admission・失敗通知・対象固定は免除しない。
config/draft の保存、worktree port 割当、conflict autosave はそれぞれ所属 service の metadata write。
worktree steps/port 割当は親 lifecycle report に帰属し、独立した未記録の Git mutation にしない。
terminal は自由な外部 shell であり Kagi の承認済み mutation API ではない。terminal-start 記録は
host の起動監査として残し、その中の任意コマンドを本契約が保護すると主張しない。

現状の追加注意: `Backend::run` の `CreateBranchWithCheckout` arm の `?` は末尾記録を迂回し得る。
また `blocking_ops` の verify は run の記録後に実行される。共通化時は family の実行結果を
内側で捕捉し、verify/partial も含めて外側 recorder に一度到達させる。receipt wrapper を
既存 run の外に足して二重記録する方式にはしない。

## 6. 既存実装への接続と GPUI 非依存境界

| 現行要素 | 再利用するもの | 新設 / 撤去するもの |
|---|---|---|
| `Backend` / `ops/*` | plan/preflight/execute triples、安全判定、実 verify、recording helper | family report/receipt を既存実行入口へ追加。生 executor の外部 caller を移して公開縮小 |
| `git::RepoSession`（ADR-0107） | foreground read handle (`Rc<Backend>`) | application `WorktreeSession` の subordinate resource にする。session map で保持し tab return の再openをなくす。`Rc` を job に送らない |
| `RepoWorker` | 将来の worker 候補と既存テスト | 初回は配線しない。production caller ゼロを「全 mutation 配線済み」と書き換えない |
| `finish_op_on_main` / `reject_if_busy` | 既存 panic/stale/busy の regression intent と klog 契約 | 移行 family は app admission/apply + 共通 GUI bridge へ。旧 helper は未移行 family だけ、最後に削除 |
| `blocking_ops.rs` | 同期実行 core、family 別 verify、対象 path 引数 | Settings/i18n/modal 型依存を除去。policy は引数、verify は Backend、表示整形は adapter。32 caller は #314 の歴史値で今回の再計測値ではない |
| `record_op` / `record_op_persist` | toast/footer/panel の表示 | migrated family は receipt 表示。Refused の隠れ append も通さない。全移行後 persist フラグと UI writer を削除 |
| `WriteOrigin` / `write_repo_for` | #476 の panel と editor の区別 | GUI が Target を作る一箇所へ接続。dispatch 後の repo_path 再読禁止 |
| CLI/MCP | argv/tool envelope/二段階確認 | 共通 resolver/DTO、report 返却。global tail read と同期用別安全経路を削除 |

### 6.1 初期配置と job/host 契約

```text
kagi(bin / host)
  ├ ui (GPUI、pane Entity、承認 UX)
  ├ app (GPUI/git2 なし、sessions + family orchestration)
  │   ├ kagi-git (実行、安全、記録、I/O)
  │   └ kagi-domain (純粋値/判定)
  └ transport / FS capability (I/O、GPUI なし)
```

root `src/app/{session,worktree,...}` から始める。app は **直接の filesystem/process/network I/O を禁止**、
ただし job 内で `kagi-git`/typed capability を呼ぶ間接 I/O は許す。純粋 reducer は domain に置く。
task runtime、GPUI Context/Task/Entity、git2 Repository、settings 読込は app に入れない。
module 境界は Cargo では強制できないので、実証 PR で `ci` の uv Rule により禁止依存を検査する。

```rust,ignore
// family ごとの owned request。Backend::open は job 内の execution factory。
fn prepare_remove(session: &mut Sessions, approved: Approved<RemovePlan>)
    -> Result<RemoveJob, AdmissionError>;
// app の job は GPUI を知らず、Send な値だけを保持する。
impl RemoveJob {
    fn run(self) -> Completion<RemoveOutcome>; // 同期本体、boundary が記録済み
}
fn apply(sessions: &mut Sessions, completion: Completion<RemoveOutcome>)
    -> Vec<Delivery>; // 所属・lease・重複配送判定。append はしない
```

GUI host の **共通 bridge 一箇所**だけが job を background spawn して mailbox に送り、
main-thread で `apply` する。task の join observer は window の `WeakEntity<KagiApp>` に所有させない。
spawn 拒否・未実行 job drop も OperationId 付き terminal event にして lease を解放する。
CLI は同じ job を inline 実行して同じ report を使う。テストは手動 job runner と mailbox で順序を制御する。
初回から汎用 executor trait/万能 closure registry は作らない。閉じた family job enum/owned struct で十分か実証する。

MCP が root を依存すると GPUI を引くため、それは不可。共通 backend resolver/policy/report は
当面 `kagi-git` 側で消費する。MCP が session/job 自体を必要とする段階で **その実 consumer と同時に**
GPUI-free app crate を抽出する。source 二重 include/コピーで擬似共有しない。

### 6.2 worker を採用する条件（#314）

| 条件 | 必要な設計 / 検証 |
|---|---|
| policy parity | 毎 request の actor/auto_snapshot/Target、実行直前 trust が direct job と一致。on/off fixture を比較 |
| liveness | worker generation + channel disconnect/JoinHandle 終了検出。確実に死んだ worker は次の新 request 用に respawn。timeout だけで respawn しない |
| panic | request 内 unwind を捕捉して report/record、handle を廃棄。死んだ worker の未実行 reply も終端化。副作用不明 request は自動再送しない |
| Drop | UI thread 上で join しない。shutdown signal と detach、host の明示 drain で正常終了を待つ。進行中 mutation は tab drop で打ち切らない |
| linked worktree | worker 一つ/タブだけでは shared refs の排他にならない。§2 admission は worker より外で維持 |
| 配線実証 | 本物の GUI mutation が worker thread に到達、panic 後の新 request 成功、close が非blocking、receipt 一件を fixture/E2E で assert |

ADR-0073 は「reopen を一行置換するだけ」「Drop で join」「一 tab が排他単位」を改訂する。
現実の待ち時間/handle 再利用の利益がなければ採用を再評価するが、#314 をこの docs PR で完了扱いにしない。

## 7. 全 mutation family の移行表

G = fixture integration + window 不要の進行/配送テスト。
E = native GUI E2E（`--features gui-e2e` と `KAGI_GUI_E2E=1`）。
M = primary session の実機確認（raw Enter/Esc/ボタン、応答性、対象表記）。
表の async は **GUI adapter の実行場所**であり、Backend core の API を async 化する意味ではない。

| family / 現行入口 | 移行先 | 同時に撤去する旧 glue | GUI 実行 / 検証 |
|---|---|---|---|
| checkout/commit checkout/tracking/switch-latest: `operations/checkout.rs`、`branch.rs` の start/confirm | app checkout → `Backend::run` | 同期 confirm、重複 busy/世代 guard、verify snapshot glue | async（checkout/fetch は可変長）。G/E/M、二段階確認 |
| commit/amend/undo-last: `operations/commit.rs`、`pull_push.rs` | app commit → run | Enter/ボタンの別実装、raw open policy、foreign target 再解決 | async（hooks/I/O）。G/E/M、linked panel #476 |
| branch create/rename/delete/upstream/tag: `operations/{branch,tag,remote_branch}.rs` | app refs → run | sync create/apply、per-family finish、checkout-after glue の二重経路 | async（local refs も I/O、checkout-after 複合）。G/E/M |
| merge/cherry-pick/revert/rebase/reset: `operations/{branch,cherry_revert,rebase,reset}.rs` | app history-change → run | `*_blocking` の policy/verify と個別 guard | async（history/status）。G/E/M、partial/conflict |
| pull/push/branch FF/force-lease: `operations/{pull_push,force_lease,branch}.rs` | app network → run | 同期 confirm、network 結果の文字列だけの変換 | async（network）。G offline remote、E/M、Unknown |
| stash push/apply/pop/drop: `operations/stash.rs` | app stash → run | sync apply/pop、drop の UI Refused writer | async（worktree 全体）。G/E/M、stash OID/競合後続 |
| discard: `operations/discard.rs`、editor tree menu | app discard → run | target/record/verify glue | async（file I/O）。G/E/M、Trash と混同しない |
| history undo/redo: `operations/history.rs::confirm_history` | app history → `run_history_move` | UI preflight/stack 更新の所属判定 | async 推奨（ref-only でも I/O）。G/E/M、index/WT 不変 |
| worktree create/open: `operations/worktree.rs`、`blocking_ops::create_worktree_blocking` | app worktree → run | steps/ports/trust の UI orchestration | async（command steps）。G/E/M、config SHA・失敗後作成物 |
| worktree remove: `confirm_remove_worktree` | app worktree → 専用 recorded run | UI persist/busy/on_done と main-thread trust write | async 維持。初回 G/E/M（§8） |
| worktree lock/unlock/prune/repair: 同ファイル confirm 群 | 同じ app family → 専用 backend boundary | sync UI executor/record | 記録所有を先に移し、当初 sync 可（短い admin 操作）。遅延測定で async。G/E/M |
| conflict save/DF/stage/continue/abort/skip: `operations/conflict.rs`、`conflict_view.rs` | app conflict → backend conflict boundary | UI writer、global pending index、独立 busy 判定 | 所有移管時は sync を維持可。save/続行の重い I/O は別 slice で async。G/E/M、編集 revision |
| cleanup: `branch_cleanup::confirm_branch_cleanup` | app cleanup → 既存 backend executor/recorder | finish glue（recorder 再実装はしない） | async 維持。G/E/M、remote/local partial |
| PR merge: `github::start_pr_merge` | app PR → `git::github::merge_pr` recorded transport | on_done 内 persist | async。offline transport G/E + M、remote head binding |
| PR queue: `github_merge::enqueue_pr/dequeue_pr`（API-only） | 同じ PR transport family | raw 公開 mutation の gate opt-in | 同期 core、将来 GUI は async。G、UI 新設は対象外 |
| SSH stash drop/pull: `operations/{stash,pull_push}.rs` → `remote/mod.rs` | app remote → typed remote execution boundary | UI/policy copies、pull の記録欠落経路 | async。offline G/E、認証付き M は別許可。未対応 gate は unsupported を明示 |
| staging: `do_stage/unstage_*` (`commit.rs`)、editor events | app index service → backend index-only | panel/tab path 取得の重複、同期別入口 | 小単位は sync 維持可。batch は async。G/E/M、file identity |
| fetch/auto-fetch: `commands.rs`、`remote_branch.rs::fetch_remote_branch_async` | app fetch → backend fetch boundary | busy を通らない dispatch、個別 refresh guard | async 維持。G/E/M、ticker admission |
| snapshot create/prune/delete/restore: `commands::create_snapshot_now`、Backend public API | app snapshot → backend snapshot boundary/run | create の latch 漏れ、別 recorder | async（tree capture）。G/E/M、API-only は G、restore savepoint |
| absorb: `Backend::execute_absorb`（GUI 導線なし） | 専用 recorded run + 将来 app request | 生 API の safety opt-in | 同期 core、将来 GUI は async。G、UI 新機能は対象外 |
| PR suggestion: `Backend::execute_apply_suggestion` / run（公開 API） | app suggestion → run | API bypass caller | 同期 core、GUI 接続時 async。G、anchor drift |
| editor create/rename/trash/gitignore: `operations/editor_fs.rs` / `editor_fs_ops.rs` | app file job + GPUI-free FS capability | UI 直 FS write、repo_path 再読 | 当初 sync 保持可、directory I/O は async 化を別確認。G/E/M、case rename/Trash |
| editor save/overwrite: `kagi-ui-editor::save_impl` | pane event → app file job → FS capability | pane 内直接 spawn/write、boolean loading | 既存 async 維持。G/E/M、save 中再編集/close |
| trust grant/config grant | `trust_prompt.rs`、worktree confirm → host capability/boundary | per-operation inline trust write | 小 metadata は sync 可。G/E/M、明示承認・SHA drift |

sync を残す family に「同期だから記録・admission 不要」という例外はない。
上記 sync は所有移管と scheduling 変更を分離する暫定措置。遅い fixture を使い main-thread stall を測り、
無制限 I/O がある経路は async に移す。#493 の既存 async ボタンに対する同期 Enter は直ちに撤去対象。
headless は同じ core の同期 consumer にし、既存 `[kagi]` 契約を保存する。read-only harness を理由に
古い mutation confirm を温存しない。

横展開の各 PR はこの表の一行（必要ならその一部）について、実 caller の移行と旧入口の削除を
同じ diff で示す。全 family が終わるまでは legacy と新 admission の接続が必要:
**初回は新 remove が既存 busy latch も共通 bridge 経由で占有し、legacy 実行中の新 dispatch も拒否する**。
残存 legacy は保守的に全 repo を塞ぐ。新旧別 mutex にして並行 mutation を許さない。
per-RepoId の並行許可は全 write 入口（staging/auto-fetch/FS を含む）の協調が揃った後に有効化する。
現行 `do_stage_all`/`create_snapshot_now` は busy gate を通らず、fetch は別 in-flight を持つ。
したがって既存ラッチを立てるだけでは十分ではない。初回実証の前提として共通 admission bridge に
これらの入口を接続し、進行中 fetch/editor save も把握する（実行本体の移設は不要）。
それが小さな slice に収まらなければ admission の先行 PR を切る。未配線のまま remove の排他完成を宣言しない。

## 8. 最初の縦断実証: worktree remove

| 候補 | 得られる実証 | 初回としての評価 |
|---|---|---|
| stash-drop | recording 済みなので app seam の費用が低い | 残る non-run writer と複数 worktree 所有を検証しにくい |
| PR merge | transport/Unknown を検証できる | 認証・remote simulation が主題を増やす |
| worktree remove | 管理 repo と対象 worktree の分離、長い job、backup/partial、close race、既存 UI writer | **採用案**。local fixture と既存 containment/steps テストがある |

### 8.1 最小 cut

| 変更点（後続実証 PR のみ） | 完了条件 |
|---|---|
| identity / operation registry の最小 slice | 管理元 repo、対象 worktree、origin tab、input revision、lease を window なしで保持 |
| 専用 remove execution boundary | open failure/trust/config grant/preflight/既存 execute/verify/record を一箇所へ。`DiscardOutcome` の backup/partial を再利用 |
| report/receipt | append 成否を結果へ。管理元 repo + 削除対象 worktree を区別して記録。path 削除後も照会可能 |
| GUI adapter | ボタン/Enter → 同一 approval/bridge。UI の remove 用 writer と個別 busy/世代 guard を削除 |
| legacy interoperability | §7 の互換 admission を共通 bridge で検証。staging/snapshot/fetch/editor save の bypass を先行解消。新規 family の UI がラッチを手でコピーしない |

**含めない**: worker 配線、全 snapshot/cache の移管、全 modal の書換え、CLI/MCP の remove tool 新設。
同じ remove request/policy を window 無しの consumer で実行し、GUI と同じ report を得ることを示す。
CLI/MCP の既存 operation resolver/receipt 消費は #509/#505 の後続 slice で本物の両 consumer を移す。
この一 family の成功だけで #484 全体を close しない。

### 8.2 テスト設計（今回は実行しない）

各 test は tempdir の main A / linked L / 独立 B と専用 log dir。sleep に依存せず、
preflight 前・mutation 後/配送前の barrier または手動 job runner で順序を固定する。

| test | 操作 / fault | assert（実状態と配送） |
|---|---|---|
| normal | clean L の remove、branch delete on/off | L/admin の消失、branch の期待状態、receipt 一件、actor/管理元/対象一致、busy 解放 |
| refused | dirty/locked/main/対象 missing、trust 拒否 | path/refs 不変、typed refusal/failed stage、実行受理済み試行は記録一件 |
| revision | plan A→入力変更→replan failure、旧完了後着 | Enter/ボタン両方不可。再plan成功後の最新対象だけ実行 |
| preflight drift | plan 後に L dirty/lock/config SHA/登録 identity 変更 | 不承認の hook/削除が走らない。旧 plan の内容で別 worktree を消さない |
| open failure | plan 後に管理元を fixture 内で一時的に利用不能にする | Backend がなくても failed report/receipt。一度。B の表示は不変 |
| tab switch | A で開始、B に切替してから completion を送る | A/L の結果記録一件、B の modal/footer/selection 不変、A session stale |
| close / Welcome | 実行中の owner tab を閉じる / 最後の tab を閉じる | 実行・記録は完了、閉じた Entity 更新なし、host に対象付き通知、pin 解放 |
| background close / reselect | A 実行中に B close、A 再選択 | A generation/selection/進行を再初期化せず A に完了が届く（#488） |
| duplicate / concurrency | double confirm、重複 completion、sibling mutation、legacy mutation | 同じ token の実行は一度、lease owner のみ解放。新旧相互排他 |
| partial | backup 後、削除/admin prune/branch delete の途中に fault | partial の backup full OID を receipt と JSONL に保持。fixture bytes を ODB から復元して一致 |
| panic | mutation 前/後の捕捉可能な unwind を注入 | before は Failed、after は Unknown/partial evidence。busy が固着せず再実行を勧めない。停止未確認 timeout は別テストで lease 保持 |
| append failure | log writer を失敗させる | mutation 成功と記録失敗を同時に返す。過去 tail を返さず、retry で削除を再実行しない |
| receipt race | append 後/応答前に別 repo・同 repo 同 op の entry を追加 | 応答 entry は最初の実行そのもの。global tail とは異なっても正しい |
| read supersede | mutation 前 read→mutation 完了→新 read→旧 read 後着 | 旧 snapshot が fresh を上書きせず、loading は自身の request だけ終端化 |

既存 `tests/{worktree,git2_owner_trust,oplog_nonrun_ops}_test.rs` と
`tests/gui_e2e_runner.rs` の worktree/recovery scenario は残す。既存 assertions を弱めない。
新 G テストは UI method を呼ばず session/job/fixture の事実を assert する。
E は real modal/focus tree を描いた後の raw Enter、ボタン、tab close を使い実行境界への到達を確認する。
M は確認対象 worktree 表示、長い pre_remove 中の応答性、完了/拒否/部分成功通知を実アプリで確認しスクリーンショットを残す。
native runner は scenario PASS だけでなく **正常な終了 status** を確認する（既知 teardown 問題を成功扱いしない）。

**横展開 gate**: G/E/M を通過し、remove の UI writer/旧 executor glue が消え、window 無しでも
progress/owner が検証できること。別 family を足す際に UI 側で busy/世代 guard を再作成する必要が
あれば境界は未成立として設計へ戻る。

## 9. ADR と既存 Issue の扱い

| 文書 | 維持する内容 | 改訂する内容 / 時期 |
|---|---|---|
| ADR-0073（2 文書） | Backend pipeline、worker という候補 | 未配線の現実、job/host と worker の分離、liveness/panic/Drop。#314 実証 ADR で置換範囲を指定 |
| ADR-0075 | session 単独所有・active 参照という目的 | tab=session、app に GPUI、controller が記録という前提を本設計へ。最初の所有実証 ADR で部分 supersede |
| ADR-0104 enforced pipeline | Backend に安全 gate を置く、旧 bypass を消す | 「全 mutation が run 済み」は現状と不一致。専用 run/例外表・verify 所有を family ごとに追記 |
| ADR-0107 | read Backend handle の再利用 | `RepoSession` は application session ではない。単一 active slot→map 配下への寿命変更時に追記 |
| ADR-0121 | feature-pane Entity、seed/event、境界安定後の crate 分離 | bin に残した進行/記録 glue の app/backend 移管だけ追記。pane 分離は巻き戻さない |
| ADR-0149 + recovery | mutation-owned recording、actor/worktree/full recovery | receipt、open failure、non-run 残存・Refused writer の移行。#501/#505 実証で追記 |
| ADR-0120 / ADR-0154 | editor 保存の Git pipeline 例外、snapshot の明示/自動区別 | 本文の FS/index/metadata 例外を正式化する際に整合を取る。暗黙に既存 UX を変えない |

本書は ADR ではなく、#484 の議論用設計。合意後に PM が Issue の Plan に要約する。
実証 PR で **実際に採用した境界だけ**新 ADR にし、上表の古い記述へ相互リンク・superseded 範囲を追記する。
本 docs PR は既存 ADR の Accepted 状態を変更しない。

| PM にレビューいただきたい点 | 推奨 / トレードオフ |
|---|---|
| common-dir 単位の初期排他 | linked worktree の並行性より安全・簡潔さを優先。per-worktree 緩和は後続 |
| app root module + backend 共通契約 | 先に crate を作らず、MCP が job を必要とするときに抽出。安全/request 契約の共有は先行可能 |
| GUI close 後の host-owned 通知 | 他 repo の modal に混ぜず Welcome/owner 通知へ。process quit の待機 UX は実証で確認 |
| sync/index/FS の例外 | 記録所有移管と scheduling/UX 変更を分離。全 mutation に modal を強制しない |
| 初回の成果範囲 | remove + 最小 operation registry + receipt + legacy admission bridge。全 session 移管や worker を抱き合わせない |

## 10. 読解根拠・関連入力

- [中心 Issue #484](https://github.com/TomiXRM/kagi/issues/484)。state/lifetime:
  [#482](https://github.com/TomiXRM/kagi/issues/482)、[#485](https://github.com/TomiXRM/kagi/issues/485)、
  [#488](https://github.com/TomiXRM/kagi/issues/488)、[#489](https://github.com/TomiXRM/kagi/issues/489)。
- execution/contract: [#494](https://github.com/TomiXRM/kagi/issues/494)、
  [#501](https://github.com/TomiXRM/kagi/issues/501)、[#502](https://github.com/TomiXRM/kagi/issues/502)、
  [#505](https://github.com/TomiXRM/kagi/issues/505)、[#509](https://github.com/TomiXRM/kagi/issues/509)、
  [#314](https://github.com/TomiXRM/kagi/issues/314)。
- UI entry: [#492](https://github.com/TomiXRM/kagi/issues/492)、
  [#493](https://github.com/TomiXRM/kagi/issues/493)、[#510](https://github.com/TomiXRM/kagi/issues/510)。
- [architecture](../architecture.md)、[migration/S5 deferred](../migration/README.md)、
  [recovery AUDIT](../../agent-loop/2026-09-astra-recovery/AUDIT.md)、
  [METRICS](../../agent-loop/2026-09-astra-recovery/METRICS.md)。その歴史的計測値を本 PR の計測と呼ばない。
- [ADR-0073 pipeline](../../adr/0073-git-backend-trait-operation-pipeline.md)、
  [ADR-0073 worker](../../adr/0073-repo-worker-thread.md)、
  [ADR-0075](../../adr/0075-appstate-reposession-operationcontroller.md)、
  [ADR-0104 pipeline](../../adr/0104-enforced-operation-pipeline.md)、
  [ADR-0107](../../adr/0107-repo-session.md)、[ADR-0121](../../adr/0121-zed-informed-modularization.md)、
  [ADR-0149](../../adr/0149-oplog-in-backend-run-and-schema.md)。同番号の swimlane ADR-0104 は本件対象外。
- PM の一時レビュー `fable-safety-design.md`（2026-09-06、recovery 時点）を参照。
  永続的な根拠は上記 recovery 文書と現行ソース。stash-drop/history/cleanup の修正済み境界は再実装しない。

確認方法: 上記 Issue 本文と現行ソースの静的読解のみ。
この設計 PR では cargo、fixture mutation、GUI E2E、性能測定を実行しない。
