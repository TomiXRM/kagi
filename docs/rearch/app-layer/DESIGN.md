# #484 アプリケーション層 — 所有・実行・配送の設計案

状態: **レビュー用 Draft r2 / 最終合意待ち・未実装**。2026-09-06。PM Round 1・omp-plan R1〜R5・PM Round 1.5 裁定の反映版。
読解基準: `origin/dev` = `85b0159a6d9455632db1c6cf758459d1b6e9558d`。
この PR は文書だけ。コード・ビルド・実測は含まない。

進め方は **設計レビュー → 1 family の縦断実証 → 横展開**。
目的はフロントエンドを交換しても安全契約と操作の所属が変わらないこと。
巨大な `KagiApp` を巨大な controller に移すことや、新 crate の作成自体は成果に数えない。

## 0. 今回決める案と、まだ決めないこと

| 項目 | 本文の推奨案（PM 合意待ち） | 実証・後続で決めること |
|---|---|---|
| 所有境界 | application は進行・所属・配送、Backend/transport は安全判定・mutation・verify・記録 | family ごとの最小 API 名と内部配置 |
| session | slice 1 は `KagiApp` 内の最小 `Sessions` が operation/lease/stale mark だけ所有 | #482/#488 で session map、`TabId`、incarnation、snapshot 共有を導入 |
| 排他 | slice 1a は remove lease + `busy_op`、1b は editor save/staging/snapshot/fetch の admission 接続。横展開には両方必須 | 全移行後は common Git directory 単位。index-only の並行化はさらに後 |
| 実行 | GPUI 非依存の owned job と完了メッセージ。spawn は adapter host | worker 導入は #314 の独立実証後 |
| 記録 | mutation 境界からその実行の receipt を返す。UI は append しない | multi-process 採番・crash recovery journal は別設計 |
| 配置 | root `src/app/` の小さな session/family module から | 本物の第 2 consumer が必要になった時点の crate 抽出 |
| 第 1 family | worktree remove（#501） | 実証で境界が成立しなければ横展開しない |

`architecture.md` の古い「app に GPUI」「controller が oplog を書く」図を
実装済みの事実として継承しない。S5 deferred と ADR-0121 の pane 分離は維持する。
**§1〜7 は全体の到達設計を含む。slice 1 の実装範囲は §8 の列挙だけ**。
後続の session/worker/全 mutation executor 移管を slice 1 の前提にしない。1a/1b の範囲と gate は §8。

## 1. 責務所有表

パスはリポジトリルートから。提案型はすべて仮称。

| 層 | 所有するもの | 所有しないもの | 現行コードでの担当箇所・差分 |
|---|---|---|---|
| domain | `Operation`/outcome、identity の値、承認・進行の純粋な判定、conflict FSM | git2、GPUI、serde 依存、I/O、thread | `crates/kagi-domain/src/{operation,plan,resolution,history}.rs`。既存型を再利用。`OperationPlan` 自体は今は Git crate |
| application session | `RepoId`/`WorktreeId` と寿命、read state/cache、request revision、進行中 operation、follow-up の所属 | focus/scroll/Entity、Git primitive、永続記録の writer | 現在は `ui/{mod,tabs,reload,tab_view,worktree_wip}.rs` に分散。既存 `git::RepoSession` はこれではない |
| application family service | request→plan job、approval の束縛、admission、job と結果の対応、対象別 invalidation | trust の実装、preflight/verify の再実装、表示文言 | `ui/operations/*`、`blocking_ops.rs` の orchestration 部分。family ごとに小さく移管 |
| Git backend / execution boundary | repository 解決、trust 再評価、policy 適用、plan/preflight/execute/verify、復元材料、記録 receipt | active tab、modal、toast、window 寿命 | `git::Backend::{plan,run,run_history_move}`、`backend/recording.rs`、`ops/*`。non-run の欠落をここで閉じる |
| Git worker（後続） | Backend handle の thread 所有、直列実行、liveness、完了保証 | 承認 UX、session map、再実行の可否を推測すること | `git::{session,worker}.rs`。production submit caller は未配線。初回は使わない |
| GUI adapter / host | input、`WriteOrigin` 解決、承認表示、GPUI Task の実行・main-thread 復帰、表示、通知 | per-handler busy/世代 guard、記録、安全 policy のコピー | slice 1 は **既存 `KagiApp` が host**。`ui/operations/mod.rs` に共通 `dispatch_job`。最後の tab close 後も Welcome として生存 |
| CLI adapter | argv、明示承認、stdout/stderr、exit code | 独自 operation resolver/plan serializer、global oplog tail の推測 | `src/cli_main.rs`。共通契約の同期 consumer |
| MCP adapter | tool envelope、接続ごとの plan store、承認 token の消費、protocol error | 独自安全 gate、再plan失敗後の旧承認再利用 | `crates/kagi-mcp/src/write.rs`、server の plan store |
| transport / filesystem capability | PR/SSH の実行・結果照会・記録、editor の明示 FS 操作 | GPUI、active tab、安全な Git operation と偽ること | `git::{github,github_merge}`、`src/remote/mod.rs`、`ui/editor_fs_ops.rs` と editor 内 FS job。後二者は順次 UI 外へ |

## 2. 識別・排他・寿命

### 2.1 path は locator、tab は表示先、repo は排他資源

| 識別子 | 解決規約 | 寿命 / 再利用 | 用途 |
|---|---|---|---|
| `RepoId` | slice 1: Backend が解決した canonical common Git directory の値 | operation/軽量 owner record が参照中は保持 | linked worktree 間で共有する refs/ODB/admin 資源 |
| `WorktreeId` | slice 1: `RepoId` + canonical per-worktree Git directory（main も明示） | locator を凍結し、remove 承認は下記 admin fingerprint にも束縛 | HEAD/index/status/conflict、書込対象 |
| `TabId` / session incarnation | **slice 1 では導入しない**。#482/#488 で発行元・同一性検証を設計してから導入 | resource fingerprint とは別の session 世代 | 将来の表示・session 寿命 |
| `OperationId` | 実行試行を一意に区別する opaque ID | plan digest/entry の連番とは別。receipt と完了を束縛 | 二重配送防止、失敗の照会 |
| `RequestId` | slice 1 の plan は単調 request/revision。read の incarnation 付き ID は後続 | request 完了/置換で失効 | 将来 loading/data/error の一体所有 |
| remote identity | transport authority（接続先/user/port 等）+ remote 側の解決済み repo/worktree identity | alias 名だけで同一視しない。再接続は検証 | ローカル path canonicalization と混同しない |

