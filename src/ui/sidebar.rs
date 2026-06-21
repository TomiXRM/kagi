//! Sidebar renderer — W2-SIDEBAR: Repository Navigator
//!
//! Extracted from mod.rs (T013) and extended to a full 4-section navigator:
//! LOCAL BRANCHES / REMOTE BRANCHES / TAGS / STASHES
//!
//! Public surface:
//! - `render_sidebar(...)` — called from `render_body` in mod.rs

use std::collections::HashSet;

use gpui::{
    div, prelude::*, px, rgb, uniform_list, Context, Entity, SharedString, UniformListScrollHandle,
};
use gpui_component::input::{Input, InputState};
use gpui_component::tooltip::Tooltip;
use gpui_component::Sizable as _;

use kagi_git::{CommitId, RemoteBranch, Stash, Tag, Worktree};

use super::theme::{self, theme};
use super::{BranchDrag, BranchDragGhost, KagiApp};

/// Uniform row height (unscaled) used for **every** virtualized sidebar row.
///
/// `uniform_list` requires a single fixed row height — it measures the first
/// item and applies that height to all of them. Every sidebar row (section
/// header, group header, branch/remote/tag/worktree/stash leaf, and the
/// placeholder rows) is therefore pinned to this height so the virtualized
/// list scrolls correctly regardless of which row happens to be first.
const SIDEBAR_ROW_H: f32 = 24.0;

/// Default sidebar width in pixels (T023). Previously `mod.rs::SIDEBAR_DEFAULT`.
const SIDEBAR_DEFAULT_WIDTH: f32 = 200.0;

/// Consolidated Repository-Navigator (left sidebar) state.
///
/// Previously six flat `sidebar_*` fields on the `KagiApp` god-struct; grouped
/// here as the prep step for a future `Entity<SidebarState>` migration
/// (ADR-0110 Phase 5 Step 5.1). All fields are app-global (not per-tab) and are
/// preserved across repository reloads. Pure constructible (no `cx`) — the
/// `filter` `InputState` is created lazily on first focus.
pub struct SidebarState {
    /// Current sidebar width in pixels (T023: user-resizable).
    pub width: f32,
    /// PERF-SIDEBAR-VIRT: scroll handle for the navigator `uniform_list`
    /// ("sidebar-list"). Persisted across frames.
    pub scroll_handle: UniformListScrollHandle,
    /// Pre-flattened navigator rows (built in `render`); the `uniform_list`
    /// processor reads `rows[i]`, so the sidebar costs O(visible rows) per frame.
    pub rows: Vec<SidebarRow>,
    /// Collapsed sections (HashSet of section keys). Preserved across reloads.
    pub collapsed: HashSet<&'static str>,
    /// Lazy `InputState` for the filter input (gpui-component IME 対応); created
    /// on first click of the filter area (requires `&mut Window`).
    pub filter: Option<Entity<InputState>>,
    /// Whether the navigator is shown (View → Toggle Sidebar). Default `true`.
    pub visible: bool,
}

impl SidebarState {
    pub fn new() -> Self {
        Self {
            width: SIDEBAR_DEFAULT_WIDTH,
            scroll_handle: UniformListScrollHandle::new(),
            rows: Vec::new(),
            collapsed: HashSet::new(),
            filter: None,
            visible: true,
        }
    }
}

impl Default for SidebarState {
    fn default() -> Self {
        Self::new()
    }
}

// W9-THEME: all colours come from `theme()` (see theme.rs).

// ──────────────────────────────────────────────────────────────
// Section keys (static strings used in SidebarState::collapsed)
// ──────────────────────────────────────────────────────────────

pub const SECTION_LOCAL: &str = "local";
pub const SECTION_REMOTE: &str = "remote";
pub const SECTION_TAGS: &str = "tags";
pub const SECTION_WORKTREES: &str = "worktrees";
pub const SECTION_STASHES: &str = "stashes";

// ──────────────────────────────────────────────────────────────
// W13-BRANCHTREE: `/`-prefix grouping of branch names
// ──────────────────────────────────────────────────────────────

/// One entry in a grouped branch listing.
///
/// Grouping is a **single first-level** split on `/` (the ticket explicitly
/// allows stopping after one level — `feat/ui/x` becomes group `feat` + leaf
/// `ui/x`, not a multi-level tree). This keeps the UI shallow and the click
/// model simple while still giving the user collapsible `feat` / `fix` groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupRow<T> {
    /// A collapsible group header for a `/`-prefix, with its child count.
    Group {
        /// The prefix before the first `/`, e.g. `"feat"`.
        prefix: String,
        /// Number of leaves under this group.
        count: usize,
    },
    /// A branch leaf that belongs to the group started by the most recent
    /// preceding [`GroupRow::Group`], displayed with the prefix stripped.
    GroupedLeaf {
        /// The owning group's prefix (for building the collapse key).
        prefix: String,
        /// The remainder of the name after the first `/` (e.g. `"a"` or
        /// `"ui/x"`). This is what the row shows; the original item carries
        /// the full name for click/tooltip behaviour.
        leaf_label: String,
        /// The original item (full branch info), preserved verbatim.
        item: T,
    },
    /// A name with no `/` — rendered at the top level exactly as before.
    TopLevel {
        /// The original item, preserved verbatim.
        item: T,
    },
}

/// Group a list of branch items by the first `/` segment of their name.
///
/// Pure function (no UI/gpui types) so it can be unit-tested. Order is
/// preserved from the input: groups appear in first-seen order, leaves within
/// a group in input order, top-level names interleaved at the position of
/// their group's first member (groups) or their own position (top-level).
///
/// `name_of` extracts the grouping name from each item (chars-based split, no
/// byte indexing). Items whose name has no `/` (or an empty prefix, e.g. a
/// leading `/`) become [`GroupRow::TopLevel`].
fn group_by_prefix<T: Clone>(items: &[T], name_of: impl Fn(&T) -> &str) -> Vec<GroupRow<T>> {
    // First pass: collect group order + counts (first-seen order).
    let mut group_order: Vec<String> = Vec::new();
    let mut group_count: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for it in items {
        if let Some((prefix, _rest)) = split_first_segment(name_of(it)) {
            if !group_count.contains_key(&prefix) {
                group_order.push(prefix.clone());
            }
            *group_count.entry(prefix).or_insert(0) += 1;
        }
    }

    // Second pass: emit rows. A group header is emitted just before the first
    // leaf that belongs to it; subsequent leaves of the same group follow.
    let mut out: Vec<GroupRow<T>> = Vec::new();
    let mut emitted_header: std::collections::HashSet<String> = std::collections::HashSet::new();
    for it in items {
        match split_first_segment(name_of(it)) {
            Some((prefix, rest)) => {
                if emitted_header.insert(prefix.clone()) {
                    out.push(GroupRow::Group {
                        prefix: prefix.clone(),
                        count: *group_count.get(&prefix).unwrap_or(&0),
                    });
                }
                out.push(GroupRow::GroupedLeaf {
                    prefix,
                    leaf_label: rest,
                    item: it.clone(),
                });
            }
            None => out.push(GroupRow::TopLevel { item: it.clone() }),
        }
    }
    out
}

