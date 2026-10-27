# ADR-0014: Repository Navigator Sidebar

- Status: Accepted / Date: 2026-06-12
- Graph の section 配置と scroll ownership は ADR-0217 で更新。

## Decision
- セクション: LOCAL BRANCHES / REMOTE BRANCHES / TAGS / STASHES(順固定)。各セクションは折りたたみ可 + 件数表示。WORKTREES は v0.2、PR/ISSUES は later
- **filter 入力**(先頭固定): 全セクションを部分一致で絞り込み(gpui-component Input を流用)
- local branch 行: current は ✓ + 強調。upstream があれば `↑a ↓b` を行内に併記
- click = jump(既存)/ double-click = checkout(既存)/ **context menu(右クリック)**: Checkout / Create branch here / Delete branch(merged のみ、plan 経由 — backend W2-DELETE)
- 実装はモジュール分離(`src/ui/sidebar.rs` へ抽出)し、mod.rs の肥大を止める

## Consequences
- 状態追加: collapsed sections / filter 文字列(KagiApp 保持、reload で維持)

## Amendment — 2026-10-14: search cancellation and Clear accessibility (#1547/#1548)

Nonempty search consumes Escape by clearing only its query and retaining the
Input's focus. Empty search Escape retains the existing exit to the local list.
IME marked-text cancellation is consumed by Input before the containing
workspace's Escape handler runs. The Input's optional Clear button receives its
own localized EN/JA action name, distinct from “Filter sidebar”; native AXPress
uses the existing clear-and-refocus callback and empty inputs expose no Clear.

## Amendment — 2026-10-15: public Input boundaries (#1479-S4)

