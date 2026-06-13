# Requirements: Branch Context Menu(Branch List Pane 右クリック)

- Status: Accepted(2026-06-13、ユーザー発案・本文はユーザー原文準拠で再構成)
- 関連 ADR: 0049〜0055 / 既存: 0020〜0026(commit context menu)、0014(delete branch)、0040(amend/force 系)、0046(discard)
- 実装 lane: codex 5.5 high(T-BCM-*)

## 目的

Branch List を「表示」から「branch 操作の起点」へ。GitKraken / Fork のように右クリックで
checkout / push / pull / set upstream / merge / worktree / rename / delete / copy 系を出し分ける。
**merge / rebase / delete / force 系は Context Menu から直接実行せず、必ず GitOperationPlan
(plan→確認→preflight→execute→verify→oplog)を経由する。**

## 要件一覧(ユーザー指定)

### R1 基本
- local / remote / current branch を右クリックできる。folder/group 行は folder 向け menu または no-op
- 右クリック対象を selection state に反映
- 種類と状態で出し分け。実行不可項目は非表示 or disabled + 理由(tooltip/補助表示)
- 状態変更は GitOperationPlan 経由、結果は Operation Log に記録
- 実行後 snapshot / graph / sidebar / header / status bar を refresh
- Right Panel / Header Toolbar と**同じ operation handler を使い二重実装しない**

### R2 Menu 構成(group 順)
1. Checkout/Open: Checkout branch / (detached later) / Open worktree from branch /
   Reveal branch HEAD in graph / (Solo・Hide・Pin later)
2. Sync: Pull / (Pull ff-only) / Push / Push and create upstream / Set upstream /
   (Fetch remote branch later / Create PR later)
3. Integrate: Merge <target> into <current> / Rebase <current> onto <target>(ADR・plan 設計のみ MVP)/
   (逆向き rebase・interactive later)
4. Create: Create branch from here / Create worktree from here / Create tag here / (annotated later)
5. Manage: Rename branch / Delete branch / Copy branch name / Copy branch HEAD SHA / Copy upstream name
6. Advanced/Dangerous(MVP は disabled or ADR のみ): Reset current to this HEAD later /
   Force-with-lease push later(ADR-0040 案C)/ Delete remote branch later

### R3 local branch(non-current / current の出し分け)— ユーザー表のとおり
- current では Checkout disabled or 非表示、Delete current は disabled
### R4 remote branch
- Checkout as local branch(= local tracking branch 作成を優先。detached は Advanced)
- Create local tracking branch / Pull・Fetch / Create branch・worktree from remote /
  Merge remote into current / Rebase current onto remote / Copy 系 / Delete remote(later/dangerous)
### R5 upstream / ahead-behind 出し分け
- upstream あり: Pull enabled iff behind>0(0 なら no-op 表示)、Push enabled iff ahead>0、
  Set upstream は現 upstream 表示つき、Copy upstream name enabled
- upstream なし: Pull disabled / Push は「Push and set upstream」/ 未設定であることを menu 内に表示
- count 表示: `Pull ↓3` / `Push ↑2`
### R6 dirty working tree
- checkout / merge / rebase / pull / delete / rename current に warning。
  plan に dirty state を含め stash 提案を表示。Set upstream・Copy 系は safe
### R7 merge / rebase 文言
- `Merge <target_branch> into <current_branch>` / `Rebase <current_branch> onto <target_branch>`
  (対象と方向を必ず文言に含める)。merge は ff 可否表示 + conflict 予測(in-memory)。
  rebase は pushed commit 含みで warning/blocker、protected branch 禁止、conflict 時 Conflict Mode
### R8 rename / delete — ADR-0053 のとおり(local のみ、validation、current rename 可否は ADR 決定、
  remote は自動 rename しない。delete は merged-only guard / current 不可 / unmerged は blocker)
### R9 worktree — path 入力、衝突防止、別 worktree checkout 済み警告、作成後 WORKTREES refresh、plan 経由
### R10 UI/UX — menu 最上部に branch 名、current に ✓、種類の視覚区別、危険は下部の赤系 group、
  Copy 系は下部、文言は対象と方向を明示(`Checkout feature/foo` 等)
### R11 availability 判定は純粋関数
```rust
fn branch_context_menu_items(ctx: &BranchMenuContext) -> Vec<MenuGroup>
```
判定入力: local/remote / is_current / has_upstream / ahead / behind / dirty / conflict mode /
protected / checked out in another worktree / merged into current / pushed / detached HEAD /
operation in progress。**UI component から直接 Git 状態判定をしない。unit test 可能にする**

## MVP 範囲(ユーザー指定)

実装: 右クリック menu / Copy name・HEAD SHA / Reveal in graph / Checkout / Create branch from here /
Pull・Push・Push and set upstream / Set upstream / Open worktree from branch /
Merge selected into current(plan)/ Rename local / Delete local(safety check 付き)

後回し: interactive rebase / delete remote / force-with-lease / AI explain / Hide・Pin・Solo /
Create PR / annotated tag / submodule / branch folder menu

## 完了条件(ユーザー指定)

- local / remote 右クリックで menu。current・upstream 有無で項目が変わる
- Pull/Push に ahead/behind count。Merge/Rebase の方向が文言で明確
- 危険操作は plan 経由・current delete 等は disabled
- dirty 時に checkout/merge/rebase/pull で warning
- 操作後に snapshot と UI が refresh。availability 判定に unit test