/// Split a name on its first `/`, returning `(prefix, rest)` where both parts
/// are non-empty. Returns `None` when there is no `/`, or when either side
/// would be empty (e.g. `"/x"` or `"feat/"`), so such names stay top-level.
///
/// chars()-based (no byte slicing) per the project's non-ASCII safety rule.
fn split_first_segment(name: &str) -> Option<(String, String)> {
    let mut prefix = String::new();
    let mut rest = String::new();
    let mut seen_slash = false;
    for ch in name.chars() {
        if !seen_slash && ch == '/' {
            seen_slash = true;
            continue;
        }
        if seen_slash {
            rest.push(ch);
        } else {
            prefix.push(ch);
        }
    }
    if seen_slash && !prefix.is_empty() && !rest.is_empty() {
        Some((prefix, rest))
    } else {
        None
    }
}

/// Build the dynamic collapse key for a group (e.g. `"local:feat"`).
fn group_key(section: &str, prefix: &str) -> String {
    format!("{section}:{prefix}")
}

/// Build the collapse key for a remote *name* level-1 header
/// (e.g. `"remote:origin"`).
fn remote_key(remote: &str) -> String {
    format!("{SECTION_REMOTE}:{remote}")
}

/// Build the collapse key for a sub-group *within* a remote
/// (e.g. `"remote:origin:feat"`). This is namespaced by remote name so two
/// remotes can both have a `feat` sub-group without their collapse state
/// colliding, and it never collides with the level-1 remote header key
/// (which has no third segment) nor with local keys (`local:…`).
fn remote_group_key(remote: &str, prefix: &str) -> String {
    format!("{SECTION_REMOTE}:{remote}:{prefix}")
}

// ──────────────────────────────────────────────────────────────
// W19-REMOTE-TREE: two-level grouping for REMOTE BRANCHES
// ──────────────────────────────────────────────────────────────

/// One flattened render row for the REMOTE BRANCHES section.
///
/// Remote branches are grouped on **two** levels: the remote name is the first
/// level (`origin`, `upstream`, …), and within each remote the branch name's
/// own first `/`-segment is the second level (so `origin/feat/x` →
/// `origin ▸ feat ▸ x`, while `origin/main` → `origin ▸ main`). This mirrors
/// the single-level [`group_by_prefix`] used for local branches, but applied
/// *per remote* to the name with the remote stripped (which `RemoteBranch`
/// already stores separately in `name`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteRow<T> {
    /// Level-1 header: the remote name, with the total branch count under it.
    Remote {
        /// The remote name, e.g. `"origin"`.
        remote: String,
        /// Number of branches belonging to this remote.
        count: usize,
    },
    /// Level-2 header: a `/`-prefix sub-group within a remote.
    SubGroup {
        /// The owning remote name (for the namespaced collapse key).
        remote: String,
        /// The prefix before the first `/` of the branch name, e.g. `"feat"`.
        prefix: String,
        /// Number of leaves under this sub-group.
        count: usize,
    },
    /// A branch leaf that sits directly under a remote (no `/` in its name),
    /// e.g. `origin/main`.
    RemoteLeaf {
        /// The owning remote name.
        remote: String,
        /// The visible label (the branch name as-is for direct leaves).
        leaf_label: String,
        /// The original item, preserved verbatim (carries full display name).
        item: T,
    },
    /// A branch leaf nested under a level-2 sub-group, e.g. `origin/feat/x`
    /// → leaf `x` under sub-group `feat`.
    SubGroupedLeaf {
        /// The owning remote name.
        remote: String,
        /// The owning sub-group prefix (for the namespaced collapse key).
        prefix: String,
        /// The visible label (the name remainder after the first `/`).
        leaf_label: String,
        /// The original item, preserved verbatim.
        item: T,
    },
}

/// Build the two-level remote render rows.
///
/// Pure function (no UI/gpui types) so it can be unit-tested. `remote_of`
/// returns the remote name (level-1 key); `name_of` returns the branch name
/// *without* the remote prefix (the part that gets second-level grouping).
/// Remotes appear in first-seen order; within a remote, sub-groups and leaves
/// preserve input order exactly like [`group_by_prefix`].
fn group_remotes<T: Clone>(
    items: &[T],
    remote_of: impl Fn(&T) -> &str,
    name_of: impl Fn(&T) -> &str,
) -> Vec<RemoteRow<T>> {
    // First pass: remote order + total counts (first-seen order).
    let mut remote_order: Vec<String> = Vec::new();
    let mut remote_count: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for it in items {
        let r = remote_of(it).to_string();
        if !remote_count.contains_key(&r) {
            remote_order.push(r.clone());
        }
        *remote_count.entry(r).or_insert(0) += 1;
    }

    let mut out: Vec<RemoteRow<T>> = Vec::new();
    for remote in &remote_order {
        let count = *remote_count.get(remote).unwrap_or(&0);
        out.push(RemoteRow::Remote {
            remote: remote.clone(),
            count,
        });

        // Collect this remote's items in input order.
        let members: Vec<&T> = items
            .iter()
            .filter(|it| remote_of(it) == remote.as_str())
            .collect();

        // Pre-compute sub-group counts (first-seen order within remote).
        let mut sub_count: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for it in &members {
            if let Some((prefix, _rest)) = split_first_segment(name_of(it)) {
                *sub_count.entry(prefix).or_insert(0) += 1;
            }
        }

        let mut emitted_sub: std::collections::HashSet<String> = std::collections::HashSet::new();
        for it in members {
            match split_first_segment(name_of(it)) {
                Some((prefix, rest)) => {
                    if emitted_sub.insert(prefix.clone()) {
                        out.push(RemoteRow::SubGroup {
                            remote: remote.clone(),
                            prefix: prefix.clone(),
                            count: *sub_count.get(&prefix).unwrap_or(&0),
                        });
                    }
                    out.push(RemoteRow::SubGroupedLeaf {
                        remote: remote.clone(),
                        prefix,
                        leaf_label: rest,
                        item: it.clone(),
                    });
                }
                None => out.push(RemoteRow::RemoteLeaf {
                    remote: remote.clone(),
                    leaf_label: name_of(it).to_string(),
                    item: it.clone(),
                }),
            }
        }
    }
    out
}

