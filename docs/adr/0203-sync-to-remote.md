# ADR-0203: ローカルを保全して remote-tracking tip に揃える

- Status: **Draft — 設計レビュー用。未承認・未実装**
- Issue: [#536](https://github.com/TomiXRM/kagi/issues/536)
- Related: [ADR-0179](0179-ref-backed-discard-remove-backups.md),
  [ADR-0196](0196-operation-lifecycle-contract.md),
  [ADR-0154](0154-working-tree-snapshots.md), #484, #523, #534
- 調査基準: `ab84b084`。この変更は本 ADR のみで、操作・型・UI を追加しない。

## 結論

**保全を先行させた合成は可能だが、既存の stash push → checkout → ref 移動を
そのまま並べるだけでは不可。** 単一の `SyncToRemote` operation とし、
`plan → confirm → preflight → execute → verify → oplog` を一度通す。
現在のローカル branch を保持したまま、その tip・index・tracked working tree を
承認済み remote-tracking commit に揃える。禁止されている破壊的 reset、
force checkout、作業ツリーの一括 cleanup は用いない。

stash push は保存と作業状態の消去を一括実行するため、返された stash OID を
後から pin するだけでは遅い。**消去前に HEAD・index・raw working bytes を
ref-backed archive として完成・検証する**。その後の stash は標準 Git で使える
追加の復旧手段であり、唯一の保全物ではない。

本 ADR はこの方式を選ぶ。下記の実装開始条件を満たすまでは有効化しない。
ref-backed であることは GC 到達可能性の保証であり、checkout・refs・oplog を
跨ぐ transaction、外部 writer の排他、電源断耐性を意味しない。

## 利用者に約束する結果と対象

- 対象は **現在 checkout 中の、既存 commit を持つローカル branch `B`**。
  設定済み upstream の具体的な remote-tracking ref `R` と full commit OID `T`
  を選ぶ。`HEAD` の symbolic target は最後まで `B`、成功時の `B` の tip は `T`。
  detached checkout、新 branch 作成、別ローカル branch の暗黙切替は別操作。
- ネットワークアクセスは含めない。`T` は取得済みの状態であり、サーバー上の
  最新値を保証しない。必要なら通常の Fetch を先に実行し、完了後に新しく plan
  する。承認後の fetch・target 差替え・自動 replan はしない。
- 成功は `HEAD = symbolic B`、`B = T`、index の `(path, OID, mode)` が
  `T` の tree と一致し、tracked working tree が凍結した変換規則の下で clean
  であること。CRLF 等の変換があるため、Git blob と filesystem bytes の一致とは
  呼ばない。既存の untracked files は保全して退避する。
- 非衝突の ignored files はその場に残す。従って「ディレクトリ全体が remote と
  同一」ではない。ignored 内容の上書き、削除、target との file/directory 衝突は
  拒否する。空 directory、mtime、ACL、xattr、所有者の復元は約束しない。
- 退避対象のファイル内容、Git mode、symlink target、削除状態、元の staged /
  unstaged の分離を復元可能にする。保全物は stash list・reflog・任意の自動
  snapshot の寿命に依存させない。ignored data は退避対象に含めない。
- `B = T` でも dirty/untracked があれば退避が必要。`B = T` かつ対象が clean
  なら no-op と表示し、保全 refs を作らない。

## 既存機能と不足する保証

パスは repository root 相対。行番号ではなく以下の symbol を実装時にも確認する。

| 既存 owner / symbol | 再利用できるもの | そのままでは足りないもの |
|---|---|---|
| `crates/kagi-git/src/ops/backup.rs`: `retain_object` | create-only の `refs/kagi/backups/<attempt>/<ordinal>`。blob / commit を保持し、error / Drop で消さない | capture、archive schema、index 復元、再起動時 discovery はない |
| `crates/kagi-git/src/oplog/retention.rs`: append / retirement validation | blob と commit の root、明示的 forget、共有 ref の保護 | OID を JSON に書くだけでは Git の到達可能性にならない。ADR-0179 の blob-only の説明より現コードが広い |
| `crates/kagi-git/src/ops/stash_push.rs`: `execute_stash_push`, `identify_created_stash` | hardened CLI、元 HEAD / message / reflog 増分による作成 OID の識別、曖昧なら拒否 | CLI は返却前に index / WT を消去する。`stash@{0}` は stable identity ではない |
| `crates/kagi-git/src/backend/stash.rs`: `verify_stash_run` | 作成 stash の存在、stack の維持、消去後 status | 元の index / raw bytes が保存された証明ではない |
| `crates/kagi-git/src/ops/checkout.rs`: `execute_checkout` | safe checkout、branch occupancy 再確認 | checkout 後に HEAD を更新する非 atomic な二段階。git2 0.21 の `overwrite_ignored` は既定で true |
| `crates/kagi-git/src/ops/branch.rs`: `execute_delete_branch` | ref lock、expected tip 比較、変更前 pin の先例 | ref lock は外部 editor を止めない。git2 の複数 ref transaction も全体 atomic ではない |
| `crates/kagi-git/src/ops/reset.rs`: `execute_reset_current_to_head` | ref のみ動かす既存操作 | expected-old CAS ではない。先に branch を動かすと checkout baseline を誤る |
| `crates/kagi-domain/src/status.rs`: `WorktreeDigest` | status 分類の変化検出 | 同じ分類のまま bytes が変わっても一致する。承認済み内容の証明に使わない |
| `src/app/{flow,session,settle,reconcile}.rs` | 一回の admission、`OwnerStamp`、lease、exactly-once settle、Unknown / stop proof | progress / lease はメモリ内。process crash 後に復元される journal ではない |
| `crates/kagi-git/src/backend/{run,recording}.rs` | `run_recorded`、`Recording::Appended / Failed` | 通常 operation の `Ok` / predicted state を sync の実測 verify と同一視しない |

ADR-0154 の snapshot は stage 分離を失い、optional / cap 付きなので必須保全に
使わない。既存 `execute_stash_apply` は `REINSTATE_INDEX` を要求しないため、
そのまま「元どおりに復元」の実装にも使わない。

## 所有権と拡張箇所

既存の層と owner を拡張する。汎用 transaction manager / 新 app crate は作らない。

- `kagi-domain`: `Operation::SyncToRemote`、専用 plan / progress / verification の
  純粋な値。Git、GPUI、I/O、app の session / operation counter を持たせない。
  `OperationPlan` と typed title / note / recovery は表示用に再利用する。
- `kagi-git`: feature 境界 `ops/sync_to_remote.rs` に
  `plan_sync_to_remote / preflight_sync_to_remote / execute_sync_to_remote /
  verify_sync_to_remote` をまとめ、既存 Backend の dispatch / recording に接続。
  backup / retention / supervisor の owner は再利用する。これは新たな複合 Git
  操作の境界であり、独立した保存サービスではない。
- `src/app`: 既存 family の `Planned / Completion / FamilyEvidence` と reconcile
  に sync を追加。一つの immutable request、一回の `begin_write`、一つの
  supervised job / lease / settle とする。現在の admission は他 repo の lease
  でも拒否するため、ここで並列実行範囲を広げない。
- UI: 既存 `ActiveModal`、plan wrapper、branch menu、oplog presenter を拡張。
  `RepoSession::backend()` と app family を通し、UI で Git / filesystem を触らない。
  独立した保全状態や現在タブから再取得した target を worker に渡さない。

## 提案する domain 契約

以下は **schema sketch であり、現存 API / コンパイル可能コードではない**。
型名の追加先は既存 crate の feature 境界。`RepoId / WorktreeId / Head / CommitId /
OperationPlan / StashIdentity` は既存型を使う。

```text
SyncToRemotePlan {
  worktree: WorktreeId,             // canonical RepoId を含む
  root: PathBuf,                    // locator。identity として比較しない
  before_head: Head,                // attached B + full H
  target: SyncTarget { remote_name, full_tracking_ref, tip: CommitId },
  approved_content: SyncContentStamp,
  stash_before: StashIdentity,
  preservation: SyncPreservationScope,
  display: OperationPlan,           // destructive=true、faithful equivalent command はなし
}
SyncContentStamp {
  index_entries,                    // path/OID/mode/stage/flags の canonical digest
  working_entries,                  // lossless path/kind/mode/raw bytes/absence の digest
  protection_state,                 // attrs/config、ignored 衝突、関連 ref/登録状態
}
SyncPreservationScope {
  tracked_paths, untracked_paths,   // 全対象。表示件数の上限で capture を切らない
  retained_ignored_paths,           // 不変更・非衝突であることを検査する範囲
}
SyncProgress {
  stage: Capturing | Preserved | Stashing | Stashed | CheckingOut |
         MovingRef | Verifying | Verified,
  actual_artifacts: [SyncArtifact { role, reference, oid }],
  stash_oid: Option<CommitId>,      // 識別できたときのみ。ordinal ではない
  observed_after: Option<SyncObserved>,
  uncertainty,
}
SyncObserved {
  head, branch_tip, index_tree_oid, tracked_clean, untracked_observation,
  ignored_protection_verified, preservation_verified,
}
```

- blob / tree OID は full object OID の文字列として区別し、`CommitId` に偽装しない。
  digest は backend が canonical encoding と content hashing で算出する。
  `WorktreeDigest` の別名にはしない。status-only refresh では生成しない。
- plan は read-only。capture 用の blob / tree / commit / ref を作らず、実際の
  保全物が存在するように表示しない。計画用 digest と実行時の archive 内容を照合。
- app request が `Arc<SyncToRemotePlan>`、`Attachment`、承認 revision / policy を
  凍結する。`OwnerStamp` は admission 時に発行。durable attempt ID は
  `ops::backup` の方式を用い、oplog sequence / app `OperationId` と混同しない。
- `SyncProgress` は unwind の外から参照可能にするが、永続 journal とは呼ばない。
  `Recording` は Git outcome と直交し、domain progress の成功 flag で代用しない。

## 消去前の必須保全形式

一つの attempt に以下の **実際の Git reachability roots** を作る。role と ordinal
の対応は manifest が持ち、ref 名に filesystem path を埋め込まない。

1. 元 branch tip `H` の commit root。
2. 元 index tree を保持する index commit `I` の root。親は `H`。
   復元対象は staged `(path, OID, mode)` であり、stat cache の物理 bytes ではない。
3. archive commit `A` の root。tree 内の versioned manifest と raw-data subtree
   に全 tracked / untracked の内容を **Git tree entry として** 接続し、`I` を親に
   持つ。manifest に OID 文字列だけを置いて参照済みと見なしてはいけない。
   内容ごとに大量の ref を増やすのでなく、commit root でまとめて保持する。
4. 対象 `T` の commit root。remote-tracking ref が後から変わっても参照を失わない。
5. stash 作成後、識別した stash commit `S` の追加 root。全 parent graph を保持。

manifest は attempt / operation kind、worktree identity、`B/H/R/T`、`I`、承認
stamp、artifact roles、各 relative path の lossless encoding、kind / Git mode、
raw-data entry、明示的 absence を保持する。空 file と削除を混同しない。
symlink は follow せず target bytes を保存する。空 directory 等の非対象も明記する。
archive 読み取りは現行 blob-only `Backend::read_backup` のままでは実現しない。

**`H/I/A/T` の exclusive pin、全 object の readback、元内容との照合が揃うまでは
stash push も checkout も実行しない。** 一部だけ作成できた場合は refs を残して
中止する。`A` だけでも `S` を使わず元の staged 内容 / raw files を復元できることが
必要である。これで stash 作成中の停止、OID 識別不能、追加 pin 失敗を隔離する。

保全は設定にかかわらず mandatory。通常の stash drop、reflog expiry、snapshot cap
では消さない。全 root を `OpLogEntry.backup_refs` と recovery handle に引き渡し、
既存の明示的 retirement のみが寿命を終える。外部からの ref 削除まで防ぐとは約束
しない。ローカルの未 commit データを Git object として保持するため、対象・保存先・
保持期間を確認画面で説明する。remote への送信は行わない。

## Preflight と途中の再確認

承認 token / revision / owner を確認した後、最初の write より前に次を検査する。
途中でも該当境界を再確認する。失敗時に承認なしで新しい値へ追従しない。

- canonical repository / worktree identity、信頼設定、現在の `HEAD = B@H`、
  `R = T`、upstream / remote 設定、branch の他 worktree 占有・登録状態が一致。
  symbolic remote HEAD をそのまま実行 target にせず、具体的 ref と full OID を使う。
- merge / rebase / cherry-pick / revert 等の進行中操作、conflicted index、unborn /
  detached HEAD、read-only / bare repository、既存 reconcile / lease は拒否。
- index の全 entries と対象 bytes が承認 stamp に一致。同じ status のままの編集、
  untracked の増減、file kind / mode の変化も検出する。読み取り不能は除外せず拒否。
- sparse checkout / sparse index、skip-worktree、assume-unchanged、intent-to-add、
  未対応 index flags / stages は拒否。影響する gitlink / nested repository、特殊
  file、安全に round-trip できない path / symlink も拒否し、理由を表示する。
- 外部 clean / smudge / process filter、LFS、未検証の working-tree encoding 等は
  拒否。CLI の filter 無効化だけを復元保証にしない。組込み EOL 変換を許す場合も
  raw archive の検証と復元を必須とし、attributes / conversion 設定を凍結する。
- target の全必要 object がローカルで読める。implicit fetch、部分的な object
  不足の無視、checkout 中の設定差替えはしない。
- ignored を status の外で調査し、target と衝突する path / ancestor / descendant、
  file/directory・case 衝突を拒否。checkout は明示的に
  `safe().overwrite_ignored(false)`。不明な directory 内容を再帰削除しない。
- capture 完了後に source stamp を再確認。stash 後は作成 OID / 保存 tree の意味、
  元 index、untracked、stack baseline、対象の clean 状態を検証する。単に stash が
  list に存在するだけでは続行しない。

Kagi lease と ref locks は外部 Git / editor を排除しない。recheck と write の間にも
race はある。本契約は **承認して capture した内容の保全** と **観測した drift での
停止** であり、同時編集の filesystem snapshot isolation ではない。確認画面で外部
writer を止める必要を明示する。観測できない同時書込みまで「すべて保存」と宣伝
しない。より強い排他を約束する場合は別の実装可能性証明が必要となる。

## 実行順序と commit point

| 段階 | 実行 / 続行条件 | 失敗時 |
|---|---|---|
| 1. Admit / preflight | 一回の lease。immutable plan と source の一致 | write 前なら Refused。自動 replan しない |
| 2. Capture / retain / verify | `H/I/A/T` を作成・pin・readback。耐久性契約を満たす | refs / object の副作用も記録し、保全未完了なら消去しない |
| 3. Stash | dirty/untracked がある場合のみ、attempt marker を付けて既存の supervised stash push を利用 | started を先に evidence に残す。停止不明 / identity 不明なら Unknown、checkout しない |
| 4. Verify stash / pin S | full OID と内容を確認、`S` を pin、source が期待どおり clean | 退避済みでも止める。`A` と全既知 artifacts を残す |
| 5. Safe checkout T | `HEAD/B = H` を再確認し、branch / worktree HEAD ref locks 下で旧 baseline `H` を明示。ignored 保護付き checkout | 部分 checkout を否定できない。実測して Partial / Unknown |
| 6. Conditional branch update | checkout 成功後、held lock 下で `B` の expected-old `H` を再確認し `T` へ更新。symbolic HEAD は `B` のまま | 無条件 ref overwrite へ fallback しない。index/WT が T、B が H の Partial もあり得る |
| 7. Verify | 下記の実測 postconditions と保全の再検証 | 失敗・観測不能を成功にしない |
| 8. Record / settle | 全実在 roots、実測 before/after、stage / uncertainty を一つの final receipt に記録 | append 失敗は別軸。attempted receipt を提示し mutation を再試行しない |

stash CLI が必要とする lock と衝突しないよう、段階 5 の ref locks を stash 全体に
跨いで保持しない。代わりに stash 前後で identity を再確認する。ref lock の取得範囲
と linked-worktree HEAD の扱いは branch-delete の先例を使うが、実機検証が必要。
`execute_checkout_commit` のように一度 detached HEAD にする実装は採らない。
先に branch を動かしてから checkout する順序も採らない。

既存 Backend の stash / checkout / reset をそれぞれ UI command として呼ばず、
同一 executor 内の段階として合成する。独立した child admission / auto-pop /
中間の success toast はない。Git 操作と recording の finalizer を二重化しない。

## Verify、Partial / Unknown、再起動

成功は command exit code ではなく、少なくとも以下の観測結果から決める。

- canonical identity と symbolic `HEAD = B`、`B = T`、index tree / entries が `T`。
- tracked WT が期待する変換規則で clean、退避対象 untracked が残っていない。
  新しい外部編集や検査不能を「揃った」と扱わない。ignored 保護も確認する。
- 全 backup refs が記録予定の OID / type を指し、`H/I/A` と optional `S` が復元に
  十分。元の staged / raw-content stamp と archive の一致を確認する。
- 承認後の remote 更新があっても実行先を差し替えない。成功表示はあくまで
  「承認した `R@T` に揃えた」であり、現在のサーバー状態の追跡ではない。

| 結果 | 分類 / 提示 |
|---|---|
| write 前に drift / unsupported 条件 | Refused、新しい plan が必要 |
| 実行失敗で副作用がないと証明できる | Failed |
| refs のみ作成、stash 後停止、checkout 後 CAS 拒否等で副作用を説明できる | Partial、完了した段階と復旧先を提示 |
| 子 process 停止、stash identity、checkout / ref の結果を確定できない | Unknown が優先。再実行・自動 rollback はしない |
| postconditions は成立したが oplog append 失敗 | Git outcome と `Recording::Failed { attempted, error }` を分離。記録失敗を隠す成功 toast は出さない |

既存 `apply` は既知の Partial を自動で reconcile 待ちにしない。sync では、checkout
途中など今後の write 前に確認が必要な Partial を family evidence で明示し、既存
reconcile owner に接続する。Unknown は stop proof 前に lease を解放しない。
停止が証明されても、必要な状態照合と acknowledgement までは scope を unblock しない。

`A` の manifest は cleanup 前の durable intent / 復旧索引でもあるが、それだけで
再起動処理は完成しない。既存 Sessions は起動時に空になり、oplog tail を表示する
だけで in-flight 状態を復元しない。以下を sync 有効化の必須条件とする。

- repository attach 時に sync archive と terminal receipt を attempt ID で照合し、
  未決着の保全物を既存 reconcile に登録。oplog の tail や `parent` sequence だけで
  判定しない。未記録・forget 後の orphan は区別不能なら「結果未確認」とする。
- writer の OS identity / 停止証明を再起動後にも扱えること。単なる PID 不在、
  manifest の存在、Kagi の再起動を停止証明にしない。現行 in-memory supervisor
  だけでは不足する。証明できなければ read-only の復旧案内に留め、解除しない。
- 自動 replay、stash 再実行、自動 ref cleanup はしない。capture 途中の orphan も
  保持する。process crash の fault injection で保全と再入場拒否を検証する。
- ODB / refs / manifest の同期・durability 順序を確認する。GC 耐性だけで電源断
  耐性を主張しない。電源断まで保証する場合はその保存経路の検証が別途必要。

## 復旧と UI

入口は **現在のローカル branch の Advanced / Dangerous menu** に
「ローカルを退避してリモートに揃える…」を追加する。Pull、Switch to latest、
ref-only Reset の名称・意味を置換しない。upstream 未設定等は理由付きで拒否する。
EN / JA の `Msg` と typed plan 文言を追加し、対象選択は現在 branch + upstream に絞る。

plan card は `B@H → R@T`、取得済み状態であること、観測時刻、branch から外れる
commit、staged / unstaged / untracked、残す ignored、保全場所・保持期間・復旧方法、
外部 writer の注意を示す。fetch 時刻を取得できない場合は「不明」とし捏造しない。
一つ目の確認で arm、二つ目の明示的 destructive confirm で同じ revision を承認する。
再 plan / fetch / selection / owner 変更で disarm。失敗した plan に古い確定ボタンを
残さない。実行前の cancel は write なし、実行中 cancel は停止要求であり undo ではない。

完了時は owner `SessionId + visit` に delivery。別タブへ切り替わっていても receipt /
記録失敗 / reconcile notice は保存し、active pane を置換しない。shared refs を変更
するため同一 repo の sibling worktrees も既存経路で invalidate する。bounded toast
は要点のみ、全 artifact・段階・error は oplog が持つ。

復旧は「元の branch を無条件に戻す」ではなく、保存 `H` から新しい recovery branch /
worktree を作り、元の index と files をそこで復元する別の承認済み write とする。
元の作業場所や sync 後の追加編集を上書きしない。archive 読み取り・安全な path
復元・index 復元は既存 Backend / backup owner の拡張とし、復旧を備えるまで sync
だけを先に提供しない。

- `S` が検証済みなら、通常 Git の index-aware stash apply を **exact backup ref /
  OID** で案内できる。生成する recovery commands は実値を shell-quote する。
  `stash@{n}`、pop、stash drop を復旧手順に使わない。
- stash apply だけでは raw bytes / encoding の忠実な復元を保証しない。`A/I` を
  authoritative とし、新しい復旧先の expected preimage を確認して raw bytes /
  symlink / absence / modes と index entries を復元・照合する。
- `S` が作れなかった場合も `H/I/A` から同じ復元が可能でなければならない。
  recovery root / ref の OID 変更、path traversal、symlink 経由の外への書込み、
  復旧先の外部編集は拒否する。途中失敗も Partial / Unknown として記録する。
- 必須 root は成功時も保持。容量解放は明示的 oplog retirement でのみ行い、
  「この復旧手段が失われる」ことを確認する。

## 実装開始条件と検証方針

以下は本 docs-only PR の実装物ではなく、実装 PR で満たす acceptance criteria。
既存 API がこれらを満たすと見なして着手・有効化してはいけない。

1. **Capture / restore:** partial staging、削除、binary untracked、mode、symlink、
   EOL を含む元 index / raw 内容を `H/I/A` だけで復元できる。stash 呼出し前に
   archive が完全であることを fault injection で示す。
2. **Protection:** same-status 編集、ignored / directory / case 衝突、全 unsupported
   条件、capture 中 drift を拒否。外部同時 writer の保証限界を UI と一致させる。
3. **Mutation:** old checkout baseline、ignored overwrite 無効、locked expected-old
   branch 更新を linked worktree でも検証。checkout 失敗・CAS 拒否時に自動 rollback
   せず、正しい実測 after と残存 artifacts を記録する。
4. **Lifecycle:** 各 phase 前後の失敗 / unwind / 子 process 停止不明、二重 confirm /
   settle、background owner、append 失敗を検証。一回の admission と recording
   finalizer、Partial / Unknown の再入場制御を確認する。
5. **Durability:** stash drop / reflog expiry / GC 後の復元、capture / cleanup / ref
   update / append の境界での process crash、再 attach、未決着 writer、明示的
   retirement の shared roots を検証。durable stop identity がないまま自動解除しない。
6. **UI:** EN / JA、二段階確認、stale revision、disabled 理由、no-op、復旧案内を
   actual native surface で確認。operation tests は既存 feature integration の流儀に従う。

### 設計調査時に確認できたこと

隔離した throwaway Git repository で、次の有限実験を実行し、全 assertion が通った。
元 HEAD / index commit / raw blobs / manifest を先に pin し、stash push 後に stash
commit も pin。通常の安全な checkout と expected-old ref 更新で target に揃え、
stash drop・全 reflog expiry・GC の後、新 branch で pinned stash を index-aware
apply した。部分 stage、未 stage の削除、binary untracked、元 index entries / status /
raw bytes の一致、非衝突 ignored の維持、stale CAS の拒否を確認した。到達不能な
control object が GC で消えることも確認した。

この実験は標準 Git primitives の正常系・到達可能性の証拠に限る。実験では
CLI の detached checkout を介し、raw blobs を個別 pin したため、本案の attached
HEAD executor / archive commit packing の実装証明ではない。filters、外部競合、
途中失敗、native UI、再起動後の writer 停止、電源断は検証していない。
アプリの build / tests は本 docs-only 変更の検証対象ではない。
