# T-COMMIT-010: Amend — backend(plan_amend / execute_amend、3 モード、SHA 変化)

- Status: todo(ADR-0040 決定済み: MVP=未push のみ、pushed は blocker。案C は v0.2 設計)
- 依存: ADR-0040 / 0023(2段階確認)/ 0039・0043(checklist)/ 既存 `execute_commit`
- 関連: lane W14-AMEND

## 背景

直前 commit の amend(message only / staged / both)。SHA を変える history-rewriting。new commit + ref 移動で
in-memory・ref-order 規則に整合。pushed 扱いは ADR-0040 の決定を待つ。

## スコープ(ADR-0040 厳守)

- `pub fn plan_amend(repo, mode: AmendMode, message: Option<&str>) -> Result<OperationPlan, GitError>`
  - `AmendMode { MessageOnly, Staged, Both }`
  - blocker: detached / unborn / **merge commit(parents>1)** / root commit の staged 畳み込み不可ケース /
    checklist(ADR-0043)の block。**pushed の扱いは ADR-0040 決定に従う**(案 A=warn / 案 B=blocker)。
  - predicted に **`旧 <short> → 新 <short>`(SHA 変化)** を明示。`destructive: true`。
- `pub fn execute_amend(repo, mode, message) -> Result<AmendOutcome, GitError>`
  - parent = 旧 HEAD の親(据え置き)。tree = message only なら旧 tree、staged 含むなら `index.write_tree_to`。
  - `repo.commit(None, ...)` で object 作成 → blocker なし確認後 `repo.reference("refs/heads/<branch>", new, true, msg)`
    で **ref を最後に**移動(ref-order 規則)。WT / index は触らない(staged は write_tree_to で読むのみ)。
  - author 保持 / committer 更新。`AmendOutcome { old: CommitId, new: CommitId }`。
- 実行前に **旧 HEAD SHA を oplog に記録**(before/after HEAD)。

## 完了条件

- [ ] 3 モードで新 SHA の commit ができ、ref が新 commit を指す(WT 不変)
- [ ] message only で tree が旧 HEAD と一致 / staged で staged が畳み込まれる
- [ ] merge commit / detached / unborn で blocker
- [ ] pushed 判定が ADR-0040 の決定どおり(案確定後)
- [ ] checklist の block/warn が反映される
- [ ] oplog に旧 HEAD SHA(before/after)が記録される
- [ ] unit test: 3 モード正常 / 各 blocker / round-trip 復元(reflog or 旧 sha から)、計 7+
- [ ] `cargo test` 全パス + own-code warning 0、`commit.amend` / checkout 系 / reset --hard を使っていない(grep)
- [ ] 実装メモを本ファイル末尾に追記

## 触ってよいファイル

- `src/git/ops.rs`(plan_amend)/ `src/git/staging.rs`(execute_amend、write_tree_to)/ `src/git/mod.rs`
- `tests/amend_test.rs`(新規)
- `docs/tickets/T-COMMIT-010.md`

## 触ってはいけないファイル

- `src/ui/*` / `src/main.rs`(UI は PM)/ 他チケットのファイル / `Cargo.toml`

## テスト方法

1. `cargo test`(exit code 確認)
2. fixture / tempdir のみ。upstream 付き fixture で pushed 判定を検証
3. grep で `commit.amend` / `reset --hard` / checkout 系の不使用を確認

## リスク・規約

- ref-order 規則厳守(object を先、ref は最後)。in-memory 主義(WT に書かない)
- **ADR-0040 が Proposed の間は着手しない**。決定後に blocker(案 B)or warn(案 A)を確定
- 文字列切り詰めは `chars()` ベース

## 実装メモ(2026-06-13、lane W14-AMEND backend)

- `src/git/ops.rs` に `AmendMode { MessageOnly, Staged, Both }` / `AmendOutcome { old, new }` /
  `plan_amend(repo, mode, message: Option<&str>)` / `execute_amend(repo, mode, message)` を追加。
- `OperationPlan` に `destructive: bool` フィールドを新設(全構築点を `false` で更新、amend のみ `true`)。
- execute は ADR-0040 の手順そのまま: parent=旧 HEAD の親(据え置き)、tree は message-only=旧 tree /
  staged=`index.write_tree_to(repo)`(in-memory、WT 不変)、`repo.commit(None,..)` で object 先作成 →
  blocker なし確認後 `repo.reference(branch, new, true, ..)` で **ref を最後に**移動(ref-order 規則)。
  `commit.amend` / `checkout_tree` / `set_head` / `reset` 系は不使用(grep 確認済み)。
- author 保持(旧 commit の author)/ committer は `build_signature` で更新。
- blocker: detached / unborn / conflict / merge(parents>1)/ root(parents==0)/
  **pushed(graph_descendant_of(upstream, head) — undo_commit と同型)** / mode 別(message 空・staged なし)。
  pushed は ADR-0040 案B(blocker)。
- predicted に `旧 <short> → 新 <new>` を明示(新 short は execute 後決定のため `<new>` プレースホルダ)、`destructive: true`。
- checklist(ADR-0043 `run_checklist`)は W14-CHECK lane 未着手のため step 5 に TODO コメントで仮置き
  (結合は PM が merge 時)。message 空 / staged なし / conflict の構造 blocker は本実装で先行。
- 旧 HEAD SHA は oplog に記録: `record_op("amend", before, Success{after: "... (was <old>)"})`(UI confirm 経路)。
- tests: `tests/amend_test.rs` 13 本(message-only / staged / both / author 保持 / pushed blocker /
  merge blocker / detached blocker / root blocker / message 空 blocker / staged なし blocker /
  round-trip(reset --hard <old>)/ preflight 不一致 / upstream なし許可)。全パス。
- headless: `KAGI_AMEND=<message|staged|both>` + `KAGI_AMEND_MSG=<text>` + `KAGI_AUTO_CONFIRM=1`
  (2段階 confirm を arm→execute で自動駆動)。検証済み: message-only=旧 tree 維持・author 保持・committer 更新、
  staged=畳み込み・msg 据え置き・WT clean、pushed=blocker で skip。