/// Build a `.tooltip(...)` closure showing the full (untruncated) name.
/// Row labels are single-line + ellipsized, so the tooltip is how the user
/// reads a name that doesn't fit the sidebar width.
fn name_tooltip(
    full: SharedString,
) -> impl Fn(&mut gpui::Window, &mut gpui::App) -> gpui::AnyView + 'static {
    move |window, cx| Tooltip::new(full.clone()).build(window, cx)
}

// ──────────────────────────────────────────────────────────────
// PERF-SIDEBAR-VIRT: flat row model for `uniform_list`
// ──────────────────────────────────────────────────────────────
//
// On repos with thousands of refs (e.g. zed: ~4500 branches/tags/remotes)
// the old per-`for`-loop sidebar fed *every* row into taffy on every full
// window draw, so unrelated redraws (graph scroll, terminal keystrokes) paid
// an O(all refs) layout cost. We now flatten the whole navigator into a single
// `Vec<SidebarRow>` (honouring section/group collapse + the filter) and render
// it with `uniform_list`, so only the visible window of rows is built and laid
// out per frame — exactly like the commit list.
//
// The flat Vec is rebuilt once per render (cheap: just grouping + collapse
// pruning) and stashed on `KagiApp.sidebar.rows`; the `uniform_list` processor
// reads `this.sidebar.rows[i]` and dispatches to `build_sidebar_row`, which
// reproduces every behaviour the old per-section code had (click/jump,
// dbl-click checkout, delete, drag, drop-to-merge, context menus, collapse
// toggles, indentation, tooltips, the ✓ HEAD marker, lane colours).

/// One flattened, virtualized sidebar row.
///
/// Each variant carries exactly the data its renderer needs. The whole list is
/// uniform-height (`SIDEBAR_ROW_H`), so headers and leaves can be interleaved
/// inside a single `uniform_list`.
#[derive(Debug, Clone)]
pub enum SidebarRow {
    /// A top-level section header (LOCAL BRANCHES / REMOTE BRANCHES / TAGS /
    /// WORKTREES / STASHES). `section` is the static collapse key.
    SectionHeader {
        section: &'static str,
        title: &'static str,
        count: usize,
        collapsed: bool,
    },
    /// A `/`-prefix group header inside LOCAL BRANCHES (collapse key
    /// `local:<prefix>`).
    LocalGroupHeader {
        key: String,
        prefix: String,
        count: usize,
        collapsed: bool,
    },
    /// A local branch leaf. `display_label` is what shows (prefix-stripped for
    /// grouped leaves); `name` is the full branch name used for handlers/id.
    LocalBranchLeaf {
        name: String,
        display_label: String,
        is_head: bool,
        indented: bool,
    },
    /// REMOTE BRANCHES level-1 header (a remote name). Collapse key
    /// `remote:<remote>`.
    RemoteHeader {
        key: String,
        remote: String,
        count: usize,
        collapsed: bool,
    },
    /// REMOTE BRANCHES level-2 sub-group header. Collapse key
    /// `remote:<remote>:<prefix>`.
    RemoteSubGroup {
        key: String,
        prefix: String,
        count: usize,
        collapsed: bool,
    },
    /// A remote branch leaf. `display` is the full `origin/…` name (used for
    /// jump/tooltip/id/drag); `display_label` is the prefix-stripped label;
    /// `depth` drives indentation (1 = direct under remote, 2 = under
    /// sub-group).
    RemoteLeaf {
        display: String,
        display_label: String,
        target: CommitId,
        depth: u8,
    },
    /// A tag leaf.
    Tag { name: String, target: CommitId },
    /// A worktree leaf.
    Worktree {
        name: String,
        path_label: String,
        is_current: bool,
    },
    /// A stash leaf.
    Stash { index: usize, message: String },
}

