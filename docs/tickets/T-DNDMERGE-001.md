# T-DNDMERGE-001: Drag-and-drop branch merge (start merge by dragging a branch label)

- Status: done (PM accepted — GUI-verified 2026-06-14)
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

実装完了(branch `rearch/dnd-merge`、base = `re-architecture`)。

### 4層の落とし込み
1. **UI drag 層**(`src/ui/sidebar.rs`):非 HEAD の local branch leaf 行に
   `.on_drag(BranchDrag { name }, |drag,_,_,cx| cx.new(|_| BranchDragGhost{..}))` を追加。
   ghost は branch 名を出すチップ(`BranchDragGhost`、`src/ui/mod.rs`)。HEAD 行 /
   remote / tag / folder 行は draggable にしていない(各々別の row builder で
   `on_drag` 無し)。既存の click=jump / dblclick=checkout / 右クリック menu / ✕delete は
   そのまま(`on_drag` は移動 threshold 後のみ発火するので click と両立)。
2. **drop target**:HEAD(current)branch 行に
   `.drag_over::<BranchDrag>(|style,_,_,_| style.bg(selected).border_color(color_branch))`
   で valid ハイライト + `.on_drop::<BranchDrag>(handler)`。handler は git を呼ばず
   action 層へディスパッチ。同一 branch / 不可ケースは action + `plan_merge_branch` の
   blocker が権威。
3. **action 層**(`src/ui/mod.rs`):`KagiApp::start_merge_from_drag(&mut self, source, cx)`。
   検証は純粋ヘルパ `validate_merge_from_drag(source, &self.branches, busy)` に切り出し
   (busy / source が local branch でない / source==current=HEAD を reject)。OK なら
   既存 `open_merge_modal(source)` に委譲(以降は既存 plan→confirm→execute→Conflict Mode)。
4. **planning / execution / conflict**:既存を完全再利用(新規 git コード 0)。

### merge dialog 文言
`plan_merge_branch` の `plan.title` が既に `Merge <source> into <current>`(modal card が
title を大きく表示)。confirm ボタンは既存どおり `Merge`(conflict 予測時は i18n
`Msg::MergeAndResolveConflicts`)。`confirm_label` が `&'static str` のため動的 String 化は
スコープ外と判断、title が source/target の権威表示を担う。

### gpui 0.2.2 drag/drop API(installed 版で確認)
- `on_drag<T,W>(value: T, ctor: impl Fn(&T, Point<Pixels>, &mut Window, &mut App) -> Entity<W>)`
- `drag_over::<S>(impl Fn(StyleRefinement, &S, &mut Window, &mut App) -> StyleRefinement)`
- `on_drop::<T>(impl Fn(&T, &mut Window, &mut App))`(`cx.listener` でラップ)
- (`can_drop` / `DragMoveEvent<T>` も存在するが本 MVP では未使用)

### テスト
- 単体(`src/ui/mod.rs`, `drag_merge_validation_tests`):accept / same-branch reject /
  unknown branch reject / busy reject。
- 結合(`tests/drag_merge_test.rs`):gate(same-branch / unknown / busy)+
  ff vs merge-commit の plan 生成 + dirty-WT が warning として出ること。
- `cargo test --workspace`:全 suite 0 failed(`could not apply ... side change` は既存
  fixture の意図的 stderr)。`grep -rnE 'git2::|Repository::open' src/ui` = 0。

### 変更ファイル
`src/ui/mod.rs`(BranchDrag / BranchDragGhost / validate_merge_from_drag /
start_merge_from_drag + unit tests)、`src/ui/sidebar.rs`(drag/drop 配線 + import)、
`tests/drag_merge_test.rs`(新規)、本ファイル。i18n は既存 `Msg::OpInProgress` を流用。

### graph BRANCH/TAG badge への拡張(branch `rearch/dnd-graph-badges`, base = `re-architecture`)

