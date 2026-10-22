# ADR-0067: Continue / Abort / Skip Safety Policy

- Status: Accepted(2026-06-13)
- 関連: requirements-conflict-ux.md §2.6 / ADR-0056 / 0066 / 既存 plan_conflict_continue/abort(W26)

## Decision

Conflict Mode 中の Continue / Abort / Skip / Mark resolved / Save は **直接実行せず Plan 経由**
(GitOperationPlan または ConflictOperationPlan → confirm → preflight → execute → verify → oplog)。

### Continue 前チェックリスト

UI は現在の buffer から unresolved / marker 等を投影して Continue を表示する。
確認の計画と実行は repository-bound な正規 checklist を再評価し、表示 cache を認可にしない。

- unresolved files count == 0
- conflict marker 残存なし(ADR-0066)
- session が扱わない unmerged index entry なし。session 内の明示解決は Continue で stage できる
- binary conflict 残なし
- required file が削除されていない
- commit message 非空(merge commit を作る場合)
- checklist(ADR-0043)blocker なし

### Continue の実行
- merge: 解決を index へ準備し、既存 Commit Panel で message と merge commit を確認する
- rebase/cherry-pick/revert: 解決を stage して sequencer continue(次の step へ)

### canonical index と prewrite の境界（#1721）

- `continue_blockers(repo, session, buffer) -> Result<Vec<ContinueBlocker>, GitError>` を
  plan / route / execute で共有する。長寿命 Backend が別 handle の Save 前の index を
  保持し得るため、canonical read は `index.read(true)` で必ず disk から更新する。
- index の取得・再読込・conflict iterator の生成・各 entry の読取失敗を伝播する。
  失敗を空 checklist、古い cache、marker-only 判定へ置き換えない。
- 共有 `stage_conflict_resolution` が WT/index 準備の直前にも同じ checklist を再確認する。
  sequencer の公開 execute と merge Commit Panel 向け準備のどちらもこの境界を通り、
  読取不能または session 外の unmerged path があれば書込前に拒否する。
  Save 済みだから通す例外や、通常の coverage 判定の緩和はしない。
- session の operation が現在と異なる場合、または解決済み path の既存 file identity
  （canonical gitdir / operation context / stage の有無・OID・mode）が live index と
  一致しない場合は、既存 `ObservationChanged` として書込前に拒否する。同じ path が
  次 conflict に存在するだけでは旧 Result の権限にならない。旧 session の path が
  既に stage 0 なら再 materialize せず再読込を要求し、fresh な空 session の Continue は許す。
  空 session でも必要な operation metadata の既知性を先に検証し、不明なら
  index/tree 作成前に拒否する。path が空の場合に gitdir の canonicalize は行わない。
- text / raw の直接 Save も fresh index と同じ file identity を書込前に照合する。
  recorded Save の既存 `BufferRevision` は payload と producer の file identity を
  一緒に凍結し、plan / run の global observation 検証に加えて照合する。現在の
  observation を旧 payload に添えても新 identity を無条件に与えない。
  無関係な index entry の更新は per-file identity を変えない。

### Abort(常に可能 = 安全弁)
- 確認ダイアログを出す。**保存済み resolution が失われる可能性**を明示
- `cleanup_state` + ORIG_HEAD 復帰(force/reset --hard/clean 不使用、ADR-0056/0057)。buffer は
  oplog 参照付きで退避(完全には消さない)

### Cherry-pick / revert sequence abort の開始点 (#1127)

`sequencer/head` が sequence 全体の開始 HEAD。先行 pick 済みの現在 HEAD や、
cherry-pick が更新しない古い ORIG_HEAD へ戻さない。開始記録を conflict revision
に含め、確認後の差替えを拒否する。live sequencer の開始記録が欠損・不正、
または commit を読めない場合は EN/JA の blocker で mutation 前に拒否する。
`sequencer/abort-safety` は現在 HEAD と一致することを要求する（Git の
`rollback_is_safe()` と同じ安全基準）。不一致なら手動 commit 等で HEAD が
移動したため巻き戻さないと EN/JA で拒否し、欠損・不正も mutation 前に拒否する。
安全基準も conflict revision に含め、承認後の差替えを拒否する。
sequencer を使わない単一 pick は現在 HEAD が開始点。復元は既存の touched-path
checkout・index 復元・ref CAS と buffer 退避・receipt を共有し、適用結果の
手編集 guard は現在 HEAD（先行 pick を含む）へ replay して再構成する。


### Rebase abort の比較基準 (#534)

復元先は ORIG_HEAD のまま。非競合ファイルの手編集保護に使う適用結果は、
`base = REBASE_HEAD の親`、`ours = 現在の HEAD` (移動先 + 適用済みコミット)、
`theirs = REBASE_HEAD` で再構成する。移動先から入った正常な差分を手編集と誤判定しない。
本当の staged / unstaged 編集は従来どおり abort を拒否する。
復元後は rebase directory に加え、libgit2 が残す `REBASE_HEAD` も削除する。

### Skip
- **rebase / cherry-pick / revert のみ**(merge では非表示)。sequencer の現 step をスキップ
- W26 段階では未実装(UI は「terminal で `--skip`」案内)。本 ADR で正式 plan 化を予約