/// Build the flat, virtualization-ready sidebar row list.
///
/// Walks the SAME grouping (`group_by_prefix` / `group_remotes`) and collapse
/// logic the old per-section renderer used: a collapsed section contributes
/// only its header; a collapsed group contributes only its header; an active
/// filter auto-expands every group (matching the previous behaviour). The
/// result is stored on `KagiApp.sidebar.rows` and consumed by the
/// `uniform_list` processor.
#[allow(clippy::too_many_arguments)]
pub fn build_sidebar_rows(
    branches: &[(String, bool)],
    remote_branches: &[RemoteBranch],
    tags: &[Tag],
    stashes: &[Stash],
    worktrees: &[Worktree],
    collapsed: &HashSet<&'static str>,
    groups_collapsed: &HashSet<String>,
    filter_text: &str,
) -> Vec<SidebarRow> {
    let has_filter = !filter_text.is_empty();
    let matches = |name: &str| -> bool {
        if has_filter {
            name.to_lowercase().contains(filter_text)
        } else {
            true
        }
    };

    let mut rows: Vec<SidebarRow> = Vec::new();

    // ── LOCAL BRANCHES ───────────────────────────────────────────
    {
        let section_collapsed = collapsed.contains(SECTION_LOCAL);
        rows.push(SidebarRow::SectionHeader {
            section: SECTION_LOCAL,
            title: "LOCAL BRANCHES",
            count: branches.len(),
            collapsed: section_collapsed,
        });
        if !section_collapsed {
            let local_owned: Vec<(String, bool)> = branches
                .iter()
                .filter(|(n, _)| matches(n))
                .cloned()
                .collect();
            let grouped = group_by_prefix(&local_owned, |(n, _)| n.as_str());
            for row in &grouped {
                match row {
                    GroupRow::Group { prefix, count } => {
                        let key = group_key(SECTION_LOCAL, prefix);
                        let group_collapsed = !has_filter && groups_collapsed.contains(&key);
                        rows.push(SidebarRow::LocalGroupHeader {
                            key,
                            prefix: prefix.clone(),
                            count: *count,
                            collapsed: group_collapsed,
                        });
                    }
                    GroupRow::GroupedLeaf {
                        prefix,
                        leaf_label,
                        item,
                    } => {
                        let key = group_key(SECTION_LOCAL, prefix);
                        let group_collapsed = !has_filter && groups_collapsed.contains(&key);
                        if !group_collapsed {
                            let (name, is_head) = item;
                            rows.push(SidebarRow::LocalBranchLeaf {
                                name: name.clone(),
                                display_label: leaf_label.clone(),
                                is_head: *is_head,
                                indented: true,
                            });
                        }
                    }
                    GroupRow::TopLevel { item } => {
                        let (name, is_head) = item;
                        rows.push(SidebarRow::LocalBranchLeaf {
                            name: name.clone(),
                            display_label: name.clone(),
                            is_head: *is_head,
                            indented: false,
                        });
                    }
                }
            }
        }
    }

    // ── REMOTE BRANCHES ──────────────────────────────────────────
    {
        let section_collapsed = collapsed.contains(SECTION_REMOTE);
        rows.push(SidebarRow::SectionHeader {
            section: SECTION_REMOTE,
            title: "REMOTE BRANCHES",
            count: remote_branches.len(),
            collapsed: section_collapsed,
        });
        if !section_collapsed {
            let remote_owned: Vec<(String, String, String, CommitId)> = remote_branches
                .iter()
                .filter(|rb| matches(&rb.name) || matches(&format!("{}/{}", rb.remote, rb.name)))
                .map(|rb| {
                    (
                        rb.remote.clone(),
                        rb.name.clone(),
                        format!("{}/{}", rb.remote, rb.name),
                        rb.target.clone(),
                    )
                })
                .collect();
            let grouped = group_remotes(
                &remote_owned,
                |(r, _, _, _)| r.as_str(),
                |(_, n, _, _)| n.as_str(),
            );
            for row in &grouped {
                match row {
                    RemoteRow::Remote { remote, count } => {
                        let key = remote_key(remote);
                        let collapsed_now = !has_filter && groups_collapsed.contains(&key);
                        rows.push(SidebarRow::RemoteHeader {
                            key,
                            remote: remote.clone(),
                            count: *count,
                            collapsed: collapsed_now,
                        });
                    }
                    RemoteRow::SubGroup {
                        remote,
                        prefix,
                        count,
                    } => {
                        let parent_key = remote_key(remote);
                        if !has_filter && groups_collapsed.contains(&parent_key) {
                            continue;
                        }
                        let key = remote_group_key(remote, prefix);
                        let collapsed_now = !has_filter && groups_collapsed.contains(&key);
                        rows.push(SidebarRow::RemoteSubGroup {
                            key,
                            prefix: prefix.clone(),
                            count: *count,
                            collapsed: collapsed_now,
                        });
                    }
                    RemoteRow::RemoteLeaf {
                        remote,
                        leaf_label,
                        item,
                    } => {
                        let parent_key = remote_key(remote);
                        if !has_filter && groups_collapsed.contains(&parent_key) {
                            continue;
                        }
                        let (_r, _n, display, target) = item;
                        rows.push(SidebarRow::RemoteLeaf {
                            display: display.clone(),
                            display_label: leaf_label.clone(),
                            target: target.clone(),
                            depth: 1,
                        });
                    }
                    RemoteRow::SubGroupedLeaf {
                        remote,
                        prefix,
                        leaf_label,
                        item,
                    } => {
                        let parent_key = remote_key(remote);
                        let sub_key = remote_group_key(remote, prefix);
                        let hidden = !has_filter
                            && (groups_collapsed.contains(&parent_key)
                                || groups_collapsed.contains(&sub_key));
                        if hidden {
                            continue;
                        }
                        let (_r, _n, display, target) = item;
                        rows.push(SidebarRow::RemoteLeaf {
                            display: display.clone(),
                            display_label: leaf_label.clone(),
                            target: target.clone(),
                            depth: 2,
                        });
                    }
                }
            }
        }
    }

    // ── TAGS ─────────────────────────────────────────────────────
    {
        let section_collapsed = collapsed.contains(SECTION_TAGS);
        rows.push(SidebarRow::SectionHeader {
            section: SECTION_TAGS,
            title: "TAGS",
            count: tags.len(),
            collapsed: section_collapsed,
        });
        if !section_collapsed {
            for tag in tags.iter().filter(|t| matches(&t.name)) {
                rows.push(SidebarRow::Tag {
                    name: tag.name.clone(),
                    target: tag.target.clone(),
                });
            }
        }
    }

    // ── WORKTREES ────────────────────────────────────────────────
    {
        let section_collapsed = collapsed.contains(SECTION_WORKTREES);
        rows.push(SidebarRow::SectionHeader {
            section: SECTION_WORKTREES,
            title: "WORKTREES",
            count: worktrees.len(),
            collapsed: section_collapsed,
        });
        if !section_collapsed {
            for wt in worktrees
                .iter()
                .filter(|w| matches(&w.name) || matches(w.path.to_string_lossy().as_ref()))
            {
                rows.push(SidebarRow::Worktree {
                    name: wt.name.clone(),
                    path_label: wt.path.display().to_string(),
                    is_current: wt.is_current,
                });
            }
        }
    }

    // ── STASHES ──────────────────────────────────────────────────
    {
        let section_collapsed = collapsed.contains(SECTION_STASHES);
        rows.push(SidebarRow::SectionHeader {
            section: SECTION_STASHES,
            title: "STASHES",
            count: stashes.len(),
            collapsed: section_collapsed,
        });
        if !section_collapsed {
            for stash in stashes.iter().filter(|s| matches(&s.message)) {
                rows.push(SidebarRow::Stash {
                    index: stash.index,
                    message: stash.message.clone(),
                });
            }
        }
    }

    rows
}

// ──────────────────────────────────────────────────────────────
// Per-row builders (called from the `uniform_list` processor)
// ──────────────────────────────────────────────────────────────

/// Dispatch a single flat row to its renderer. Reads live data from `this`
/// (upstream info, commit index) so handlers see current state, mirroring the
/// commit-list per-row builders.
fn build_sidebar_row(
    this: &KagiApp,
    row: &SidebarRow,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    match row {
        SidebarRow::SectionHeader {
            section,
            title,
            count,
            collapsed,
        } => build_section_header(section, title, *count, *collapsed, cx),
        SidebarRow::LocalGroupHeader {
            key,
            prefix,
            count,
            collapsed,
        } => build_group_header(key, prefix, *count, *collapsed, theme::scaled_px(20.), cx),
        SidebarRow::LocalBranchLeaf {
            name,
            display_label,
            is_head,
            indented,
        } => build_local_branch_leaf(this, name, *is_head, display_label, *indented, cx),
        SidebarRow::RemoteHeader {
            key,
            remote,
            count,
            collapsed,
        } => build_group_header(key, remote, *count, *collapsed, theme::scaled_px(20.), cx),
        SidebarRow::RemoteSubGroup {
            key,
            prefix,
            count,
            collapsed,
        } => build_group_header(key, prefix, *count, *collapsed, theme::scaled_px(32.), cx),
        SidebarRow::RemoteLeaf {
            display,
            display_label,
            target,
            depth,
        } => build_remote_leaf(this, display, display_label, target.clone(), *depth, cx),
        SidebarRow::Tag { name, target } => build_tag_row(this, name, target.clone(), cx),
        SidebarRow::Worktree {
            name,
            path_label,
            is_current,
        } => build_worktree_row(name, path_label, *is_current),
        SidebarRow::Stash { index, message } => build_stash_row(*index, message, cx),
    }
}