Backend が canonicalize/discover する。UI の `canon(path)` の失敗時 raw-path fallback は
表示・過去ログ検索には残せるが、**新規 mutation の排他 identity には使わない**。
解決不能は refusal。削除予定の worktree の locator は dispatch 前に凍結し、完了時に再解決しない。
slice 1a の remove 承認は `<common>/worktrees/<name>` の **`(inode, st_birthtime)` +
`gitdir` ファイル内容 + config SHA + HEAD OID** を plan 時に凍結し、preflight で再照合する。
不一致/取得不能は Refused。path/name/OID/config が同じ再作成でも古い承認で削除しない。
これが今回の resource incarnation の出所であり、session 世代は #482 の別設計。
birthtime 等が得られない filesystem/platform は path-only に縮退せず拒否する。対応拡張時は
代替 identity と ABA fixture を別途合意する。任意の外部改変に対する cross-process lock ではない。

| 資源 | 初期の排他キー | 理由 / 限界 |
|---|---|---|
| local Git mutation（index-only、fetch、snapshot 含む） | 到達目標 `RepoId` | HEAD/index は別でも refs/stash/ODB/admin は共有。slice 1 は gate を通る旧操作とのみ相互排他（§7） |
| worktree lifecycle | 管理元 `RepoId`、対象 `WorktreeId` は payload | tab の repo と削除される worktree は違う。管理元から記録・verify できる |
| PR/SSH mutation | **後続**: transport authority + repository identity 候補 | 複数キー一括 admission/取得順序は slice 1 の設計・実装対象外。transport family で決める |
| editor FS write | 所属 worktree の `RepoId`（移行後） | Git mutation と同じファイルを書き得る。plan を省略することと排他を省略することは別 |
| read | write lease を保持しない。read revision で整合を管理 | 作業中の status は stale と表示。mutation 前の read が後着しても fresh に戻さない |

busy は `HashMap<ConflictKey, OperationId>` 相当の admission 状態。キュー UX は作らず
競合 request を Busy として拒否する。独立 repo の並行許可は全移行後。slice 1 は保守的な global busy も使用。
承認待ち中は lease を占有しない。
これは **同一 process の協調排他**であり、別 CLI process・外部 Git を止める保証ではない。
Git の lock、preflight、lease/OID 条件は Backend に残す。cross-process transaction lock は未合意。

### 2.2 状態と close

slice 1 は第二の global/actor/Workspace を新設しない。

```rust,ignore
// slice 1: KagiApp の一フィールドに保持。snapshot/tab_cache は入れない。
struct Sessions {
    operations: HashMap<OperationId, InFlight>,
    leases: HashMap<RepoId, OperationId>,
    stale: HashSet<WorktreeId>,
}
// KagiApp { app_sessions: Sessions, ...既存 active_view/tab_cache/busy_op... }
// completion と最小 owner 情報は operation に所属し、tab reset では消さない。
```

最後の **tab** close は Welcome へ遷移して KagiApp が残る。
KagiApp は window が生きている間だけの host。slice 1a は **実行中 lease がある間、window close と
Quit を共通入口で保留**し、既存 dirty guard と同様に「操作中」の modal を提示する。
native close/menu/shortcut/Quit の全経路を共通判定へ接続し、実行中に remove_window/cx.quit へ進めない。
完了・停止確認と lease 解放後に閉じられる。停止不明は保留を続ける。
tab close、window close、Quit は別々に実証する。Dock reopen は新しい KagiApp だが、その時点で旧 in-flight は無い。
window より長寿命の registry/drain/mailbox は後続。強制 kill/電源断の完了保証はない。

以下の full session/map/`TabView`/read 所有表は **後続 #482/#488/#489** の到達目標。
slice 1 は既存 snapshot/reload/cache をそのまま利用し、operation 情報の第三のコピーを作らない。

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
| replan 後着 | owner/request revision が一致した結果だけ Ready にする（incarnation 検証は後続） | 旧 plan を参考表示しても execute token を持たせない（#510） |
| approve | plan digest・resolved request・対象・policy revision・承認主体を束縛 | 二段階確認を飛ばせない。UI は private な Approved 値を直接構築しない |
| confirm 再送 / busy | 受理済み token は再消費不可。競合は Busy、未dispatchなら再試行前に再検証 | 他 operation の lease を消さない。Busy は mutation entry ではない |
| admission 成功 | 所属と revision を確認し token を消費、lease と job を同時に確立 | spawn 失敗も terminal outcome。中途半端な busy を残さない |
| trust/config/HEAD/対象 drift | Backend が Refused/PreflightFailed を返す。必要なら新 plan を要求 | 受理した試行は mutation 境界で記録、lease 解放 |
| 成功 / 部分成功 / verify 失敗 | Settled。実行結果と確認結果を分離。verify 失敗を「未実行」にしない | 復元材料を含め一度記録。affected read を stale にする |
| 実行例外 / panic | unwind は mutation 境界で捕捉。未変更と証明不能なら `Unknown` | 使用 handle を再利用しない。停止が確実なら lease 解放、要照会状態は残す |
| タブ切替 / close | Draft/Ready の承認は失効、Running は継続 | 記録と lease は tab と無関係。表示配送は §4 |
| timeout / channel loss | timeout は実行停止の証拠ではない。Cancelling/Unknown の区別 | 生存不明 job の lease を時間だけで解放しない。自動 mutation 再送禁止 |
| 最後の tab close / window close / Quit | tab close は Welcome、window close/Quit は実行中 lease があれば共通入口で保留 | 完了・停止確認後は閉じられる。強制 kill/電源断は保証外 |

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
| job → host | slice 1 は `KagiApp::dispatch_job` が OperationId ごとの completion を受け、**表示 guard より先に** `apply(&mut app_sessions, completion)`。global mailbox/別 actor は作らない |
| active owner tab | 既記録結果の toast/footer/oplog と必要な error modal。snapshot 更新は別 read request |
| inactive owner | slice 1 は Sessions に stale mark、repo 名付き toast/既記録ログへの導線。現在の別 repo の footer/modal/selection は変更しない |
| owner tab が close 済み | closed tab に callback は送らない。operation を終端化し KagiApp に対象付き通知。失敗/partial/record failure は既存 error modal への pending 表示要求も保持 |
| Welcome | repo 名付き通知と既記録ログ、失敗は対象明示の error modal。閉じた repo を暗黙に reopen しない |
| 同じ path を reopen | slice 1 は既存 switch_generation の不一致で旧 modal/selection を適用しない。新しい read と明示ログ照会。TabId/incarnation は後続 |
| 同じ completion を再配送 | OperationId で既受理なら no-op。UI ring-buffer にも重複追加しない |

