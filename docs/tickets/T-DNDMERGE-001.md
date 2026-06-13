# T-DNDMERGE-001: Drag-and-drop branch merge (start merge by dragging a branch label)

- Status: todo
- Group: 新機能 / sidebar + merge
- 仕様の正: ADR-0079 + this ticket. Reuses the existing merge pipeline
  (`open_merge_modal` → `Backend::plan_merge_branch` → `MergePlanModal` →
  `execute_merge_branch` / Conflict Mode).

## 背景 / 既存経路(調査済み)

- 既存 merge トリガ: `KagiApp::open_merge_modal(target: String)`(`src/ui/mod.rs:5630`)
  が `Backend::plan_merge_branch(&target)` を呼び、`MergePlanModal` を開く。`target` は
  「HEAD に merge する側」= source。現在は branch context menu からのみ起動。
- preflight / ff判定 / conflict予測 / blockers は `plan_merge_branch` が実施済み。
  実行・conflict mode 遷移も既存(`execute_merge_branch` / `execute_merge_into_conflict`)。
- sidebar の local branch label 描画: `src/ui/sidebar.rs::render_sidebar`(LOCAL BRANCHES
  セクション、leaf 行)。既に `on_mouse_down(Right)` で context menu を開く。
- GPUI drag の既存例: `.on_drag(payload, |_,_,_,cx| cx.new(|_| Ghost))`(pane divider:
  `mod.rs:10443` ほか)。drop 側 `drag_over::<T>` / `on_drop::<T>` は本機能で新規導入。

## スコープ(ADR-0079 の4層、UI に git を書かない)

1. **UI drag 層**(`sidebar.rs`)
   - LOCAL BRANCHES の local branch leaf 行を `.on_drag(BranchDrag { name }, ghost)` で
     draggable に。drag 中は branch 名が分かる ghost chip を表示。
   - remote branch / tag / folder(group)行は draggable にしない。
   - 既存の左クリック(jump)/右クリック(context menu)を壊さない。
2. **drop target**
   - MVP の drop target = **current(checkout 中)branch の行**(チェック付きの行)。
   - `.drag_over::<BranchDrag>(…)` で drop 可能時のハイライト(valid)を表示。
   - 同一 branch を自身にドロップ等の不可ケースは reject 表示(可能なら hover で、
     最低でも drop 後に理由を出す — `plan_merge_branch` の blockers が権威)。
3. **action 層**(`KagiApp::start_merge_from_drag(source: String)`、`mod.rs`)
   - drop event はこの 1 メソッドにディスパッチ(view から git を直接呼ばない)。
   - 検証: source が実在する local branch / source != current / `busy_op` なし。
   - OK なら `self.open_merge_modal(source)` に委譲(以降は既存パイプライン)。
4. **planning / execution / conflict 層** — 既存を再利用(新規 git コード禁止)。

## merge dialog 表示(既存 MergePlanModal を流用、文言確認)

source / target / strategy / fast-forward 可否 / merge commit 生成可能性 / conflict 可能性 /
実行される Git operation 概要 / `Cancel` / `Merge <source> into <target>`(曖昧な OK/Apply 禁止)。
既存 modal に不足があれば最小限補う。

## 完了条件(受け入れ条件)

- [ ] local branch label を drag できる(remote/tag/folder は不可)
- [ ] drag 中に dragged branch 名が UI でわかる
- [ ] valid な drop target で drop 可能を視覚表示、invalid で不可を視覚表示
- [ ] drop で merge preview dialog(MergePlanModal)が開く
- [ ] dialog に source / target が正しく表示される
- [ ] confirm するまで merge は実行されない(drop 即実行しない)
- [ ] source == target は拒否される(理由表示)
- [ ] dirty working tree は拒否または明示警告(`plan_merge_branch` の挙動を踏襲)
- [ ] merge 成功時に commit graph / branch state が更新される
- [ ] conflict 発生時に既存 Conflict Mode へ遷移(abort / continue 可能)
- [ ] cancel 時に repository state が変わらない
- [ ] UI event から git を直接呼ばず、`start_merge_from_drag` → `open_merge_modal` →
      Backend(planning)を経由している
- [ ] `start_merge_from_drag` の検証 + 生成される merge plan の最低限の unit/integration
      test(fixture/tempdir):same-branch 拒否、dirty-WT、ff vs merge-commit
- [ ] `cargo test --workspace` 全パス + `grep -rE 'git2::|Repository::open' src/ui` = 0

## 規約

- UI に `git merge` 相当を直書きしない(ADR-0078/0079)。git は `Backend` 経由のみ。
- 文字列は i18n `Msg` 経由(ADR-0048。branch 名・domain word は英語のまま)。色は `theme()` 経由。
- fixture / tempdir のみで検証。実 repo に書き込むテスト禁止。
- 既存 branch/label 描画ロジックに merge 操作の責務を混ぜない(drop は intent を emit するだけ)。

## やってはいけないこと

drop 即 merge / UI に git merge 直書き / dirty WT 無視 / conflict 時に状態破壊 /
安全確認を後回しにした「とりあえず動く DnD」。

## Implementation memo

(担当 agent が完了時に追記)