/// Section header row (LOCAL BRANCHES / …). Click toggles `sidebar_collapsed`.
fn build_section_header(
    section: &'static str,
    title: &'static str,
    count: usize,
    collapsed: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let label = SharedString::from(format!(
        "{} {} ({})",
        if collapsed { "\u{25b8}" } else { "\u{25be}" },
        title,
        count
    ));
    let toggle = cx.listener(
        move |this: &mut KagiApp, _: &gpui::ClickEvent, _window, cx| {
            if this.sidebar.collapsed.contains(section) {
                this.sidebar.collapsed.remove(section);
            } else {
                this.sidebar.collapsed.insert(section);
            }
            cx.notify();
        },
    );
    div()
        .id(SharedString::from(format!("sidebar-section-{}", section)))
        .h(theme::scaled_px(SIDEBAR_ROW_H))
        .px_3()
        .flex()
        .flex_row()
        .items_center()
        .text_xs()
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(theme().text_muted))
        .on_click(toggle)
        .hover(|s| s.bg(rgb(theme().surface)))
        .child(label)
        .into_any()
}

/// A `/`-prefix group header (local groups, remote level-1, remote level-2).
/// Click toggles `branch_groups_collapsed` for `key`. `left_pad` sets indent.
fn build_group_header(
    key: &str,
    label_text: &str,
    count: usize,
    collapsed: bool,
    left_pad: gpui::Pixels,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let arrow = if collapsed { "\u{25b8}" } else { "\u{25be}" };
    let glabel = SharedString::from(format!("{} {} ({})", arrow, label_text, count));
    let key_for_toggle = key.to_string();
    let toggle = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        if this.branch_groups_collapsed.contains(&key_for_toggle) {
            this.branch_groups_collapsed.remove(&key_for_toggle);
        } else {
            this.branch_groups_collapsed.insert(key_for_toggle.clone());
        }
        cx.notify();
    });
    div()
        .id(SharedString::from(format!("sidebar-group-{}", key)))
        .h(theme::scaled_px(SIDEBAR_ROW_H))
        .flex()
        .flex_row()
        .items_center()
        .pl(left_pad)
        .pr_3()
        .text_sm()
        .text_color(rgb(theme().text_sub))
        .overflow_hidden()
        .on_click(toggle)
        .hover(|s| s.bg(rgb(theme().surface)))
        .child(div().flex_1().truncate().child(glabel))
        .into_any()
}

/// A local branch leaf — preserves the full behaviour of the old
/// `local_leaf_row`: HEAD rows are drop targets (`drag_over`/`on_drop` →
/// `start_merge_from_drag`) with click=jump + right-click menu; non-HEAD rows
/// are draggable (`BranchDrag` + ghost), click=jump / dbl-click=checkout
/// (`open_plan_modal`), have a right-click menu and a ✕ delete button; both
/// show the ✓ HEAD marker, ↑↓ upstream counts, truncation + tooltip.
fn build_local_branch_leaf(
    this: &KagiApp,
    branch_name: &str,
    is_head: bool,
    display_label: &str,
    indented: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let upstream_label: Option<SharedString> = this
        .active_view
        .branch_upstream_info
        .get(branch_name)
        .and_then(|u| {
            if u.ahead > 0 || u.behind > 0 {
                Some(SharedString::from(format!(
                    "\u{2191}{} \u{2193}{}",
                    u.ahead, u.behind
                )))
            } else {
                None
            }
        });

    let label = if is_head {
        SharedString::from(format!("\u{2713} {}", display_label))
    } else {
        SharedString::from(display_label.to_string())
    };
    let text_color = if is_head {
        theme().color_success
    } else {
        theme().text_main
    };
    let full_name = SharedString::from(branch_name.to_string());
    let left_pad = if indented {
        theme::scaled_px(28.)
    } else {
        theme::scaled_px(12.)
    };

    if is_head {
        let branch_for_click = branch_name.to_string();
        let branch_for_menu = branch_name.to_string();
        let head_click = cx.listener(move |this: &mut KagiApp, _e: &gpui::ClickEvent, _w, cx| {
            this.jump_to_branch(&branch_for_click);
            cx.notify();
        });
        let menu_click = cx.listener(
            move |this: &mut KagiApp, event: &gpui::MouseDownEvent, _window, cx| {
                this.open_local_branch_menu(branch_for_menu.clone(), event.position);
                cx.stop_propagation();
                cx.notify();
            },
        );
        let drop_handler = cx.listener(
            move |this: &mut KagiApp, payload: &BranchDrag, _window, cx| {
                this.start_merge_from_drag(payload.name.clone(), cx);
                cx.notify();
            },
        );
        div()
            .id(SharedString::from(format!(
                "sidebar-branch-{}",
                branch_name
            )))
            .h(theme::scaled_px(SIDEBAR_ROW_H))
            .flex()
            .flex_row()
            .items_center()
            .pl(left_pad)
            .pr_3()
            .text_sm()
            .text_color(rgb(text_color))
            .overflow_hidden()
            .on_click(head_click)
            .on_mouse_down(gpui::MouseButton::Right, menu_click)
            .drag_over::<BranchDrag>(|style, _drag, _window, _cx| {
                style
                    .bg(rgb(theme().selected))
                    .border_color(rgb(theme().color_branch))
            })
            .on_drop::<BranchDrag>(drop_handler)
            .hover(|style| style.bg(rgb(theme().surface)))
            .tooltip(name_tooltip(full_name))
            .child(div().flex_1().truncate().child(label))
            .when_some(upstream_label, |el, ul| {
                el.child(
                    div()
                        .flex_shrink_0()
                        .ml_2()
                        .text_xs()
                        .text_color(rgb(theme().text_sub))
                        .child(ul),
                )
            })
            .into_any()
    } else {
        let branch_for_dbl = branch_name.to_string();
        let branch_for_delete = branch_name.to_string();
        let branch_for_menu = branch_name.to_string();
        let branch_for_drag = branch_name.to_string();
        let click_handler = cx.listener(
            move |this: &mut KagiApp, event: &gpui::ClickEvent, _window, cx| {
                if event.click_count() >= 2 {
                    this.open_plan_modal(branch_for_dbl.clone());
                } else {
                    this.jump_to_branch(&branch_for_dbl);
                }
                cx.notify();
            },
        );
        let delete_handler = cx.listener(
            move |this: &mut KagiApp, _event: &gpui::ClickEvent, _window, cx| {
                this.open_delete_branch_modal(branch_for_delete.clone());
                cx.notify();
            },
        );
        let menu_click = cx.listener(
            move |this: &mut KagiApp, event: &gpui::MouseDownEvent, _window, cx| {
                this.open_local_branch_menu(branch_for_menu.clone(), event.position);
                cx.stop_propagation();
                cx.notify();
            },
        );
        div()
            .id(SharedString::from(format!(
                "sidebar-branch-{}",
                branch_name
            )))
            .h(theme::scaled_px(SIDEBAR_ROW_H))
            .flex()
            .flex_row()
            .items_center()
            .pl(left_pad)
            .pr_3()
            .text_sm()
            .text_color(rgb(text_color))
            .overflow_hidden()
            .on_click(click_handler)
            .on_mouse_down(gpui::MouseButton::Right, menu_click)
            .on_drag(
                BranchDrag {
                    name: branch_for_drag.clone(),
                },
                move |drag: &BranchDrag, _pos, _window, cx| {
                    let name = SharedString::from(drag.name.clone());
                    cx.new(|_| BranchDragGhost { name })
                },
            )
            .hover(|style| style.bg(rgb(theme().surface)))
            .tooltip(name_tooltip(full_name))
            .child(div().flex_1().truncate().child(label))
            .when_some(upstream_label, |el, ul| {
                el.child(
                    div()
                        .flex_shrink_0()
                        .ml_2()
                        .text_xs()
                        .text_color(rgb(theme().text_sub))
                        .child(ul),
                )
            })
            .child(
                div()
                    .id(SharedString::from(format!(
                        "sidebar-delete-{}",
                        branch_name
                    )))
                    .flex_shrink_0()
                    .ml_1()
                    .px_1()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .on_click(delete_handler)
                    .hover(|s| s.text_color(rgb(theme().color_blocker)))
                    .child(SharedString::from("\u{00d7}")),
            )
            .into_any()
    }
}