Slice definitions: [official upstream migration #1479](../rearch/upstream-migration-1479.md).

The sidebar keeps the same dense xsmall Input and ghost CircleX Button:
empty queries expose no Clear, hover/pressed colours remain the component's,
the button is not an extra Tab stop, and EN/JA names stay on the real Button
and TextInput nodes. Clear changes only the existing query owner and restores
that Input's focus; no extra editor or semantic Group is introduced.
Use public `Input::suffix` with `cleanable(false)`, not the fork-only
`clear_button_aria_label` API.

The current pin's `InputState::clean` is private. Its public
`EntityInputHandler::replace_text_in_range` over the whole UTF-16 range is the
closest equivalent for this plain, enabled search field: it emits Change,
retains normal typing-history grouping, and places the empty caret at zero.
Unlike `replace_all`, it does not force a new undo group; unlike `set_value`,
it does not clear history or suppress Change. There is no highlighter or
completion provider on this search field, so the private clean path's silent
LSP/highlighter handling has no observable counterpart here.

Repository and independent Settings roots each retain one window-owned bool.
Capture of `input::Escape` overwrites it from the actually focused Input's
public marked-text query, then propagates. The child keeps its menu/completion
and IME priorities. Bubble consumes only if that action began with marked
text, preventing a later Escape binding from clearing search or closing its
host after upstream unmarks. No early unmark, per-render allocation or global
registry is introduced. A retained but unfocused Input is never an editing
target. No caller enables `clean_on_escape`; adding one requires revisiting
the boundary because child-side clear precedes bubble.

The pinned fork's Input action already consumes IME Escape, but the native
regression exposed a separate Kagi binding bug: the negation-only
`!Terminal && !SearchPanel` predicate matched at Input depth and its later
registration outranked the child action. Commit and Issue body Escape left
marked text intact; directly dispatching `input::Escape` unmarked it.
The shared workspace/Welcome routing container now declares `KagiWorkspace`,
and the fallback binding requires that positive context in addition to the
existing exclusions. GPUI ranks the child's Input/PopupMenu binding ahead of
the shallower workspace context; an unhandled child Escape still reaches the
unchanged modal/menu/filter/selection/bottom-pane fallback. Adding `!Input`
instead would disable that fallback throughout the focus path.
The same child-first rule applies to Settings' open theme Select: the first
Escape closes its popup, and the next closes Settings and restores the
underlying File History focus. The prior focus-return oracle incorrectly
relied on the global binding skipping that popup; it now checks both layers.

Input naming oracles inspect the native AX tree's actual focused TextInput or
MultilineTextInput. The IME regression also covers a real toolbar PopupMenu's
Escape and button-focused editing-action isolation (#1554). This is preparation
for the official dependency cutover, not that cutover.

### Nested Popover Escape audit (#1479-S4)

A containing kit Popover consumes its `Cancel`; a Kagi-owned child must not
rely on the shallower workspace fallback to dismiss first. The co-author
ListBox now binds Escape in `KagiCoauthorMenu` to its existing dismiss action:
clear `coauthor_menu`, reconcile navigation, and restore `cp-coauthor` focus.
Pinned group reveals retain both `KagiRowList` and `KagiSidebarPaneRows` at
the revealed row's context depth, alongside `SidebarGroupReveal`. The row-list
keys and tree Left must outrank negation-only global pane bindings at that
same depth; retaining only the ancestor's tree context lets global Left win.
Escape binds to the existing row-local `CloseMainDiff` handler.
`sidebar_group_reveal_lifetime` covers revealed Left, Up/Down/Home/End, and
Right's existing inert Graph behavior (tree expansion remains Enter/Space).
A root overlay must also take focus:
the PR context menu now shares `MenuKeys`, `with_menu_keys` and `focus_row`
with the Changes file menu, retaining its existing four-row chrome while
restoring its opener on dismissal.

The application has four explicit `Popover::new` host families (no additional
constructors in the UI crates). Their child/front-layer routes are:

| Host / child | Escape owner and result |
| --- | --- |
| Compact composer / co-author ListBox | `KagiCoauthorMenu` → `CoauthorDismiss`; retire menu, restore trigger, keep composer and draft. |
| Compact composer / title or body Input | Input handles completion/composition first; root composition boundary prevents a second binding; ordinary unhandled Escape reaches containing Popover. |
| Compact composer / file context menu, commit plan, Smart Commit | These are root/front-layer overlays, not composer descendants; existing named menu/modal routing owns dismissal. |
| Compact rail local/remote/tags/stashes / filter or tree rows | Input handles composition first; otherwise Popover closes the navigator. Compact filtering intentionally preserves query on close, unlike desktop clear-first Escape. |
| Compact rail Worktrees | Local activation opens the repository; remote rows have no inspection card. There is no nested compact inspection-card Escape route. Desktop inspection remains owned by its existing row-local workspace action. |
| Compact rail / branch, remote, tag, stash or worktree context menu | Rendered as root overlays by `menu_overlay`; existing `KagiMenu` → `MenuEscape` closes submenu/menu first and returns to its origin. |
| Compact rail / pinned group-header tooltip | While revealed, `SidebarGroupReveal` → row-local `CloseMainDiff` clears only `group_reveal`; the group retains focus and the navigator stays open. |
| Compact rail PRs/Issues / title filter, category tabs or rows | Input composition first; otherwise containing Popover dismissal. These lists do not introduce an inner popup owner. |
| Compact rail PRs/Issues / filter-option menu | Root overlay (`list_filter_strip::render_menu`) already takes focus with `MenuKeys` and `KagiMenu` Escape. |
| Compact rail PRs / PR context menu | Root overlay (`file_menu::render_pr_menu_overlay`) now takes focus via session-owned `MenuKeys`; `KagiMenu` Escape retires only `pr_menu` and restores the opener, leaving the navigator open. Root placement alone was insufficient while focus remained inside the Popover. |
| Toolbar overflow / kit PopupMenu | Deeper kit PopupMenu Escape dismisses the menu; existing callback synchronizes its Popover and trigger focus. |
| Repository-tab overflow / kit PopupMenu | Same kit PopupMenu-first route; existing callback synchronizes the tab-overflow host. |
| Settings theme cards (#1816) | Cards have no nested popup. Escape or Command-Period closes the portable overlay and restores its origin; the macOS Settings window retains its window-level Escape no-op (ADR-0223). |

Regression coverage includes `compact_layout_state` and
`compact_layout_state_tall` (existing co-author owner/focus/draft assertions),
plus `compact_rail_nested_escape`, `compact_pr_menu_escape` and
`compact_group_reveal_escape` for child-then-navigator dismissal in EN/JA,
light/dark and 100%/167%, and `sidebar_rows` for desktop inspection focus return.
Run all `compact_` and `sidebar_` filters alongside the input boundary and
workspace gates; deferred paint
placement alone does not change the element's action ancestry.
