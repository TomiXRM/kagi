# ADR-0176: local stash family の application boundary

状態: 採用・PR 1 実装済み（#541）、owner lifetime 補強（#485）、remote Drop 拡張（PR 2）
日付: 2026-09-07
関連: #484、[FAMILY-stash](../rearch/app-layer/FAMILY-stash.md)、
[ADR-0175](0175-app-remove-boundary.md)、[ADR-0149](0149-oplog-in-backend-run-and-schema.md)、
[ADR-0148](0148-stash-conflict-resolution.md)、[ADR-0097](0097-remote-stash-drop.md)

## 決定

local push/apply/pop/drop と remote Drop は既存 `Sessions` の単一 plan slot と lease を使う。
`Planned` / `Job` / `Completion` / `FamilyEvidence` は remove と stash の有限 enum。
共通 `ExecutionReport` は実 receipt と family evidence を配送し、remove の progress、
target_exists、停止不明を欠落させない。新 controller / worker / crate は設けない。
承認 revision と policy は prepare 前に照合し、一回消費する。Busy 後も新 plan が必要。
host は引き続き KagiApp。spawn・settings・localization は app 層に入れない。

`Backend::run` は互換 facade として result を返す。
`run_recorded` は同じ `backend/run.rs` の dispatch に実 verify と一回の finalize を接続し、
append が採番した entry/path または attempted entry/error を返す。
非 stash mapper は #524 の partial_after を含め変更しない。remove の Recording も
同じ小さな recording module を再 export し、共通 finalize を使う。
fresh-open 失敗と abandon は同じ finalizer への材料生成のみ。
実行・verify unwind は finalize の内側で捕捉する。配送や重複 apply で append し直さない。

stash plan error は未記録 completion。revision 採用時だけ非 Clone の記録 job を発行し、
採用後の modal/revision の終了とは独立に完了させる。旧 completion は記録しない。
runtime preflight drift は未変更 Refused（旧 local stash の Failed を変更）。
plan blocker、Busy、StaleApproval と区別し、runtime refusal の理由は EN/JA の
既存 preflight 表現で toast/footer/共通通知 modal に出す。

## 対象と観測

plan の full OID 列は順序・重複を保持し、count / index / 選択 OID と同じ読取に束縛する。
既存 HEAD / status classification digest に追加して実行直前に照合する。
push も status digest と message/include_untracked を凍結する。
pop の apply 後は list を再照合してから drop。不一致なら変更済み Partial、未変更拒否に戻さない。
conflict は stash を保持し、receipt / JSONL / 非緑通知が同じ Partial を表す。
conflict evidence は当該 Apply/Pop が生成したものだけとし、既存 conflict 中の独立 Drop は
list / WT / index verify が通れば Success。別操作の継続 payload を置換しない。

verify は list と実 status/index を読む。clean apply/pop は in-memory three-way merge と
実 worktree の diff、untracked stash tree を照合する。drop は返却 OID、前後 ordered list、
WT bytes/index fingerprint を確認し、auto-snapshot を作らない。
push は新規 OID、tracked clean と include_untracked 方針を確認する。
snapshot OID / 適用済み / 観測 after を副作用の evidence とし、verify failure は
Partial、実行中 unwind は Unknown。停止済み Unknown も read→ack 前は write 不可。
有限 fault は doc-hidden integration-test API と既存 uv fault gate を使う。
report は実際の停止境界（open / identity / trust / blocker / preflight / abandon）を保持する。
plan に blocker があるだけでは blocker gate 到達とみなさず、receipt と klog lane を実停止理由に揃える。

### #622: push の実行エンジン

push のみ既存 `cli::run_git` の hardened `git stash push` を使う。libgit2 の
untracked tree 構築は変更していない tracked bytes まで tree→workdir diff で
再走査し、2ファイルの独立cloneで約12.5秒（Git CLI は351ms）を要した。
plan/preflight/verify/recording は変更しない。CLI 後は libgit2 の cached index を
再読込する。stdout の文言ではなく `refs/stash` の新OIDと既存 verify で結果を確定する。
署名は従来の config/fallback を渡し、repo hooks は既存 runner が無効化する。
external clean/smudge/process filters は libgit2 と同じく実行しないよう無効化する。
期限切れ・不完全capture の `TerminationUnknown` は family evidence に伝播し、
receipt/GUI/admission 全てで Unknown として read→ack を要求する。自動再試行しない。

## GUI と conflict 継続

4 modal を Planning で開き、Ready だけ承認可能。debounce 入力更新で即 revision 無効化。
raw Enter / button は同じ confirm → approve → dispatch を通す。
pop Enter に started/finished を追加する承認済み契約変更以外は既存 klog 列を保つ。
local stash の UI record_op / finish_op_on_main / blocking core は撤去した。
PR 2 で remote 分岐の finish/record/direct transport 呼出しも撤去し、typed Remote job と
`WriteScope::Remote(RemoteRepoId)` に移管した。remote の停止不明は completion token + read の
ack まで同じ lease を保持する。local/remote とも共通 `ExecutionPolicy` を approval に束縛し、
remote receipt も `recording::finalize` 以外から append しない。

conflict 継続 payload は canonical worktree ごとに operation id、full OID、HEAD と
index conflict sides の evidence を保持し、表示 guard より先に保存する。
別 linked worktree に流用しない。再検出時に同じ conflict sides の連続性を確認できなければ
破棄する（解決済み paths の減少は許す）。abort/終了/置換で clear。
continue 成功時だけ pending とし、空 conflict の観測時に active conflict map から
owner 別の one-shot follow-up slot へ移す。pending conflict 自体は空観測を跨がない。
reload 後に一意 OID を新 plan に束縛する。
候補 0 / 複数 / 読取間 drift なら prompt を出さない。新確認の取消は stash を保持する。
tab close は同じ owner の active conflict と未提示 follow-up の両方を破棄する。
close が continue 後の reload より先でも後でも、再open に提案を持ち越さない。
外部 Git / 再起動後など出所不明の conflict に自動 drop は提案しない。

## 保証しないもの・検証

app lease は外部 Git を排他しない。最終照合と index 指定 libgit2 drop の間の競合窓、
観測値が完全に同一に戻る ABA、OID の GC 後保持は保証しない。
conflict/follow-up payload は in-memory で、tab close と再起動を跨いで永続化しない。
CLI/MCP tail 撤去は #505。
保証するのはアプリ内 close/Quit 入口の保留のみ。Dock/OS Quit は GPUI の veto API がなく保証外。

G は同じ公開 plan/approve/prepare/run/apply 列を window なしで駆動する。
E は実 modal / focus / current-window bounds で 4 op の両入力と conflict/replan failure を駆動し、
window remove と entity drop で後始末する。E は PM 実行の全 PASS と exit 0 が必要。
ただし GUI の conflict continue 後の follow-up 提示は E 未検証で、#546 で追跡する。
payload から一意な新規 Drop plan が Ready になるまでの application 列は G で検証する。
G は pending→空観測の one-shot 移管と、continue→close（reload 前後）→再open で
prompt が残らないことも検証する。E は実 KagiApp の continue と同一 turn の tab close、
再open 後に payload/modal が無いことを検証する。
旧 stash conflict/pop/oplog の実 mutation assertions は保持し、preflight outcome assertion のみ
承認済み Refused に変更。remove 24 件・1b admission 8 件は既存 assertion を変更しない。