/// A remote branch leaf — preserves the old `remote_leaf_row`: jumpable rows
/// get click=jump + right-click `open_remote_branch_menu`; all rows are
/// draggable merge sources (`BranchDrag` + ghost) with the menu; indentation
/// by `depth`; truncation + tooltip; remote lane colour.
fn build_remote_leaf(
    this: &KagiApp,
    display: &str,
    display_label: &str,
    rb_target: CommitId,
    depth: u8,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let can_jump = this.active_view.commit_row_index.contains_key(&rb_target);
    let full_name = SharedString::from(display.to_string());
    let label = SharedString::from(display_label.to_string());
    let drag_name = display.to_string();
    let left_pad = match depth {
        0 => theme::scaled_px(12.),
        1 => theme::scaled_px(28.),
        _ => theme::scaled_px(44.),
    };
    let display_for_menu = display.to_string();
    let target_for_menu = rb_target.clone();
    let menu_click = cx.listener(
        move |this: &mut KagiApp, event: &gpui::MouseDownEvent, _window, cx| {
            this.open_remote_branch_menu(
                display_for_menu.clone(),
                target_for_menu.clone(),
                event.position,
            );
            cx.stop_propagation();
            cx.notify();
        },
    );

    let base = div()
        .id(SharedString::from(format!("sidebar-remote-{}", display)))
        .h(theme::scaled_px(SIDEBAR_ROW_H))
        .flex()
        .flex_row()
        .items_center()
        .pl(left_pad)
        .pr_3()
        .text_sm()
        .text_color(rgb(theme().color_remote))
        .overflow_hidden()
        .on_mouse_down(gpui::MouseButton::Right, menu_click)
        .cursor_grab()
        .on_drag(
            BranchDrag {
                name: drag_name.clone(),
            },
            move |drag: &BranchDrag, _pos, _window, cx| {
                let name = SharedString::from(drag.name.clone());
                cx.new(|_| BranchDragGhost { name })
            },
        )
        .hover(|style| style.bg(rgb(theme().surface)))
        .tooltip(name_tooltip(full_name))
        .child(div().flex_1().truncate().child(label));

    if can_jump {
        let click_handler = cx.listener(
            move |this: &mut KagiApp, _event: &gpui::ClickEvent, _window, cx| {
                this.jump_to_commit(&rb_target);
                cx.notify();
            },
        );
        base.on_click(click_handler).into_any()
    } else {
        base.into_any()
    }
}

/// A tag leaf — click jumps to the tag's target when it is in the loaded graph.
fn build_tag_row(
    this: &KagiApp,
    tag_name: &str,
    tag_target: CommitId,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let tag_label = SharedString::from(tag_name.to_string());
    let full_name = SharedString::from(tag_name.to_string());
    let can_jump = this.active_view.commit_row_index.contains_key(&tag_target);
    let base = div()
        .id(SharedString::from(format!("sidebar-tag-{}", tag_name)))
        .h(theme::scaled_px(SIDEBAR_ROW_H))
        .flex()
        .flex_row()
        .items_center()
        .px_3()
        .text_sm()
        .text_color(rgb(theme().color_tag))
        .overflow_hidden()
        .tooltip(name_tooltip(full_name))
        .child(div().flex_1().truncate().child(tag_label));
    if can_jump {
        let click_handler = cx.listener(
            move |this: &mut KagiApp, _event: &gpui::ClickEvent, _window, cx| {
                this.jump_to_commit(&tag_target);
                cx.notify();
            },
        );
        base.hover(|style| style.bg(rgb(theme().surface)))
            .on_click(click_handler)
            .into_any()
    } else {
        base.into_any()
    }
}

/// A worktree leaf (read-only; ✓ marks the current worktree).
fn build_worktree_row(name: &str, path_label: &str, is_current: bool) -> gpui::AnyElement {
    let label = if is_current {
        SharedString::from(format!("\u{2713} {}  {}", name, path_label))
    } else {
        SharedString::from(format!("{}  {}", name, path_label))
    };
    let full_name = label.clone();
    let text_color = if is_current {
        theme().color_success
    } else {
        theme().text_sub
    };
    div()
        .id(SharedString::from(format!("sidebar-worktree-{}", name)))
        .h(theme::scaled_px(SIDEBAR_ROW_H))
        .flex()
        .flex_row()
        .items_center()
        .px_3()
        .text_sm()
        .text_color(rgb(text_color))
        .overflow_hidden()
        .tooltip(name_tooltip(full_name))
        .child(div().flex_1().truncate().child(label))
        .into_any()
}

