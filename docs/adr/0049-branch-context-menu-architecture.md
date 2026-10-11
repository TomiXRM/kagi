# ADR-0049: Branch Context Menu Architecture

- Status: Accepted(2026-06-13)
- 関連: requirements-branch-context-menu.md、ADR-0020〜0026(commit context menu)

## Decision

- **既存 context_menu.rs の MenuGroup / MenuItem / overlay 機構を再利用**する(backdrop+card とも
  `.occlude()` 必須 — 既知の click-through バグ)。branch 専用の組み立ては新 module
  `src/ui/branch_menu.rs` に置く
- menu 構築は純粋関数 `branch_context_menu_items(ctx: &BranchMenuContext) -> Vec<MenuGroup>`
  (ADR-0050)。UI は sidebar.rs の branch 行に `on_mouse_down(Right)` を付け、対象 branch を
  selection state に反映してから anchor 位置で overlay を開く
- **dispatch は既存 handler を呼ぶだけ**(start_checkout / start_pull / start_push /
  open_create_branch_modal / open_delete_branch_modal / start_create_worktree / copy 系)。
  Context Menu 専用の git 実装を作らない(二重実装禁止)
- 新規操作(set upstream / rename / merge / non-current pull(ff-only ref update)/
  create tracking branch)は `src/git/ops.rs` に plan_*/execute_* を追加し、Header/Panel からも
  使える形で実装する
- folder/group 行は MVP では menu を出さない(no-op)

## Amendment — 2026-10-12: compact branch context menu (#1238)

- Status: Accepted
- Scope: branch menu organization and reusable context-menu submenu presentation;
  no new Git operations or availability/safety policies.

### Role, reference and organization

The role is a **dense contextual command menu**, not an explanatory panel.
References are Apple's [Menus](https://developer.apple.com/design/human-interface-guidelines/menus)
and [Context menus](https://developer.apple.com/design/human-interface-guidelines/context-menus),
using the 2026-09-12 captures in the HIG reference corpus.
Take succinct labels, frequent-first ordering, logical separator groups and
predictively named, shallow submenus; do not import touch-platform previews
or a new native-menu owner.

- Limit the branch menu to **at most ten top-level command/submenu rows**.
  Submenu launchers count toward the limit; separators and a passive target
  header do not.
- Keep Checkout, current-branch Pull / Push / Fetch, Create Branch, and
  Rename / Delete readily discoverable according to the existing context.
  Put related less-frequent commands in **Integrate**, **Advanced** and
  **Worktree** submenus, with separators between logical command groups.
  Branch menus use only one submenu level.
- Retain every previously offered command and its existing ID. Moving a
  command into a submenu changes presentation, not dispatch, reachability or
  availability. Command lookup and menu-model consumers traverse children
  recursively rather than assuming all commands are top-level.

### Shared ownership and interaction

Extend the existing `MenuItem` model and the shared `menu_overlay` /
`menu_keys` owners with generic reusable submenu support. Branch-specific
assembly stays in `src/ui/branch_menu.rs`; do not create a branch-only
renderer, keyboard controller or duplicate Git dispatch implementation.
Retain backdrop/card occlusion, viewport-aware positioning and scrolling,
and the existing focus-return and overlay-dismissal ownership.

- Up / Down and Home / End navigate the active level, including visible
  disabled rows. Hidden items and separators are not navigation targets.
- Right or Enter on a submenu launcher opens its child level. Left or
  Escape in a child closes **one level** and returns focus to its launcher;
  Escape at the root dismisses the menu. Tab / Shift+Tab dismiss the menu.
- Enabled leaf activation reuses the existing dispatch and confirmation
  paths. Disabled leaves remain keyboard-focusable but **inert**: pointer
  clicks and keyboard activation never dispatch them or dismiss the menu.
- Disabled rows show a concise dimmed command label, **no inline reason
  paragraph**. The complete reason is available in a hover tooltip and the
  row's accessibility description, alongside its disabled state. Keep menu
  and item roles, submenu expanded state and keyboard-focus highlighting.
  Submenu launchers remain available even if their children are disabled.
- Rest, hover, keyboard focus, disabled and submenu-open states use the
  shared menu presentation. Busy/protected/conflict/worktree restrictions
  come from the existing context; selecting a row does not introduce a
  loading state or a new operation outcome.

### Availability, safety and localization

Preserve [ADR-0050](0050-branch-operation-availability.md): impossible actions
stay hidden; temporarily unavailable actions stay disabled with reasons.
This is an explicit safety/discoverability exception to the Context menus
HIG recommendation to hide unavailable commands. The general Menus HIG
also supports dimmed, unresponsive unavailable items; keyboard focus here
is for inspecting the reason, not permission to execute.

All existing dangerous flags, blockers and confirmation stages remain
unchanged ([ADR-0023](0023-dangerous-operations-policy.md),
[ADR-0022](0022-plan-integration-context-menu.md)). Organizing an Advanced
submenu must not enable previously disabled danger commands. Repository
writes still follow plan → confirm → preflight → execute → verify → oplog.
No new Git operation, backend policy or `klog!` contract is added or changed.

Submenu labels and reason prose use the existing EN/JA `Msg` tables in
`kagi-ui-core`, including tooltips and accessibility descriptions.
Preserve English Git domain words in JA under
[ADR-0048](0048-ui-localization.md); never translate branch names or IDs.
