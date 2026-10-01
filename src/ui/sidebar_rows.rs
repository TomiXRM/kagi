//! Pure grouping and flattened navigator rows for the existing sidebar list.

use std::collections::HashSet;

use kagi_git::{CommitId, RemoteBranch, Stash, Tag, Worktree};

use super::sidebar::{
    SidebarRow, SECTION_LOCAL, SECTION_REMOTE, SECTION_STASHES, SECTION_TAGS, SECTION_WORKTREES,
};

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
pub(super) fn group_by_prefix<T: Clone>(
    items: &[T],
    name_of: impl Fn(&T) -> &str,
) -> Vec<GroupRow<T>> {
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
pub(super) fn split_first_segment(name: &str) -> Option<(String, String)> {
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
pub(super) fn group_key(section: &str, prefix: &str) -> String {
    format!("{section}:{prefix}")
}

/// Build the collapse key for a remote *name* level-1 header
/// (e.g. `"remote:origin"`).
pub(super) fn remote_key(remote: &str) -> String {
    format!("{SECTION_REMOTE}:{remote}")
}

/// Build the collapse key for a sub-group *within* a remote
/// (e.g. `"remote:origin:feat"`). This is namespaced by remote name so two
/// remotes can both have a `feat` sub-group without their collapse state
/// colliding, and it never collides with the level-1 remote header key
/// (which has no third segment) nor with local keys (`local:…`).
pub(super) fn remote_group_key(remote: &str, prefix: &str) -> String {
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
pub(super) fn group_remotes<T: Clone>(
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

/// T-PERF-RENDER-002 (ADR-0116 Wave 2): a cheap, allocation-free fingerprint of
/// the inputs to [`build_sidebar_rows`].
///
/// `render` recomputes this each frame and only rebuilds `rows` when it changes,
/// so unchanged frames skip the O(all-refs) clone+collect. The session owner is
/// part of the key so two tabs with equal per-session epochs cannot reuse each
/// other's rows. Heavy collection contents are covered by `view_epoch`;
/// collection lengths are an O(1) backstop. The collapsed sets and filter text
/// are hashed directly because they can change without the epoch.
#[allow(clippy::too_many_arguments)]
pub fn sidebar_rows_fingerprint(
    owner: Option<crate::app::SessionId>,
    view_epoch: u64,
    branches_len: usize,
    remote_branches_len: usize,
    tags_len: usize,
    stashes_len: usize,
    worktrees_len: usize,
    collapsed: &HashSet<&'static str>,
    groups_collapsed: &HashSet<String>,
    filter_text: &str,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    owner.hash(&mut hasher);
    view_epoch.hash(&mut hasher);
    branches_len.hash(&mut hasher);
    remote_branches_len.hash(&mut hasher);
    tags_len.hash(&mut hasher);
    stashes_len.hash(&mut hasher);
    worktrees_len.hash(&mut hasher);
    // Order-independent fold for the two collapse sets (HashSet iteration order
    // is non-deterministic, so XOR per-element hashes instead of hashing the set
    // in iteration order).
    let mut collapsed_acc: u64 = 0;
    for key in collapsed {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut h);
        collapsed_acc ^= h.finish();
    }
    collapsed_acc.hash(&mut hasher);
    let mut groups_acc: u64 = 0;
    for key in groups_collapsed {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut h);
        groups_acc ^= h.finish();
    }
    groups_acc.hash(&mut hasher);
    filter_text.hash(&mut hasher);
    hasher.finish()
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
    run_mode: kagi_domain::worktree_run_mode::RunMode,
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

    // ── WORKTREES ────────────────────────────────────────────────
    {
        let section_collapsed = collapsed.contains(SECTION_WORKTREES);
        rows.push(SidebarRow::SectionHeader {
            section: SECTION_WORKTREES,
            title: "WORKTREES",
            count: worktrees.iter().filter(|w| !w.is_main).count(),
            collapsed: section_collapsed,
        });
        if !section_collapsed {
            // #869: a shared-port mode hands every worktree the main
            // worktree's block, so every row links to it.
            let main_port = worktrees.iter().find(|w| w.is_main).and_then(|w| w.port);
            // The main worktree remains in the read model (and supplies the
            // shared port), but has no removable leaf in this navigator.
            for wt in worktrees.iter().filter(|w| {
                !w.is_main && (matches(&w.name) || matches(w.path.to_string_lossy().as_ref()))
            }) {
                rows.push(SidebarRow::Worktree {
                    name: wt.name.clone(),
                    path: wt.path.clone(),
                    path_label: wt.path.display().to_string(),
                    is_current: wt.is_current,
                    locked: wt.locked,
                    port: if run_mode.shares_ports() {
                        main_port
                    } else {
                        wt.port
                    },
                });
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