/// A stash leaf — left-click **pops** (apply + remove); right-click opens a
/// menu (Apply / Drop). User request: clicking a stash should consume it.
fn build_stash_row(index: usize, message: &str, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let raw_label = format!("stash@{{{}}}: {}", index, message);
    let full_name = SharedString::from(raw_label.clone());
    let click_handler = cx.listener(
        move |this: &mut KagiApp, _event: &gpui::ClickEvent, _window, cx| {
            this.open_pop_modal(index);
            cx.notify();
        },
    );
    let msg_for_menu = message.to_string();
    let menu_handler = cx.listener(
        move |this: &mut KagiApp, event: &gpui::MouseDownEvent, _window, cx| {
            this.open_stash_menu(index, msg_for_menu.clone(), event.position);
            cx.stop_propagation();
            cx.notify();
        },
    );
    div()
        .id(("sidebar-stash", index))
        .h(theme::scaled_px(SIDEBAR_ROW_H))
        .flex()
        .flex_row()
        .items_center()
        .px_3()
        .text_sm()
        .text_color(rgb(theme().color_warning))
        .overflow_hidden()
        .on_click(click_handler)
        .on_mouse_down(gpui::MouseButton::Right, menu_handler)
        .hover(|style| style.bg(rgb(theme().surface)))
        .tooltip(name_tooltip(full_name))
        .child(
            div()
                .flex_1()
                .truncate()
                .child(SharedString::from(raw_label)),
        )
        .into_any()
}

// ──────────────────────────────────────────────────────────────
// render_sidebar — main entry point
// ──────────────────────────────────────────────────────────────

/// Render the left sidebar as a 4-section Repository Navigator.
///
/// Sections: LOCAL BRANCHES / REMOTE BRANCHES / TAGS / WORKTREES / STASHES.
///
/// PERF-SIDEBAR-VIRT: the section/group/leaf rows are virtualized with
/// `uniform_list` over the pre-flattened `this.sidebar.rows` (built by
/// [`build_sidebar_rows`] in `render`), so only the visible rows are built and
/// laid out per frame — fixing the O(all refs) taffy cost on huge repos. The
/// filter input is pinned above the list (it has its own height). All section
/// headers, group headers and leaves share `SIDEBAR_ROW_H` so the uniform list
/// scrolls correctly. Every click/jump/dbl-click/drag/drop/context-menu/
/// collapse behaviour from the old per-`for`-loop version is preserved in the
/// per-row builders.
///
/// State fields on `KagiApp`:
/// - `sidebar_rows: Vec<SidebarRow>` (the virtualization source)
/// - `sidebar_scroll_handle: UniformListScrollHandle`
/// - `sidebar_collapsed` / `branch_groups_collapsed` / `sidebar_filter`
pub fn render_sidebar(
    filter_input: Option<Entity<InputState>>,
    width: f32,
    row_count: usize,
    scroll_handle: gpui::UniformListScrollHandle,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    // ── Filter input row (pinned above the virtualized list) ──────
    let filter_area: gpui::AnyElement = if let Some(ref input_entity) = filter_input {
        div()
            .px_2()
            .py_1()
            .flex_shrink_0()
            .child(Input::new(input_entity).xsmall().appearance(true))
            .into_any_element()
    } else {
        // Placeholder: clicking creates the InputState (requires Window).
        let create_handler = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, window, cx| {
            this.ensure_sidebar_filter(window, cx);
            cx.notify();
        });
        div()
            .id("sidebar-filter-placeholder")
            .px_2()
            .py_1()
            .flex_shrink_0()
            .on_click(create_handler)
            .hover(|s| s.bg(rgb(theme().surface)))
            .child(
                div()
                    .h(theme::scaled_px(22.))
                    .flex()
                    .items_center()
                    .px_2()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .bg(rgb(theme().bg_base))
                    .rounded(theme::scaled_px(4.))
                    .child(SharedString::from("filter…")),
            )
            .into_any_element()
    };

    // ── Virtualized navigator list ────────────────────────────────
    let scrollbar_handle = scroll_handle.clone();
    let list = super::with_vertical_scrollbar(
        "sidebar-list-scroll",
        &scrollbar_handle,
        uniform_list(
            "sidebar-list",
            row_count,
            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                range
                    .filter_map(|i| {
                        this.sidebar
                            .rows
                            .get(i)
                            .cloned()
                            .map(|row| build_sidebar_row(this, &row, cx))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(scroll_handle)
        .flex_1()
        .min_h(px(0.))
        .py_1(),
        // Hidden scrollbar (user request): the branch list still scrolls via
        // wheel/trackpad, just without the overlay bar.
        false,
    );

    // ── Fixed-width outer shell ───────────────────────────────────
    div()
        // `width` is the unscaled, persisted sidebar width; scale at render so
        // it tracks zoom uniformly with the text. The resize/drag math in
        // mod.rs interprets cursor deltas in the same scaled space.
        .w(theme::scaled_px(width))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().sidebar))
        .child(filter_area)
        .child(list)
}