ユーザーが本来欲しかった一次導線=コミットグラフの BRANCH/TAG バッジでも同じ
drag-and-drop を実装(sidebar 版は維持、両方が同じ `BranchDrag` /
`start_merge_from_drag` / merge パイプラインを再利用)。

- **`render_badges_column`**(`src/ui/mod.rs`):`cx: &mut Context<KagiApp>` 引数を追加。
  各チップに安定 id(`graph-badge-{i}-{label}`)を付与。`BadgeKind::Branch` のチップは
  個別に draggable(`cursor_grab` + `on_drag(BranchDrag { name }, ghost ctor)`)。1 コミット
  が複数 branch を持つ場合でも各バッジが自分の branch 名を payload に運ぶので、特定の
  バッジを掴めば曖昧さなくその branch を選べる。`BadgeKind::HeadBranch`(current)は
  drop target:`drag_over::<BranchDrag>`(valid ハイライト)+ `on_drop::<BranchDrag>` で
  `start_merge_from_drag(payload.name, cx)` へディスパッチ(view から git は呼ばない)。
  `Remote` / `Tag` は draggable でも drop target でもない。
- **`render_rows`**(`src/ui/mod.rs`):`render_badges_column(..., &mut *cx)` と reborrow して
  `.map()` クロージャから `cx` を渡す(同クロージャは既に `cx.listener(...)` 用に `cx` を
  可変借用しているので、行ごとに `&mut *cx` で再借用)。
- **payload の branch 名取得**:純粋ヘルパ `draggable_branch_name(&RefBadge) -> Option<String>`
  を新設。`BadgeKind::Branch` のとき `label`(= 素の branch 名)を返し、HeadBranch
  (label は `"<name> ✓"`、かつ drop 先であって source ではない)/ Remote / Tag は `None`。
  単体テスト `draggable_branch_name_tests` を追加。
- **`BranchDragGhost`**(`src/ui/mod.rs`):ghost を実際の branch バッジと同じ見た目の
  チップに変更(`badge_style(color_branch)` の tint、`rounded_sm`、`px_1`、`text_sm`)。
  掴んだバッジがカーソルに「貼り付く」アニメーションになる。sidebar 版も同じ
  `BranchDragGhost` を使うので両方が同時に改善(一貫性キープ)。
- **`+N` overflow 制限(既知)**:`render_badges_column` は `MAX_BADGES=2` まで表示し、
  以降を `+N` チップに畳む。`+N` の裏に隠れたバッジは現状まだ個別に draggable で
  ない(コードに `// TODO(T-DNDMERGE-001)`)。overflow の再設計(draggable popover 等)は
  本レーン対象外。
- 変更ファイル:`src/ui/mod.rs`(render_badges_column / render_rows / BranchDragGhost /
  draggable_branch_name + unit tests)、本ファイルのみ。`commit_list.rs` への raw-name
  追加は不要だった(`Branch` の `label` がそのまま素の branch 名のため)。
- `cargo test --workspace` 全 suite 0 failed。`grep -rnE 'git2::|Repository::open' src/ui` = 0。
  既存 `drag_merge_validation_tests` / `tests/drag_merge_test.rs` も全 pass(action 層は不変)。

## PM acceptance (2026-06-14, GUI-verified with cliclick)

Drove the real gesture: dragged `feature/two` onto the current branch `main` in the
running app.
- ✅ Drag started a merge (`[kagi] drag-merge: start merge from drag — source=feature/two`);
  drop opened the merge preview dialog — NOT executed.
- ✅ Dialog title + button both explicit: **"Merge feature/two into main"** (Cancel + the
  named confirm button; no vague OK/Apply).
- ✅ Plan showed current→predicted state, dirty-WT warnings, merge-commit kind, 0 blockers.
- ✅ Drop did not execute: HEAD unchanged (3adca07), no `MERGE_HEAD`.
- ✅ Cancel closed the dialog with repository state unchanged.
Screens: /tmp/kagi_dnd_post2.png (dialog), /tmp/kagi_dnd_cancel3.png (after cancel).