slice 1 の GUI attachment は既存 **`(repo_path, switch_generation)`**。
Delivery は owner `RepoId/WorktreeId` と発行時 attachment を持つ。adapter の共通 bridge が
現在 attachment と一致する場合だけ footer/modal を反映する。**operation の終端化は常に先**。
背景 tab close で generation が変わる #488 自体は直さず、完了/記録は届き表示だけ落ちるところまで。
read completion の全面移管と request stamp 統一は #482/#489 で行う。
失敗の modal は他 repo の操作確認として開かない。KagiApp の既存 modal が空なら対象 repo を
明記して提示し、別 modal がある間は receipt/OperationId に束縛した表示要求を保留する。
repo を選び直す承認 modal の復活ではなく、既記録エラーの説明である。toast のみで失敗を終わらせない。

| `Delivery::Invalidate(WorktreeId)` の受取状態 | slice 1 の adapter 動作 |
|---|---|
| owner が現在 active | 既存 `reload(cx)` を呼ぶ。表示 attachment が stale でも、同じ owner への read 更新は許す（旧 modal は戻さない） |
| owner が inactive | Sessions に stale mark。次の `switch_repo` でその owner に戻る際、既存 reload を必ず起動。失敗時は stale を残す |
| owner/対象が closed・削除済み | 勝手に reopen しない。既存 cache の該当データを失効、管理元への invalidation と repo 名付き通知。次回 open は新規読込 |

remove は管理元と対象を別々に invalidate する。削除された L を無条件 reload せず、登録一覧は
管理元 A の reload で更新する。失効通知は read ownership の移管ではなく既存 reload への接続。

記録の「必ず」は **すべての受理した試行が recording boundary に到達する設計**の意味。
現行 JSONL は disk-full・process kill に対する exactly-once durable transaction ではない。
append 失敗を successful receipt にしない。panic 直前の副作用を完全復元するには別の journal が必要であり、
slice 1 は通常結果・捕捉可能な unwind・tab 消滅（KagiApp 生存）の境界を対象とする。
slice 1 の `NeedsReconcile` 発生源は **副作用領域の捕捉可能 unwind と pre_remove の結果/停止不明**。
副作用開始は削除ではなく config grant / pre_remove の開始を含む（§8.1 実行証跡）。
PM 裁定により、pre_remove 開始後の既知の失敗は Partial、結果/停止不明は Unknown。
削除前・unwind 以外という理由で単純 Failed に限定しない。
停止確認後は busy lease を解放し、新 app admission は対象を要確認として拒否する。
出口は **read（管理元の再 snapshot/status、対象登録/存在の照会）→確認 modal の acknowledge**。
read 失敗では留まり、**実行主体の停止確認済み**かつ成功した照会結果を acknowledge して通常状態へ戻す。
これは mutation/復旧の承認ではない。snapshot/status の成功は process 停止の証明には使わない。
復旧が必要なら通常状態へ戻した後に別 request/plan を作る。自動再実行はしない。
legacy bypass まで一律保護する保証は slice 1a にない。1b の共通 admission も要確認状態を拒否する（§7）。
pre_remove の直接 child 終了と子孫 process の停止は区別する。停止不明なら新 app admission を拒否し続け、
**lease を保持し read+ack で解除しない**。現行 runner の process-tree 制限は #507 に委ね、slice 1 で supervisor は作らない。
PR/SSH/worker channel loss からの reconcile は後続 family の設計事項。

## 5. GUI / CLI / MCP の共通実行契約

### 5.1 resolver と policy