// ──────────────────────────────────────────────────────────────
// W13-BRANCHTREE: unit tests for the pure grouping helpers
// ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Compact view of a GroupRow for assertions: ("G", prefix, count) for a
    /// group header, ("L", leaf_label, item) for a grouped leaf, and
    /// ("T", item, item) for a top-level item.
    fn summarize(rows: &[GroupRow<String>]) -> Vec<(&'static str, String, String)> {
        rows.iter()
            .map(|r| match r {
                GroupRow::Group { prefix, count } => ("G", prefix.clone(), count.to_string()),
                GroupRow::GroupedLeaf {
                    leaf_label, item, ..
                } => ("L", leaf_label.clone(), item.clone()),
                GroupRow::TopLevel { item } => ("T", item.clone(), item.clone()),
            })
            .collect()
    }

    fn group(names: &[&str]) -> Vec<GroupRow<String>> {
        let owned: Vec<String> = names.iter().map(|s| s.to_string()).collect();
        group_by_prefix(&owned, |s| s.as_str())
    }

    #[test]
    fn split_basic() {
        assert_eq!(
            split_first_segment("feat/a"),
            Some(("feat".into(), "a".into()))
        );
        assert_eq!(
            split_first_segment("feat/ui/x"),
            Some(("feat".into(), "ui/x".into()))
        );
        assert_eq!(split_first_segment("main"), None);
        // Empty halves stay top-level.
        assert_eq!(split_first_segment("/x"), None);
        assert_eq!(split_first_segment("feat/"), None);
    }

    #[test]
    fn split_non_ascii() {
        // chars()-based: multibyte prefixes must not panic or mis-split.
        assert_eq!(
            split_first_segment("機能/あ"),
            Some(("機能".into(), "あ".into()))
        );
    }

    #[test]
    fn groups_and_top_level() {
        // feat/a, feat/b → group feat(2); fix/c → group fix(1); main → top.
        let rows = group(&["feat/a", "feat/b", "fix/c", "main"]);
        assert_eq!(
            summarize(&rows),
            vec![
                ("G", "feat".into(), "2".into()),
                ("L", "a".into(), "feat/a".into()),
                ("L", "b".into(), "feat/b".into()),
                ("G", "fix".into(), "1".into()),
                ("L", "c".into(), "fix/c".into()),
                ("T", "main".into(), "main".into()),
            ]
        );
    }

    #[test]
    fn multi_segment_leaf_keeps_remainder() {
        // Single first-level split: feat/ui/x → group feat, leaf "ui/x".
        let rows = group(&["feat/ui/x"]);
        assert_eq!(
            summarize(&rows),
            vec![
                ("G", "feat".into(), "1".into()),
                ("L", "ui/x".into(), "feat/ui/x".into()),
            ]
        );
    }

    #[test]
    fn remote_grouped_by_remote_name() {
        // origin/feat/x → group origin, leaf "feat/x".
        let rows = group(&["origin/main", "origin/feat/x", "upstream/dev"]);
        assert_eq!(
            summarize(&rows),
            vec![
                ("G", "origin".into(), "2".into()),
                ("L", "main".into(), "origin/main".into()),
                ("L", "feat/x".into(), "origin/feat/x".into()),
                ("G", "upstream".into(), "1".into()),
                ("L", "dev".into(), "upstream/dev".into()),
            ]
        );
    }

    #[test]
    fn group_key_format() {
        assert_eq!(group_key(SECTION_LOCAL, "feat"), "local:feat");
        assert_eq!(group_key(SECTION_REMOTE, "origin"), "remote:origin");
    }

    #[test]
    fn no_groups_all_top_level() {
        let rows = group(&["main", "dev", "trunk"]);
        assert!(rows.iter().all(|r| matches!(r, GroupRow::TopLevel { .. })));
        assert_eq!(rows.len(), 3);
    }

    // ── W19-REMOTE-TREE: two-level remote grouping ──────────────────

    /// Compact view of a RemoteRow: ("R", remote, count), ("S", prefix, count),
    /// ("RL", remote, leaf), ("SL", prefix, leaf).
    fn summarize_remote(
        rows: &[RemoteRow<(String, String)>],
    ) -> Vec<(&'static str, String, String)> {
        rows.iter()
            .map(|r| match r {
                RemoteRow::Remote { remote, count } => ("R", remote.clone(), count.to_string()),
                RemoteRow::SubGroup { prefix, count, .. } => {
                    ("S", prefix.clone(), count.to_string())
                }
                RemoteRow::RemoteLeaf { leaf_label, .. } => {
                    ("RL", leaf_label.clone(), String::new())
                }
                RemoteRow::SubGroupedLeaf {
                    prefix, leaf_label, ..
                } => ("SL", prefix.clone(), leaf_label.clone()),
            })
            .collect()
    }

    /// Build remote rows from (remote, name) pairs.
    fn group_rem(pairs: &[(&str, &str)]) -> Vec<RemoteRow<(String, String)>> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(r, n)| (r.to_string(), n.to_string()))
            .collect();
        group_remotes(&owned, |(r, _)| r.as_str(), |(_, n)| n.as_str())
    }

    #[test]
    fn remote_two_levels_basic() {
        // origin/main → origin ▸ main (direct leaf)
        // origin/feat/x → origin ▸ feat ▸ x (sub-grouped leaf)
        let rows = group_rem(&[("origin", "main"), ("origin", "feat/x")]);
        assert_eq!(
            summarize_remote(&rows),
            vec![
                ("R", "origin".into(), "2".into()),
                ("RL", "main".into(), String::new()),
                ("S", "feat".into(), "1".into()),
                ("SL", "feat".into(), "x".into()),
            ]
        );
    }

    #[test]
    fn remote_multiple_remotes_independent() {
        // origin and upstream group independently, in first-seen order.
        let rows = group_rem(&[
            ("origin", "feat/a"),
            ("origin", "feat/b"),
            ("upstream", "feat/c"),
            ("upstream", "dev"),
        ]);
        assert_eq!(
            summarize_remote(&rows),
            vec![
                ("R", "origin".into(), "2".into()),
                ("S", "feat".into(), "2".into()),
                ("SL", "feat".into(), "a".into()),
                ("SL", "feat".into(), "b".into()),
                ("R", "upstream".into(), "2".into()),
                ("S", "feat".into(), "1".into()),
                ("SL", "feat".into(), "c".into()),
                ("RL", "dev".into(), String::new()),
            ]
        );
    }

    #[test]
    fn remote_deep_name_keeps_remainder() {
        // origin/feat/ui/x → origin ▸ feat ▸ ui/x (single sub-level split).
        let rows = group_rem(&[("origin", "feat/ui/x")]);
        assert_eq!(
            summarize_remote(&rows),
            vec![
                ("R", "origin".into(), "1".into()),
                ("S", "feat".into(), "1".into()),
                ("SL", "feat".into(), "ui/x".into()),
            ]
        );
    }

    #[test]
    fn remote_collapse_keys_unique_and_no_collision() {
        // Level-1 remote header vs level-2 sub-group vs local must all differ.
        assert_eq!(remote_key("origin"), "remote:origin");
        assert_eq!(remote_group_key("origin", "feat"), "remote:origin:feat");
        assert_eq!(remote_group_key("upstream", "feat"), "remote:upstream:feat");
        // Two remotes with the same sub-group prefix get distinct keys.
        assert_ne!(
            remote_group_key("origin", "feat"),
            remote_group_key("upstream", "feat")
        );
        // The remote header key (2 segments) never equals any sub-group key
        // (3 segments), and never matches a local key.
        assert_ne!(remote_key("origin"), remote_group_key("origin", "feat"));
        assert_ne!(
            remote_group_key("origin", "feat"),
            group_key(SECTION_LOCAL, "feat")
        );
    }

    #[test]
    fn remote_non_ascii_subgroup() {
        let rows = group_rem(&[("origin", "機能/あ")]);
        assert_eq!(
            summarize_remote(&rows),
            vec![
                ("R", "origin".into(), "1".into()),
                ("S", "機能".into(), "1".into()),
                ("SL", "機能".into(), "あ".into()),
            ]
        );
    }
}