| 値 / 判定 | 共通の owner / 方針 |
|---|---|
| request 解決 (#509) | 文字列→typed request の純粋検証と、Backend による HEAD/OID/path 解決を分離。既存 `Operation` を使う。CLI/MCP の二つの match を一つにする |
| plan wire DTO | Git crate の GPUI 非依存 facade に一つ。既存 JSON field/意味を維持し、domain に serde を追加しない |
| actor | adapter identity から `Human/Cli/Mcp`。tool 引数で任意 actor に偽装させない |
| auto_snapshot | GUI は既存 Settings を host が読む。CLI/MCP は当面明示 policy（default on）、GUI 設定ファイルを暗黙に読まない |
| policy の凍結 | plan 時に適用値を提示し approval に束縛。承認後に安全関連設定が変われば再plan。worker の起動時 default に依存しない |
| trust | slice 1 は job 内で dispatch 後に新しく `Backend::open` し、その評価と専用実行入口の gate を使う。RepoSession の長寿命 read handle は mutation に使わない。worker 導入時の再評価は後続。application が trusted bool を権限として与えない |
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

| ExecutionReport / Recording | 永続 JSONL (`OpOutcome`) と receipt の対応（slice 1a） |
|---|---|
| Success / Refused / NotStarted Failed | 既存 Success / Refused / Failed variant。未観測 after を予測で埋めない |
| Partial / 停止済みの pre_remove 失敗 / verify failure | 既存 `Partial { after, error }`。after に取得済み path→full blob OID・branch full OID、error に stage/観測済み step/再試行リスクを保持 |
| Unknown / 副作用領域の unwind / 停止不明 | **`Unknown { after, evidence }` を OpOutcome に追加**。after に同じ復元材料、evidence に stage・step・停止状態・検証結果を保持。Partial への隠し encoding はしない |
| Recording::Appended | 上記 outcome を含め、その append が書いた実 entry を RecordReceipt に返す |
| Recording::Failed | 同じ outcome/evidence の attempted entry と append error。過去 tail を返さず、mutation の再実行は禁止 |
| Recording::Exempt | §5.3 の明示例外のみ。remove は該当しない |

既存 variant と JSON 表現は変更せず、旧 JSONL の parse/serialize 互換を保つ。
新 Unknown は全 reader/UI の網羅処理で非成功として扱い、手書き serializer/parser も同時に対応する。
古い実行バイナリが新 variant を読めるとは保証しない。新旧 golden round-trip と
Partial/Unknown の after から full OID を取り出す復元 test を実証に含め、ADR-0149 へ追記する。

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
| `stage_file(s)`, `unstage_file(s)` | 必須 | 明示クリック/要求。modal 表示は免除、対象 plan は boundary で束縛 | worktree/path/index/content 条件 | index の対象差分 | WT/refs 非変更なので自動 snapshot 免除 | 到達契約は一件記録。現行の未記録を恒久例外にしない（slice 1 は変更しない） |
| `fetch_remote`, `fetch_remote_branch` | 必須 | 明示操作/auto-fetch policy。modal 免除 | remote/refspec | 更新 refs | WT 非変更、自動 snapshot 免除 | fetch boundary に一件（auto は actor/source 明示） |
| `create_snapshot` | 必須 | 明示要求、plan modal 免除（ADR-0154） | 対象/保存 ID | ref/tree | 自身が復元点。再帰 snapshot 禁止 | 単独要求は一件、auto 子処理は親 report に束縛 |
| `delete_snapshot`, `prune_snapshots` | 必須（delete は現状欠落） | delete は対象確認、prune は cap policy の承認 | ID/ref/cap | 残存 ref | snapshot 再作成しない（保持上限が無効になる） | 単独一件 / auto は親に削除 ID を含める |
| `trust_repo`, `trust_worktree_config_for_worktree` | 未trusted状態からの grant を許す特別入口 | 明示 identity/config SHA 承認 | path/owner/SHA 再検証 | trust store | WT snapshot 対象外 | grant boundary で監査。mutation 成功と同一視しない |
| `github::merge_pr` | transport policy | PR/head/method の承認 | PR state/head/ruleset | remote merge state | merge OID。ローカル snapshot は免除 | transport へ移す。timeout は Unknown |
| `github_merge::enqueue_pr/dequeue_pr`（公開 API、production caller なし） | transport policy | PR node ID/queue 操作を明示承認 | node/repo/queue 状態 | queue membership | queue の before 状態。WT snapshot 免除 | transport に一件。Git ref mutation とは別結果 |
| `remote_stash_drop`, `remote_pull` | remote 側 gate | request/remote plan の承認 | remote HEAD/stash OID 等 | remote 状態 | stash OID/remote recovery | transport owner、drop の既存記録を維持 |
| editor save/overwrite・create/rename/trash/gitignore | FS capability の対象制限（Git owner trust の代用ではない） | 保存は直接意思、overwrite/trash は明示確認 | containment/.git 禁止、file/buffer revision、衝突 | 保存 bytes/移動先 | buffer/Trash。自動 Git snapshot 免除 | 通常保存は oplog 免除（ADR-0120）、他 FS も Git oplog 外の明示例外。typed FS report/通知 |

例外は enum/契約として明示し、`destructive: bool` 一つから全 gate を推論しない。
index の modal/snapshot 例外、FS の明示例外でも admission・失敗通知・対象固定は免除しない。
index-only の oplog 免除は r1 から撤回する。AGENTS.md の Git write 記録原則を優先し、
例外化したい場合は別 ADR/規則変更の合意が必要。slice 1 は staging 実装も記録 UX も変えない。
config/draft の保存、worktree port 割当、conflict autosave はそれぞれ所属 service の metadata write。
worktree steps/port 割当は親 lifecycle report に帰属し、独立した未記録の Git mutation にしない。
terminal は自由な外部 shell であり Kagi の承認済み mutation API ではない。terminal-start 記録は
host の起動監査として残し、その中の任意コマンドを本契約が保護すると主張しない。

現状の追加注意: `fetch_remote`/`fetch_remote_branch` の facade は `require_trust` を呼ばない。
#502 の public mutation gate 棚卸しに含めるべき既存差分。ただし実行は Git CLI へ委譲されるため、
OS 所有者を変更した実害再現と facade の gate 欠落は別の主張。untrusted test seam で refusal と refs 不変を検証する。
`Backend::run` の `CreateBranchWithCheckout` arm の `?` は末尾記録を迂回し得る
（独立 bug [#522](https://github.com/TomiXRM/kagi/issues/522)、本設計の合意待ちにしない）。
また `blocking_ops` の verify は run の記録後に実行される。共通化時は family の実行結果を
内側で捕捉し、verify/partial も含めて外側 recorder に一度到達させる。receipt wrapper を
既存 run の外に足して二重記録する方式にはしない。

## 6. 既存実装への接続と GPUI 非依存境界

| 現行要素 | 再利用するもの | 新設 / 撤去するもの |
|---|---|---|
| `Backend` / `ops/*` | plan/preflight/execute triples、安全判定、実 verify、recording helper | family report/receipt を既存実行入口へ追加。生 executor の外部 caller を移して公開縮小 |
| `git::RepoSession`（ADR-0107） | foreground read handle (`Rc<Backend>`) | slice 1 では変更しない、`Rc` を job に送らない。後続 session map slice で subordinate resource にする |
| `RepoWorker` | 将来の worker 候補と既存テスト | 初回は配線しない。production caller ゼロを「全 mutation 配線済み」と書き換えない |
| `finish_op_on_main` / `reject_if_busy` | 既存 panic/stale/busy の regression intent と klog 契約 | slice 1 の remove は `KagiApp::dispatch_job` へ。reject は busy/leases の両方を見る。旧 helper は他 family に残す |
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
fn prepare_remove(session: &mut Sessions, approved: Approved<RemovePlan>, legacy: LegacyBusy)
    -> Result<RemoveJob, AdmissionError>;
// app の job は GPUI を知らず、Send な値だけを保持する。
impl RemoveJob {
    fn run(self) -> Completion<RemoveOutcome>; // 同期本体、boundary が記録済み
}
fn apply(sessions: &mut Sessions, completion: Completion<RemoveOutcome>)
    -> Vec<Delivery>; // 所属・lease・重複配送判定。append はしない
```

slice 1 は **`KagiApp::dispatch_job` 一箇所**だけが background spawn と main-thread 復帰を担当する。
既存 `cx.spawn` の KagiApp update 内で、表示 stale guard に関係なく `apply` を先に呼ぶ。
Sessions は KagiApp の一フィールドであり、別 global/mailbox/actor を設けない。
spawn 拒否・未実行 job drop も OperationId 付き terminal event にして lease を解放する。
Entity 自体が破棄された場合は update できないが、job 内の記録は update の到達を条件としない。
通常の window close/Quit は §2.2 の lease guard で保留する。強制終了を跨ぐ runtime drain は保証外。
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
| history undo/redo: `operations/history.rs::confirm_history` | app history → `run_history_move` | UI preflight/stack 更新の所属判定 | sync 維持（移管後に計測）。G/E/M、index/WT 不変。#493 と scheduling 変更を混ぜない |
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
同じ diff で示す。slice 1 は **保守的な二重化**で既存 gate を通る legacy と共存する:

- `dispatch_job` が lease 確立と同じ main-thread turn で既存 `busy_op` も立てる。
- `reject_if_busy` と新 admission の判断入力は `busy_op.is_some() || !leases.is_empty()`。
  app は `LegacyBusy` という値を受け、GPUI/KagiApp フィールドを直接読まない。
- completion は OperationId を照合して自身の lease を解除し、bridge が対応する busy mirror を解除する。
  duplicate/stale completion が別 operation の busy を消すことはない。
- **staging/snapshot/fetch/editor save の先行接続を slice 1a の前提から外す**。

slice 1a に残る既知の穴: `do_stage_all` 等の staging と `create_snapshot_now` は busy を通らず、
fetch は別 in-flight、editor save は pane 側 job を持つ。fetch が既に走っている場合の逆方向など、
既存 gate 外の競合を 1a は新たに防がない。**横展開前の slice 1b** で admission のみ接続する。
従って **remove を含む全 mutation の排他完成を宣言しない**。既存の安全 preflight/backup を弱めず、
新 bridge で既存 gate 内の相互排他・結果所属・記録を実証する。
全 write の admission 移行完了後にのみ per-RepoId の独立 repo 並行許可を有効化する。

## 8. 最初の縦断実証: worktree remove

| 候補 | 得られる実証 | 初回としての評価 |
|---|---|---|
| stash-drop | recording 済みなので app seam の費用が低い | 残る non-run writer と複数 worktree 所有を検証しにくい |
| PR merge | transport/Unknown を検証できる | 認証・remote simulation が主題を増やす |
| worktree remove | 管理 repo と対象 worktree の分離、長い job、backup/partial、close race、既存 UI writer | **採用案**。local fixture と既存 containment/steps テストがある |

### 8.1 最小 cut

| slice 1a — 記録・配送・寿命・協調入口の実証 | 完了条件 |
|---|---|
| identity / operation registry の最小 slice | 管理元 repo、対象 worktree、input revision、lease を window なしで保持。admin fingerprint を承認に束縛。GUI origin は既存 `(repo_path, switch_generation)`、TabId/session incarnation は導入しない |
| 専用 remove execution boundary | open failure/trust/config grant/preflight/既存 execute/verify/record を一箇所へ。外側所有の RemoveProgress と永続 Unknown を追加 |
| report/receipt | append 成否を結果へ。管理元 repo + 削除対象 worktree を区別して記録。path 削除後も照会可能 |
| GUI adapter | ボタン/Enter → 同一 approval/bridge。UI の remove 用 writer と個別 busy/世代 guard を削除 |
| legacy interoperability | §7 の lease + busy mirror を共通 bridge で検証。既存 bypass は残存を明記し先行解消しない。新規 family の UI がラッチを手でコピーしない |
| host lifetime | 実行中 lease がある間 window close/Quit を共通入口で保留。最後の tab close は Welcome で継続 |

**1a に含めない**: worker 配線、全 snapshot/cache の移管、TabId/session incarnation、
window より長寿命の registry/global host/mailbox、複数キー一括 admission、
editor save 等の admission 接続（1b）、全 modal の書換え、CLI/MCP の remove tool 新設。

| slice 1b — 横展開前提の admission 接続（executor は移さない） | 完了条件 |
|---|---|
| editor save（最優先） | pane の dispatch 前に共通 write lease を取得。remove backup→delete 間の save は Busy、save が先行中なら remove が Busy。保存済み bytes を失わない |
| staging | single/batch、panel/editor の入口を同じ admission へ。既存同期実行中も lease を保持 |
| create_snapshot_now | capture dispatch 前から完了まで lease。既存 snapshot 本体/記録契約を維持 |
| fetch | manual/auto/branch 各 dispatch と既存 in-flight を lease に接続。先行 fetch と後続 remove も逆方向も Busy |

app の `write_lease(path)` は単なる busy 問い合わせではなく、identity 解決と Busy/NeedsReconcile 判定を伴う
**予約の取得**。GUI 共通 bridge が既存 busy mirror と同じ turn で確立し、owned guard を job 完了/停止確認まで保持する。
check→spawn の間に別 writer が入る TOCTOU を作らず、既に走る writer も追跡する。
pane は host から予約された job を受け取ってから保存を dispatch する。未知停止時は guard の通常 Drop で解放しない。
独立 repo の並行許可や executor 移管は 1b に含めない。1a だけで「排他を実証した」と呼ばず、横展開 gate は 1a+1b。

同じ remove request/policy を window 無しの consumer で実行し、GUI と同じ report を得ることを示す。
CLI/MCP の既存 operation resolver/receipt 消費は #509/#505 の後続 slice で本物の両 consumer を移す。
この一 family の成功だけで #484 全体を close しない。

#### 副作用の段階・unwind を跨ぐ owner（第2レビュー R2/R3）

`RemoveProgress` は **Backend execution boundary が catch_unwind の外側で所有**する。
既存 executor と pre_remove runner に限定的な `&mut RemoveProgress` を渡し、
app/GUI ではなく Git/transport 層が各副作用の前後に更新する。返却 `DiscardOutcome` だけに依存しない。
これは in-memory の実行証跡であり、crash journal/任意 command rollback ではない。

```rust,ignore
struct RemoveProgress {
    stage: RemoveStage,
    backups: Vec<(PathBuf, CommitId)>, // full blob OID、取得直後に外側へ保存
    branch_tip: Option<CommitId>,
    // step の開始/終了/停止証拠、config grant、verify の観測も保持
}
enum RemoveStage {
    NotStarted, PreRemove { step: usize }, BackupCaptured,
    DeletionStarted, AdminPruned, BranchDeleted, Done,
}
```

段階順は上記 enum。optional branch delete は飛ばせる。不可逆副作用の**直前**に開始情報を更新し、
完了済み stage は成功を観測してから進める。例えば prune 呼出前は DeletionStarted + prune開始、
成功後だけ AdminPruned。branch tip は削除前に保存する。NotStarted のまま config grant してはならず、
grant の開始/成否も progress に保持し、それ以後の失敗を未変更扱いしない。

| stage / 証跡 | 観測済み副作用・保存値 | 失敗 / 再試行の安全性 |
|---|---|---|
| NotStarted | 凍結 target・plan fingerprint、副作用未開始 | open/trust/preflight 拒否は Failed/Refused。再試行も新 plan/承認が必要 |
| PreRemove { step } / config grant 証跡 | grant の成否、step index/kind、開始・終了、判明している exit/停止状態 | 途中 Err/停止済み timeout は Partial、停止不明は Unknown。副作用を繰り返し得るため自動再試行不可 |
| BackupCaptured | 取得ごとに path/full blob OID を外側へ保存、branch name/full tip 捕捉 | panic でも取得済み材料は残る。先行 step があり自動再試行不可 |
| DeletionStarted | 削除直前の stage、観測した消失状態 | Err は Partial、内部 unwind は Unknown。自動再試行不可 |
| AdminPruned | admin prune 成功、次の branch 削除開始情報 | 先行副作用を保持。自動再試行不可 |
| BranchDeleted | optional branch 削除成功、削除前 full tip | 復旧は別 plan。自動再試行不可 |
| Done / verify 証跡 | 登録/path/branch の観測結果または検証 error | verify 失敗は Partial/Unknown、成功観測だけ Verified。再試行不要/自動再試行不可 |

任意 command の全外部効果を列挙できるとは主張しない。step 開始・終了・停止証拠と
「外部効果未確認/再実行リスク」を report に保持し、timeout 後の自動再試行を禁止する。
既知の partial copy 等は停止済みだが、child だけ kill/reap した timeout は process-tree 停止確認済みにしない。

永続化は §5.2 の単一対応表に従う。receipt は **progress 由来の after/evidence を含め実際に append した entry**。
append 失敗でも attempted entry と証跡を返し、過去 tail で代用しない。

保持した裸の ODB blob/OID は ref-backed snapshot ではなく、GC 後の到達可能性を保証しない
（ADR-0154 が既に記す制限、第1自動レビュー指摘）。slice 1 は既存 backup の記録消失を閉じる範囲。
**GC 後も復元可能という保証は未達**と明記する。専用 recovery refs/snapshot と retention は別の安全設計として
PM に切り分けを求め、今回の復元 test を GC 耐性の証拠にはしない。

#### window なしで使う唯一の公開 API 列（B8）

以下は仮シグネチャだが、この呼出列を integration test と GUI で共有することを実証条件とする。
Approved/PlanToken は外部で直接構築できず、`PlanState::Ready` は発行済み token の観測面。
state をテスト側で書き換えて承認を迂回する seam は作らない。

```rust,ignore
let mut sessions = Sessions::new();
// plan_remove は read-only の PlanJob を返す。GUI は背景、G は inline。
let plan_job = plan_remove(&mut sessions, request, policy.clone())?;
apply_plan(&mut sessions, plan_job.run());
let token = match sessions.plan_state(request_id) {
    PlanState::Ready { token, .. } => token.clone(),
    _ => return, // GUI は実行不可、test は必要な state を assert
};
let approved = approve(&mut sessions, token, policy)?;
let job = prepare_remove(&mut sessions, approved, LegacyBusy(false))?;
let completion = job.run();
let deliveries = apply(&mut sessions, completion);
```

`plan_remove → Ready{token} → approve → prepare_remove → run → apply` が共通列。
公開 API の実装で input revision/policy/owner を照合し、一度 dispatch した token の再消費を拒否する。
GUI は Ready 表示と承認 UX を既存 remove modal に接続し、approve 後の処理を
`KagiApp::dispatch_job(approved, cx)` に渡す。この bridge だけが LegacyBusy 値の取得、
prepare/remove lease と busy mirror の同時確立、spawn、apply、Delivery の表示 guard を行う。
G は `LegacyBusy(true)` も渡して拒否を試せるが、Approved の constructor には触れない。
read-only plan job が返すエラーも PlanState に反映し、旧 Ready を使えないことを試す。

#### PR 分割・規模の目安（C5）

推奨は **2 PR（1a / 1b）**。1a は最小 app skeleton + remove boundary/report + G + GUI bridge + E/実機、
window close/Quit guard・fingerprint・progress/JSONL Unknown を一つの縦断実証にする。skeleton-only は先行 merge しない。
1a は production 550〜850 行追加、test 400〜650 行追加、旧 glue 100〜160 行削除、約 12〜18 ファイル。
1b は writer admission 接続だけで production 150〜300 行追加、test 150〜250 行追加、約 6〜10 ファイル。
未実装の概算で LOC 目標ではない。両 PR の G/E/M と gate が通るまで横展開しない。
receipt API の全 caller 整合で広がる場合は既存 append API を保ち実 entry を返す sibling を追加し、
既存 append はそこへ委譲する。writer の二重実装はしない。全 run consumer の API 変更は #505 へ分離。
さらに分割が必要な具体的依存が判明した場合は PM と合意し、黙って土台 PR を増やさない。

`src/ui/operations/worktree.rs` は remove の open/replan/confirm を共通 API へ接続し、
confirm 内の trust write・busy 操作・background open/execute・on_done persist を削除対象とする。
既存 remove 表示/klog は adapter に維持。他の create/open/lock/unlock/prune/repair と
`worktree_wip.rs` の `WriteOrigin`、commit/amend/discard の #476 経路は触らない。

### 8.2 テスト設計（今回は実行しない）

各 test は tempdir の main A / linked L / 独立 B と専用 log dir。sleep に依存せず、
preflight 前・mutation 後/配送前の barrier または手動 job runner で順序を固定する。

#### integration test から使える fault seam（B7）

`#[cfg(test)]` の lib 内 seam は `tests/` から見えないため使わない。
`RemoveJob` が private な `fault: Option<RemoveFaultPoint>` を持ち、通常 prepare は必ず `None`。
承認済み job に対する公開 `with_fault_for_test(point)`（doc-hidden、release でも利用可）を一つ設ける。
有限 enum だけを受け、任意 closure/対象変更/安全 gate のスキップは許さない。
GUI/CLI/MCP はこのメソッドを呼ばず、設定/env/tool 引数への配線もしない。

| fault / 検証対象 | 具体的な注入位置と手段 |
|---|---|
| `PanicBeforeMutation` | execution boundary の捕捉領域内、config grant/pre_remove/削除の前。未変更 Failed として一度記録 |
| `PanicAfterDeletionStarted` | **executor 内部**、backup 捕捉後・削除開始後・正常 return 前に注入。外側 RemoveProgress から Unknown/recovery を作り一度記録。正常 return 後だけの注入では代用不可 |
| partial remove / pre_remove | `FailAfterBackupBeforeDelete` / `FailAfterDirectoryDelete` を既存 primitive の phase に渡す。pre_remove は実 copy step 成功→次 step の実エラーで副作用の残存を確認。公開 API の None 経路は挙動不変 |
| `PreRemoveTerminationUnknown` | step 開始後の runner 結果を有限の停止不明値にする契約 test 用 seam。process-tree kill が動いたという証拠には使わない。実プロセス所有の検証は #507 |
| append failure | ENV_LOCK + 専用 `KAGI_LOG_DIR`。readonly dir を使う場合は権限を必ず復元。権限を迂回できる実行環境もあるため、**標準の決定的 fixture は log file path をディレクトリにして append を失敗させる**（EISDIR 等、具体 errno の一致は要求しない） |
| open failure | 専用 fixture の管理元 `.git` を plan 後/job.run 前に一時 rename。RAII cleanup で戻す。実ユーザー repo は触らない |

panic を catch する owner は recorder より内側の execution boundary 一つ。
**記録後**に panic を注入して outer catch が再appendするテストは作らない。
実際の executor 内 unwind は副作用領域に入っていれば未変更と断定せず Unknown。
取得済み材料を外側 evidence に残す。未取得分は `evidence unavailable` と明示し、成功 outcome を捏造しない。
barrier が不要な配送 test は `completion = job.run()` 後に手動で切替/close→apply とし、
GUI E2E では foreground 更新を同一 turn にまとめ、必要な slow pre_remove fixture で実行中も確認する。

| test | 操作 / fault | assert（実状態と配送） |
|---|---|---|
| normal | clean L の remove、branch delete on/off | L/admin の消失、branch の期待状態、receipt 一件、actor/管理元/対象一致、busy 解放 |
| refused | dirty/locked/main/対象 missing、trust 拒否 | path/refs 不変、typed refusal/failed stage、実行受理済み試行は記録一件 |
| revision | plan A→入力変更→replan failure、旧完了後着 | Enter/ボタン両方不可。再plan成功後の最新対象だけ実行 |
| preflight drift | plan 後に L dirty/lock/config SHA/登録 identity 変更 | 不承認の hook/削除が走らない。旧 plan の内容で別 worktree を消さない |
| same-locator ABA（R4 / 1a） | plan 後に同名同path・同OID同config の L を再作成 | admin fingerprint 不一致で Refused。新しい L は削除されず pre_remove も走らない |
| open failure | plan 後に管理元を fixture 内で一時的に利用不能にする | Backend がなくても failed report/receipt。一度。B の表示は不変 |
| tab switch | A で開始、B に切替してから completion を送る | A/L の結果記録一件、B の modal/footer/selection 不変、A session stale |
| close / Welcome | 実行中の owner tab を閉じる / 最後の tab を閉じる | 実行・記録は完了、閉じた Entity 更新なし、host に対象付き通知、pin 解放 |
| window close（R1 / 1a） | job を barrier で止め native/menu close、完了後に再要求 | 実行中は操作中 modal で保留し host/lease 維持。完了後は close 可。Dock reopen 時に旧 in-flight 無し |
| Quit（R1 / 1a） | 同じ停止点で menu/shortcut Quit、完了後に再要求 | cx.quit に進まず保留、完了後に終了可。最終 tab close の test と独立 |
| editor save / ほかの bypass（R5 / 1b） | remove backup→delete で save 要求、save 先行中の remove。staging/snapshot/fetch も両順序 | 競合は Busy、予約前の write 無し、保存済み bytes 消失無し。未知停止/NeedsReconcile も拒否 |
| background close / reselect | A 実行中に B close、A 再選択 | slice 1 は A の operation 終端化/記録が必ず届く。既存 generation 変更なら footer は落として対象付き通知。selection/reset の完全修正は #488 |
| duplicate / concurrency | double confirm、重複 completion、gate を通る sibling/legacy mutation | 同じ token の実行は一度、lease owner のみ解放。busy/leases の両方向拒否。gate 外の既知 bypass は合格対象に含めない |
| partial | backup 後、削除/admin prune/branch delete の途中に fault | partial の backup full OID を receipt と JSONL に保持。fixture bytes を ODB から復元して一致 |
| panic / reconcile | 上記 enum の副作用前/削除開始後の executor 内 unwind | JSONL を再parseし backup/full tip/Unknown/verification を抽出、fixture 内容を復元。停止確認済み read→ack で解除、read 失敗/旧 ack は解除不可 |
| pre_remove failure / termination | 実 copy 成功→次stepエラー、timeout 結果の有限 fault fixture | 削除されなかったことを未実行と判定しない。既知 step と再実行リスクを JSONL に保存。停止不明は read+ack でも解除しない。実 process-tree 管理は #507 |
| append failure | log writer を失敗させる | mutation 成功と記録失敗を同時に返す。過去 tail を返さず、retry で削除を再実行しない |
| receipt race | append 後/応答前に別 repo・同 repo 同 op の entry を追加 | 応答 entry は最初の実行そのもの。global tail とは異なっても正しい |
| invalidation | completion→Delivery を active/inactive/Welcome に配送 | active owner は既存 reload、inactive は stale mark→再選択で reload、削除対象は無条件 reload しない。read supersede の全面改修は #489 |

既存 `tests/{worktree,git2_owner_trust,oplog_nonrun_ops}_test.rs` と
`tests/gui_e2e_runner.rs` の worktree/recovery scenario は残す。既存 assertions を弱めない。
新 G テストは UI method を呼ばず session/job/fixture の事実を assert する。
E は real modal/focus tree を描いた後の raw Enter、ボタン、tab close を使い実行境界への到達を確認する。
M は確認対象 worktree 表示、長い pre_remove 中の応答性、完了/拒否/部分成功通知を実アプリで確認しスクリーンショットを残す。
native runner は scenario PASS だけでなく **正常な終了 status** を確認する（既知 teardown 問題を成功扱いしない）。

**横展開 gate は 1a+1b の G/E/M 通過**。1a は記録・配送・寿命・協調入口間 admission の合格、
1b は競合 writer の admission 接続の合格。remove UI writer/旧 executor glue が消え、window 無しの
progress/owner、window close/Quit 保留、保存競合を検証できること。1a 単独で排他完成とは呼ばない。
#488/read 所有や他 family の移管は残り、#484 全体は close しない。別 family が UI に busy/世代 guard を
再実装する必要があれば境界未成立として設計へ戻る。

## 9. ADR と既存 Issue の扱い

| 文書 | 維持する内容 | 改訂する内容 / 時期 |
|---|---|---|
| ADR-0073（2 文書） | Backend pipeline、worker という候補 | 未配線の現実、job/host と worker の分離、liveness/panic/Drop。#314 実証 ADR で置換範囲を指定 |
| ADR-0075 | session 単独所有・active 参照という目的 | tab=session、app に GPUI、controller が記録という前提を本設計へ。最初の所有実証 ADR で部分 supersede |
| ADR-0104 enforced pipeline | Backend に安全 gate を置く、旧 bypass を消す | 「全 mutation が run 済み」は現状と不一致。専用 run/例外表・verify 所有を family ごとに追記 |
| ADR-0107 | read Backend handle の再利用 | `RepoSession` は application session ではない。単一 active slot→map 配下への寿命変更時に追記 |
| ADR-0121 | feature-pane Entity、seed/event、境界安定後の crate 分離 | bin に残した進行/記録 glue の app/backend 移管だけ追記。pane 分離は巻き戻さない |
| ADR-0149 + recovery | mutation-owned recording、actor/worktree/full recovery | receipt、open failure、永続 Unknown と後方互換・progress の復元材料。#501/#505 実証で追記 |
| ADR-0120 / ADR-0154 | editor 保存の Git pipeline 例外、snapshot の明示/自動区別 | 本文の FS/index/metadata 例外を正式化する際に整合を取る。暗黙に既存 UX を変えない |

本書は ADR ではなく、#484 の議論用設計。合意後に PM が Issue の Plan に要約する。
実証 PR で **実際に採用した境界だけ**新 ADR にし、上表の古い記述へ相互リンク・superseded 範囲を追記する。
本 docs PR は既存 ADR の Accepted 状態を変更しない。

| PM にレビューいただきたい点 | 推奨 / トレードオフ |
|---|---|
| common-dir 単位の初期排他 | linked worktree の並行性より安全・簡潔さを優先。per-worktree 緩和は後続 |
| app root module + backend 共通契約 | 先に crate を作らず、MCP が job を必要とするときに抽出。安全/request 契約の共有は先行可能 |
| tab close 後の通知 | KagiApp で Welcome/owner 通知、1a で window close/Quit 保留。window 外 registry は後続 |
| sync/index/FS の例外 | 記録所有移管と scheduling/UX 変更を分離。全 mutation に modal を強制しない |
| 初回の成果範囲 | 1a: remove/最小 registry/receipt/lifetime/fingerprint、1b: writer admission のみ。TabId/session incarnation、全 session 移管や worker は含めない |

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
