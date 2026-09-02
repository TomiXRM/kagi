//! Conflict session detection + terminology + continue/abort backend —
//! W26-CONFLICT-CORE (T-CONFLICT-001 / 008 / 010).
//!
//! This module is **backend-only**: it builds a UI-free [`ConflictSession`]
//! describing an in-progress merge / rebase / cherry-pick / revert, classifies
//! the conflicting files, supplies the role-based terminology labels of
//! ADR-0058 (the words "ours"/"theirs" never appear in any user-facing string),
//! and plans the `continue` / `abort` operations on top of the existing
//! `OperationPlan` pipeline. No `src/ui/**` is touched; a later lane wires the
//! banner / panel.
//!
//! # Design
//!
//! - **Detection** (T-CONFLICT-001): [`detect_conflict_session`] reads
//!   [`Repository::state`] for the operation kind, then walks
//!   [`Index::conflicts`] to enumerate the conflicting paths.  The `step/total`
//!   of a rebase and the source `sha + summary` of a cherry-pick / revert come
//!   from the `.git/` state files (`rebase-merge/{msgnum,end}`,
//!   `CHERRY_PICK_HEAD`, `REVERT_HEAD`) because libgit2 does not expose them.
//! - **File kind classification** (ADR-0056): each conflict entry is mapped to
//!   `content` / `rename-delete` / `modify-delete` / `binary` from the presence
//!   pattern of its stage-1/2/3 entries plus a blob binary probe.
//! - **Terminology** (T-CONFLICT-010 / ADR-0058): [`side_labels`] returns the
//!   role + real-name label pair for the current and incoming side of an
//!   operation.  rebase translates the libgit2 ours/theirs swap into
//!   "New base" / "Your commit being replayed" — never raw ours/theirs.
//! - **continue / abort** (T-CONFLICT-008): [`plan_conflict_continue`] gates on
//!   unresolved files + marker residue then writes the resolution buffer to the
//!   working tree, stages, and continues the operation;
//!   [`plan_conflict_abort`] / [`execute_conflict_abort`] clean the operation
//!   state and restore the pre-op snapshot from `ORIG_HEAD`, **preserving the
//!   resolution buffer** in the autosave dir for later recovery (ADR-0057, the
//!   jj "never lose a partial resolution" principle).
//!
//! Hard rules honored: `chars()`-only on user text (no byte slicing of paths /
//! content); no force ops / `reset --hard` / `clean`; in-memory first (the repo
//! is untouched until `execute_*`).

use kagi_domain::plan_note::{
    ConflictsNote, ConflictsRecovery, ConflictsTitle, PlanDisposition, PlanNote, PlanRecovery,
    PlanTitle, RecoveryKind,
};
use std::path::{Path, PathBuf};

use git2::{Repository, RepositoryState};

use super::cli::run_git;
use super::log::CommitId;
use super::ops::{OperationPlan, StateSummary};
use super::resolution::ResolutionBuffer;
use super::status::working_tree_status;
use super::{resolve_head, GitError, Head};

// ────────────────────────────────────────────────────────────
// Public types — operation kind
// ────────────────────────────────────────────────────────────

/// The kind of in-progress operation that produced a conflict, with the extra
/// context needed to render progress and terminology.
///
/// Mirrors ADR-0056's `op` enum.  `Rebase` carries `step/total` (read from the
/// `.git/rebase-merge` state files); `CherryPick` / `Revert` carry the source
/// commit's short sha + summary so the UI can name the commit being applied /
/// undone without ever saying "theirs".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictOp {
    /// A `git merge` is in progress.
    Merge {
        /// Short sha of the branch / commit being merged in (from `MERGE_HEAD`),
        /// if it could be read.
        incoming: Option<String>,
        /// One-line summary of the incoming commit, if available.
        incoming_summary: Option<String>,
    },
    /// A `git rebase` (merge backend or interactive) is in progress.
    Rebase {
        /// 1-based index of the commit currently being replayed.
        step: usize,
        /// Total number of commits in the rebase.
        total: usize,
        /// Short sha of the commit currently being replayed, if available.
        commit: Option<String>,
        /// One-line summary of the commit being replayed, if available.
        commit_summary: Option<String>,
    },
    /// A `git cherry-pick` is in progress.
    CherryPick {
        /// Short sha of the commit being applied (from `CHERRY_PICK_HEAD`).
        source: Option<String>,
        /// One-line summary of the commit being applied.
        source_summary: Option<String>,
    },
    /// A `git revert` is in progress.
    Revert {
        /// Short sha of the commit being undone (from `REVERT_HEAD`).
        source: Option<String>,
        /// One-line summary of the commit being undone.
        source_summary: Option<String>,
    },
    /// A `git stash apply` / `pop` left conflicts (#309). Unlike the other ops
    /// this leaves `RepositoryState::Clean` (no MERGE_HEAD / sequencer state) —
    /// only the index carries unmerged entries. It is **not** a commit-producing
    /// operation: "continue" merely stages the resolved paths, and "abort"
    /// restores HEAD for the conflicted paths while leaving the stash intact.
    StashConflict,
}

impl ConflictOp {
    /// A short, stable identifier used for oplog `op` names and tests.
    pub fn slug(&self) -> &'static str {
        match self {
            ConflictOp::Merge { .. } => "merge",
            ConflictOp::Rebase { .. } => "rebase",
            ConflictOp::CherryPick { .. } => "cherry-pick",
            ConflictOp::Revert { .. } => "revert",
            ConflictOp::StashConflict => "stash",
        }
    }

    /// Whether this operation is part of a sequencer (rebase / cherry-pick /
    /// revert sequences support `skip`; a plain merge does not).
    pub fn is_sequencer(&self) -> bool {
        // Explicit allow-list (#309): only these three support `skip`. A merge
        // never did; a StashConflict (no sequencer state) must not either. Do not
        // reduce this to "anything but Merge".
        matches!(
            self,
            ConflictOp::Rebase { .. } | ConflictOp::CherryPick { .. } | ConflictOp::Revert { .. }
        )
    }
}

// ────────────────────────────────────────────────────────────
// Public types — conflicting file
// ────────────────────────────────────────────────────────────

/// How a single conflicting path conflicts (ADR-0056 `kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// Both sides changed overlapping content (stage 1/2/3 all present, text).
    Content,
    /// One side renamed while the other deleted (a stage is missing and the
    /// path differs across stages — best-effort detection).
    RenameDelete,
    /// One side modified while the other deleted (stage 2 or stage 3 missing).
    ModifyDelete,
    /// Both sides added the path independently with no common ancestor
    /// (add/add). A text merge is possible (see `merge_addadd`); the UI needs
    /// this signal to pick the base-less 3-way path (resolution.rs:601).
    AddAdd,
    /// A conflicted submodule / gitlink (git mode 160000). Resolved by staging a
    /// side's commit OID directly (#297), never by a text merge.
    Submodule,
    /// A conflicted symlink (git mode 120000). Resolved by staging a side's
    /// symlink blob OID directly (#297) — never by dereferencing the on-disk
    /// link, which would write outside the repo (#298).
    Symlink,
    /// At least one side is a binary blob (no usable text merge).
    Binary,
}

impl ConflictKind {
    /// Stable identifier for tests / logging.
    pub fn slug(&self) -> &'static str {
        match self {
            ConflictKind::Content => "content",
            ConflictKind::RenameDelete => "rename-delete",
            ConflictKind::ModifyDelete => "modify-delete",
            ConflictKind::AddAdd => "add-add",
            ConflictKind::Submodule => "submodule",
            ConflictKind::Symlink => "symlink",
            ConflictKind::Binary => "binary",
        }
    }

    /// Whether this kind is structurally unmergeable and must be resolved by
    /// staging a raw blob OID rather than a text buffer (#297).
    pub fn is_raw(&self) -> bool {
        matches!(
            self,
            ConflictKind::Binary | ConflictKind::Submodule | ConflictKind::Symlink
        )
    }
}

/// Resolution status of a single conflicting file within the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictStatus {
    /// Not yet resolved in the resolution buffer.
    Unresolved,
    /// A draft exists in the resolution buffer (chosen side or manual edit).
    Resolved,
    /// Resolved but flagged for review (e.g. marker residue detected).
    NeedsReview,
}

/// One conflicting file in a [`ConflictSession`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictFile {
    /// Repository-relative path of the conflicting file.
    pub path: PathBuf,
    /// How the file conflicts.
    pub kind: ConflictKind,
    /// Current resolution status (always [`ConflictStatus::Unresolved`] at
    /// detection time; the UI/buffer updates it later).
    pub status: ConflictStatus,
}

// ────────────────────────────────────────────────────────────
// Public types — session
// ────────────────────────────────────────────────────────────

/// A first-class snapshot of the repository's conflict state (ADR-0056).
///
/// Pure data, UI-free.  Produced by [`detect_conflict_session`]; consumed by the
/// continue/abort planners and (later) the UI lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictSession {
    /// What operation is in progress.
    pub op: ConflictOp,
    /// The conflicting files (sorted by path for deterministic display).
    pub files: Vec<ConflictFile>,
}

impl ConflictSession {
    /// Number of files not yet resolved in the buffer.
    pub fn unresolved_count(&self) -> usize {
        self.files
            .iter()
            .filter(|f| f.status == ConflictStatus::Unresolved)
            .count()
    }

    /// Total number of conflicting files.
    pub fn total_count(&self) -> usize {
        self.files.len()
    }
}

/// Re-resolve the selected conflict file across a re-detection that may have
/// re-sorted / renumbered `files` (issue #285). The stored index is meaningless
/// after a per-file Save folds a path out of `index.conflicts()` and the list is
/// rebuilt sorted-by-path, so selection must follow the **path**, not the index:
/// prefer the same path as before; else the first Unresolved file ("land on work
/// to do"); else index 0. Returns `None` only for an empty list.
pub fn resolve_selected_file(files: &[ConflictFile], prev_path: Option<&Path>) -> Option<usize> {
    prev_path
        .and_then(|p| files.iter().position(|f| f.path == p))
        .or_else(|| {
            files
                .iter()
                .position(|f| f.status == ConflictStatus::Unresolved)
        })
        .or_else(|| (!files.is_empty()).then_some(0))
}

// ────────────────────────────────────────────────────────────
// Detection (T-CONFLICT-001)
// ────────────────────────────────────────────────────────────

/// Detect an in-progress conflict session, or `None` if the repository is in a
/// clean (non-conflict) state.
///
/// Returns `Some` whenever [`Repository::state`] reports a merge / rebase /
/// cherry-pick / revert **and** the index has conflict entries.  A repository
/// mid-operation with all conflicts already staged (index clean) still reports
/// `Some` with an empty `files` list so the UI can offer "continue"; callers
/// distinguish via [`ConflictSession::total_count`].
///
/// Detection never mutates the repository.
pub fn detect_conflict_session(repo: &Repository) -> Option<ConflictSession> {
    let state = repo.state();
    let op = classify_op(repo, state)?;

    let files = collect_conflict_files(repo).unwrap_or_default();

    Some(ConflictSession { op, files })
}

/// Map a [`RepositoryState`] to a [`ConflictOp`], reading the `.git/` state
/// files for the extra context.  Returns `None` for non-conflict states.
fn classify_op(repo: &Repository, state: RepositoryState) -> Option<ConflictOp> {
    let git_dir = repo.path();
    match state {
        RepositoryState::Merge => {
            let (incoming, incoming_summary) = read_head_ref(repo, git_dir, "MERGE_HEAD");
            Some(ConflictOp::Merge {
                incoming,
                incoming_summary,
            })
        }
        RepositoryState::Rebase
        | RepositoryState::RebaseInteractive
        | RepositoryState::RebaseMerge => {
            let (step, total) = read_rebase_progress(git_dir);
            let (commit, commit_summary) = read_rebase_commit(repo, git_dir);
            Some(ConflictOp::Rebase {
                step,
                total,
                commit,
                commit_summary,
            })
        }
        RepositoryState::CherryPick | RepositoryState::CherryPickSequence => {
            let (source, source_summary) = read_head_ref(repo, git_dir, "CHERRY_PICK_HEAD");
            Some(ConflictOp::CherryPick {
                source,
                source_summary,
            })
        }
        RepositoryState::Revert | RepositoryState::RevertSequence => {
            let (source, source_summary) = read_head_ref(repo, git_dir, "REVERT_HEAD");
            Some(ConflictOp::Revert {
                source,
                source_summary,
            })
        }
        // #309: a conflicted `git stash apply`/`pop` leaves the repo Clean (no
        // MERGE_HEAD / sequencer state) but writes unmerged entries into the
        // index. Named-state arms above still win; this is only the
        // Clean-plus-unmerged-index case.
        _ => {
            if index_has_conflicts(repo) {
                Some(ConflictOp::StashConflict)
            } else {
                None
            }
        }
    }
}

/// Whether the repository index currently has unmerged (conflict) entries.
/// Best effort: any read failure reports `false` (no conflict to surface).
fn index_has_conflicts(repo: &Repository) -> bool {
    matches!(repo.index(), Ok(idx) if idx.has_conflicts())
}

/// Read a `.git/<name>` file holding a single object id, returning the short
/// sha and the commit's one-line summary (best effort; `(None, None)` on any
/// failure — detection must never error out over missing context).
fn read_head_ref(
    repo: &Repository,
    git_dir: &Path,
    name: &str,
) -> (Option<String>, Option<String>) {
    let raw = match std::fs::read_to_string(git_dir.join(name)) {
        Ok(s) => s,
        Err(_) => return (None, None),
    };
    let sha = raw.trim();
    if sha.is_empty() {
        return (None, None);
    }
    let short = short_sha(sha);
    let summary = git2::Oid::from_str(sha)
        .ok()
        .and_then(|oid| repo.find_commit(oid).ok())
        .and_then(|c| c.summary().ok().flatten().map(str::to_string));
    (Some(short), summary)
}

/// Read rebase `step/total` from `.git/rebase-merge/{msgnum,end}` (the merge
/// backend) falling back to `(0, 0)` when the files are absent (apply backend
/// or unexpected layout).
fn read_rebase_progress(git_dir: &Path) -> (usize, usize) {
    let dir = git_dir.join("rebase-merge");
    let step = read_trimmed_usize(&dir.join("msgnum")).unwrap_or(0);
    let total = read_trimmed_usize(&dir.join("end")).unwrap_or(0);
    (step, total)
}

/// Read the commit currently being replayed in a rebase from
/// `.git/rebase-merge/{stopped-sha,orig-head}` → short sha + summary.
fn read_rebase_commit(repo: &Repository, git_dir: &Path) -> (Option<String>, Option<String>) {
    let dir = git_dir.join("rebase-merge");
    // `stopped-sha` holds the commit that conflicted (merge backend, Git 2.x).
    for name in ["stopped-sha", "orig-head"] {
        if let Ok(raw) = std::fs::read_to_string(dir.join(name)) {
            let sha = raw.trim();
            if sha.is_empty() {
                continue;
            }
            let short = short_sha(sha);
            let summary = git2::Oid::from_str(sha)
                .ok()
                .and_then(|oid| repo.find_commit(oid).ok())
                .and_then(|c| c.summary().ok().flatten().map(str::to_string));
            return (Some(short), summary);
        }
    }
    (None, None)
}

/// Parse the first line of a file as a `usize`, ignoring surrounding whitespace.
fn read_trimmed_usize(path: &Path) -> Option<usize> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
}

/// First 8 chars of a sha (char-based; never byte-slices a possibly-multibyte
/// string — although shas are ASCII this keeps the hard-rule audit clean).
fn short_sha(sha: &str) -> String {
    sha.chars().take(8).collect()
}

/// Walk the index conflict iterator and classify every conflicting path.
fn collect_conflict_files(repo: &Repository) -> Result<Vec<ConflictFile>, GitError> {
    let index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;

    let conflicts = index
        .conflicts()
        .map_err(|e| GitError::Other(format!("index.conflicts() failed: {}", e.message())))?;

    let mut files: Vec<ConflictFile> = Vec::new();
    for entry in conflicts {
        let conflict = match entry {
            Ok(c) => c,
            Err(_) => continue,
        };
        let path = match conflict_path(&conflict) {
            Some(p) => p,
            None => continue,
        };
        let kind = classify_kind(repo, &conflict);
        files.push(ConflictFile {
            path,
            kind,
            status: ConflictStatus::Unresolved,
        });
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    // Collapse only *exact* duplicate entries (same path AND kind). Keying the
    // dedup on path alone dropped distinct sides of a rename/rename (git indexes
    // it as three unmerged entries at three paths — ancestor, our-target,
    // their-target); a same-path but different-kind pair (rare) also stays.
    files.dedup_by(|a, b| a.path == b.path && a.kind == b.kind);
    Ok(files)
}

/// Extract the path of a conflict from whichever stage entry is present.
fn conflict_path(conflict: &git2::IndexConflict) -> Option<PathBuf> {
    let bytes = conflict
        .our
        .as_ref()
        .or(conflict.their.as_ref())
        .or(conflict.ancestor.as_ref())
        .map(|e| e.path.clone())?;
    bytes_to_pathbuf(&bytes)
}

/// Convert index-entry path bytes (always `/`-separated, no NUL) to a
/// `PathBuf`, byte-faithfully (#293).
///
/// The old body went through `String::from_utf8_lossy`, so a non-UTF-8 conflict
/// name became a *different* path — and the resolution write path
/// (`workdir.join(rel)`) then created a bogus renamed file while the real
/// conflict stayed unresolved. On Unix we build the path from raw bytes; on
/// non-Unix a non-UTF-8 name yields `None` (the entry is skipped) rather than a
/// silently-wrong path. Non-Unix non-UTF-8 handling is out of scope (#293).
fn bytes_to_pathbuf(bytes: &[u8]) -> Option<PathBuf> {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return Some(PathBuf::from(s));
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
    }
    #[cfg(not(unix))]
    {
        eprintln!(
            "[kagi-git] conflicts: skipping conflict with non-UTF-8 path (unsupported on this platform)"
        );
        None
    }
}

/// Classify the kind of a single index conflict from its stage presence pattern
/// and a binary probe of the present blobs.
fn classify_kind(repo: &Repository, conflict: &git2::IndexConflict) -> ConflictKind {
    let our = conflict.our.as_ref();
    let their = conflict.their.as_ref();
    let ancestor = conflict.ancestor.as_ref();

    // Submodule / gitlink (mode 160000): no text merge — must stage a commit OID
    // (#297). Checked before Binary because a gitlink has no readable blob.
    if entry_has_mode(our, MODE_GITLINK)
        || entry_has_mode(their, MODE_GITLINK)
        || entry_has_mode(ancestor, MODE_GITLINK)
    {
        return ConflictKind::Submodule;
    }

    // Symlink (mode 120000): the blob is the link target *text*, but writing it
    // through the on-disk link escapes the repo (#298) — stage the OID (#297).
    if entry_has_mode(our, MODE_SYMLINK)
        || entry_has_mode(their, MODE_SYMLINK)
        || entry_has_mode(ancestor, MODE_SYMLINK)
    {
        return ConflictKind::Symlink;
    }

    // Binary wins over every other classification (no usable text merge).
    if entry_is_binary(repo, our) || entry_is_binary(repo, their) || entry_is_binary(repo, ancestor)
    {
        return ConflictKind::Binary;
    }

    match (our.is_some(), their.is_some()) {
        // Both sides present.
        (true, true) => {
            // No common ancestor → add/add (each side added the path). The UI's
            // base-less 3-way path (resolution.rs merge_addadd) needs this.
            if ancestor.is_none() {
                ConflictKind::AddAdd
            } else {
                ConflictKind::Content
            }
        }
        // Exactly one side present.  Distinguish rename/delete from
        // modify/delete: a rename leaves the surviving stage at a path that
        // differs from the ancestor's path.
        (true, false) | (false, true) => {
            let present = our.or(their);
            let renamed = match (present, ancestor) {
                (Some(p), Some(a)) => p.path != a.path,
                // No ancestor at all → an add/add or rename without base; treat
                // a differing-only-side as modify/delete unless paths reveal a
                // rename, which we cannot see here.
                _ => false,
            };
            if renamed {
                ConflictKind::RenameDelete
            } else {
                ConflictKind::ModifyDelete
            }
        }
        // Neither side present (only ancestor) → both deleted differently /
        // delete-delete; classify as modify/delete for UI purposes.
        (false, false) => ConflictKind::ModifyDelete,
    }
}

/// Git symlink / gitlink file modes (no text merge — resolved via a raw OID).
const MODE_SYMLINK: u32 = 0o120000;
const MODE_GITLINK: u32 = 0o160000;

/// Whether a present index entry carries exactly `mode`.
fn entry_has_mode(entry: Option<&git2::IndexEntry>, mode: u32) -> bool {
    matches!(entry, Some(e) if e.mode == mode)
}

/// Probe whether an index entry's blob is binary.  A missing entry or
/// unreadable blob is treated as non-binary (best effort).
fn entry_is_binary(repo: &Repository, entry: Option<&git2::IndexEntry>) -> bool {
    let entry = match entry {
        Some(e) => e,
        None => return false,
    };
    if entry.id.is_zero() {
        return false;
    }
    match repo.find_blob(entry.id) {
        Ok(blob) => blob.is_binary() || blob_has_nul(blob.content()),
        Err(_) => false,
    }
}

/// NUL-byte heuristic over the leading 8 KiB (matches `checklist.rs`).
fn blob_has_nul(content: &[u8]) -> bool {
    let probe = &content[..content.len().min(8 * 1024)];
    probe.contains(&0u8)
}

// ────────────────────────────────────────────────────────────
// Terminology (T-CONFLICT-010 / ADR-0058)
// ────────────────────────────────────────────────────────────

/// A single role + real-name label pair (ADR-0058 two-line label).
///
/// `role` is the translatable role word (e.g. "Current branch", "New base");
/// `name` is the real branch / commit name shown verbatim (never translated).
/// The words "ours" / "theirs" must never appear in `role`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideLabel {
    /// Role word (translatable via Msg in the UI lane).
    pub role: String,
    /// Real branch / commit name (verbatim, not translated).
    pub name: String,
}

impl SideLabel {
    fn new(role: &str, name: impl Into<String>) -> Self {
        SideLabel {
            role: role.to_string(),
            name: name.into(),
        }
    }
}

/// The current + incoming side labels for an operation, plus the base and result
/// roles (the four roles of §2: Base, current, incoming, Result).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideLabels {
    /// Left side (index stage 2 = libgit2 "ours") translated to a role name.
    pub current: SideLabel,
    /// Right side (index stage 3 = libgit2 "theirs") translated to a role name.
    pub incoming: SideLabel,
    /// Base (common ancestor) role label.
    pub base: SideLabel,
    /// Result (editable resolution) role label.
    pub result: SideLabel,
}

/// Produce the role + real-name labels for an operation (ADR-0058 §2 table).
///
/// `current_branch` is the short name of the branch HEAD is on (used for the
/// merge / cherry-pick / revert "Current branch" / "New base" left label).
///
/// The rebase direction swap (libgit2 reports onto as "ours", the replayed
/// commit as "theirs") is translated here so the UI never has to know: the
/// left/current label becomes **New base** and the right/incoming label becomes
/// **Your commit being replayed**.  The strings "ours"/"theirs" never appear.
pub fn side_labels(op: &ConflictOp, current_branch: &str) -> SideLabels {
    let base = SideLabel::new("Base", "common ancestor");
    let result = SideLabel::new("Result", "your resolution");

    match op {
        ConflictOp::Merge {
            incoming,
            incoming_summary,
        } => SideLabels {
            current: SideLabel::new("Current branch", current_branch),
            incoming: SideLabel::new("Merging in", commit_display(incoming, incoming_summary)),
            base,
            result,
        },
        ConflictOp::Rebase {
            commit,
            commit_summary,
            ..
        } => SideLabels {
            // Direction translation: libgit2 "ours" == the rebase target (onto),
            // surfaced to the user as the New base.
            current: SideLabel::new("New base", current_branch),
            // libgit2 "theirs" == the commit being replayed.
            incoming: SideLabel::new(
                "Your commit being replayed",
                commit_display(commit, commit_summary),
            ),
            base,
            result,
        },
        ConflictOp::CherryPick {
            source,
            source_summary,
        } => SideLabels {
            current: SideLabel::new("Current branch", current_branch),
            incoming: SideLabel::new(
                "Commit being applied",
                commit_display(source, source_summary),
            ),
            base,
            result,
        },
        ConflictOp::Revert {
            source,
            source_summary,
        } => SideLabels {
            current: SideLabel::new("Current branch", current_branch),
            incoming: SideLabel::new(
                "Changes being undone",
                commit_display(source, source_summary),
            ),
            base,
            result,
        },
        // #309: a stash apply/pop has no incoming commit — the "incoming" side is
        // the stashed changes themselves. Never "ours"/"theirs" (ADR-0058).
        ConflictOp::StashConflict => SideLabels {
            current: SideLabel::new("Current branch", current_branch),
            incoming: SideLabel::new("Stashed changes", "your stash"),
            base,
            result,
        },
    }
}

/// Real-name display for a commit: `"<sha> <summary>"`, `"<sha>"`, or
/// `"(unknown commit)"` — built with `chars()`-safe concatenation only.
fn commit_display(sha: &Option<String>, summary: &Option<String>) -> String {
    match (sha, summary) {
        (Some(s), Some(sum)) => format!("{} {}", s, sum),
        (Some(s), None) => s.clone(),
        (None, Some(sum)) => sum.clone(),
        (None, None) => "(unknown commit)".to_string(),
    }
}

// ────────────────────────────────────────────────────────────
// Continue gate (T-043 / T-044, ADR-0067) — structured blockers
// ────────────────────────────────────────────────────────────

/// A specific reason the Continue action is blocked (ADR-0067 checklist).
///
/// Each variant maps 1:1 to a checklist item so the UI can surface the exact
/// blocking reason next to the disabled Continue button.  The words
/// "ours"/"theirs" never appear (ADR-0058); file paths are carried verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinueBlocker {
    /// One or more detected files have no resolution draft in the buffer.
    UnresolvedFiles(Vec<String>),
    /// One or more resolved buffer texts still contain conflict markers.
    MarkerResidue(Vec<String>),
    /// The git index still has unmerged entries not tracked by the session.
    IndexUnmerged(Vec<String>),
    /// One or more binary conflicts are still unresolved (no side chosen).
    BinaryUnresolved(Vec<String>),
    /// A modify/delete or rename/delete file's keep-or-delete decision is still
    /// undecided (no resolution draft chosen for it).
    DeletionUndecided(Vec<String>),
    /// A merge commit is required but its message is empty.
    EmptyMergeMessage,
    /// The commit checklist (ADR-0043) reports a hard blocker.
    ChecklistBlocker(String),
}

impl ContinueBlocker {
    /// Stable identifier for tests / logging (never user-facing prose).
    pub fn code(&self) -> &'static str {
        match self {
            ContinueBlocker::UnresolvedFiles(_) => "unresolved-files",
            ContinueBlocker::MarkerResidue(_) => "marker-residue",
            ContinueBlocker::IndexUnmerged(_) => "index-unmerged",
            ContinueBlocker::BinaryUnresolved(_) => "binary-unresolved",
            ContinueBlocker::DeletionUndecided(_) => "deletion-undecided",
            ContinueBlocker::EmptyMergeMessage => "empty-merge-message",
            ContinueBlocker::ChecklistBlocker(_) => "checklist-blocker",
        }
    }
}

/// Compute the full ADR-0067 continue checklist for a session, returning every
/// blocking reason (empty == Continue is allowed).
///
/// This is the single source of truth shared by [`plan_conflict_continue`] (for
/// the plan modal's `blockers`) and the UI's Continue gate (which surfaces the
/// specific reason).  It strengthens the original unresolved + marker check
/// with: index has no untracked unmerged entries, no unresolved binary
/// conflict, no undecided required-file deletion, and a non-empty merge message
/// when a merge commit is needed.
///
/// The repository is read but never mutated.
pub fn continue_blockers(
    repo: &Repository,
    session: &ConflictSession,
    buffer: &ResolutionBuffer,
) -> Vec<ContinueBlocker> {
    let mut out: Vec<ContinueBlocker> = Vec::new();

    // 1. Every detected file must have a resolution draft.
    let unresolved: Vec<String> = session
        .files
        .iter()
        .filter(|f| !buffer.has_resolution(&f.path))
        .map(|f| f.path.to_string_lossy().into_owned())
        .collect();
    if !unresolved.is_empty() {
        out.push(ContinueBlocker::UnresolvedFiles(unresolved));
    }

    // 2. No marker residue in any resolved buffer text.
    let residue: Vec<String> = buffer
        .files_with_marker_residue()
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    if !residue.is_empty() {
        out.push(ContinueBlocker::MarkerResidue(residue));
    }

    // 3. Raw conflicts (binary / symlink / submodule) must have an explicit side
    //    chosen — there is no text merge, so a side is staged by OID (#297).
    let binary_unresolved: Vec<String> = session
        .files
        .iter()
        .filter(|f| f.kind.is_raw() && !buffer.has_resolution(&f.path))
        .map(|f| f.path.to_string_lossy().into_owned())
        .collect();
    if !binary_unresolved.is_empty() {
        out.push(ContinueBlocker::BinaryUnresolved(binary_unresolved));
    }

    // 4. Modify/delete + rename/delete files need an explicit keep-or-delete
    //    decision (a chosen resolution draft).
    let deletion_undecided: Vec<String> = session
        .files
        .iter()
        .filter(|f| {
            matches!(
                f.kind,
                ConflictKind::ModifyDelete | ConflictKind::RenameDelete
            ) && !buffer.has_resolution(&f.path)
        })
        .map(|f| f.path.to_string_lossy().into_owned())
        .collect();
    if !deletion_undecided.is_empty() {
        out.push(ContinueBlocker::DeletionUndecided(deletion_undecided));
    }

    // 5. The index must hold no unmerged entry that the session does not know
    //    about.  execute_continue stages the session's own files (collapsing
    //    their stages), but an unmerged path outside the session means a
    //    re-scan is needed before continuing.
    if let Ok(index) = repo.index() {
        if let Ok(conflicts) = index.conflicts() {
            let session_paths: std::collections::BTreeSet<PathBuf> =
                session.files.iter().map(|f| f.path.clone()).collect();
            let mut untracked_unmerged: Vec<String> = Vec::new();
            for entry in conflicts.flatten() {
                if let Some(path) = conflict_path_local(&entry) {
                    if !session_paths.contains(&path) {
                        untracked_unmerged.push(path.to_string_lossy().into_owned());
                    }
                }
            }
            if !untracked_unmerged.is_empty() {
                out.push(ContinueBlocker::IndexUnmerged(untracked_unmerged));
            }
        }
    }

    // 6. Merge commit needs a non-empty message (merge only — sequencer ops
    //    reuse the picked commit's message, so this gate is merge-specific).
    if let ConflictOp::Merge { .. } = session.op {
        if merge_message_is_empty(repo) {
            out.push(ContinueBlocker::EmptyMergeMessage);
        }
    }

    out
}

/// Extract a conflict's path from whichever index stage entry is present
/// (local copy; the detection path has its own private `conflict_path`).
fn conflict_path_local(conflict: &git2::IndexConflict) -> Option<PathBuf> {
    let bytes = conflict
        .our
        .as_ref()
        .or(conflict.their.as_ref())
        .or(conflict.ancestor.as_ref())
        .map(|e| e.path.clone())?;
    bytes_to_pathbuf(&bytes)
}

/// Whether the merge message (`MERGE_MSG`, comment lines stripped) is empty.
///
/// Git writes a default merge message to `MERGE_MSG`; an empty / comment-only
/// file means the user (or a `--no-commit` flow) left no message, which blocks
/// the merge commit.  A missing file is treated as **not empty** because
/// [`create_merge_commit`] synthesizes a default summary in that case.
fn merge_message_is_empty(repo: &Repository) -> bool {
    let raw = match std::fs::read_to_string(repo.path().join("MERGE_MSG")) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let meaningful = raw
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .any(|l| !l.trim().is_empty());
    !meaningful
}

/// Render a [`ContinueBlocker`] as a structured [`PlanNote`] for the plan
/// modal (ADR-0129 Phase 2 — was English-prose `format!`, now typed).
///
/// The UI lane localizes the *category* via `Msg` (`conflict_view::blocker_msg`,
/// keyed off `ContinueBlocker` directly, untouched by this migration); this is
/// the backend's plan-modal note, byte-identical to the original
/// `plan_conflict_continue` strings via `message_en()`.
fn blocker_note(b: &ContinueBlocker) -> PlanNote {
    match b {
        ContinueBlocker::UnresolvedFiles(files) => {
            PlanNote::Conflicts(ConflictsNote::UnresolvedFiles {
                files: files.clone(),
            })
        }
        ContinueBlocker::MarkerResidue(files) => {
            PlanNote::Conflicts(ConflictsNote::MarkerResidue {
                files: files.clone(),
            })
        }
        ContinueBlocker::IndexUnmerged(files) => {
            PlanNote::Conflicts(ConflictsNote::IndexUnmerged {
                files: files.clone(),
            })
        }
        ContinueBlocker::BinaryUnresolved(files) => {
            PlanNote::Conflicts(ConflictsNote::BinaryUnresolved {
                files: files.clone(),
            })
        }
        ContinueBlocker::DeletionUndecided(files) => {
            PlanNote::Conflicts(ConflictsNote::DeletionUndecided {
                files: files.clone(),
            })
        }
        ContinueBlocker::EmptyMergeMessage => PlanNote::Conflicts(ConflictsNote::EmptyMergeMessage),
        ContinueBlocker::ChecklistBlocker(msg) => {
            PlanNote::Conflicts(ConflictsNote::ChecklistBlocker {
                message: msg.clone(),
            })
        }
    }
}

/// Render a [`ContinueBlocker`] as an English sentence (used where only a
/// `String` is needed, e.g. wrapping into a [`GitError`]).
fn blocker_sentence(b: &ContinueBlocker) -> String {
    blocker_note(b).message_en()
}

// ────────────────────────────────────────────────────────────
// continue / abort (T-CONFLICT-008, backend half)
// ────────────────────────────────────────────────────────────

/// Outcome of an executed conflict continuation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinueOutcome {
    /// The operation produced a new commit (merge commit / sequencer step).
    Committed(CommitId),
    /// The buffer was written + staged but the operation needs another step
    /// (e.g. a multi-commit rebase) — caller should continue the sequence.
    Staged,
}

/// The result of [`execute_conflict_continue`]: the [`ContinueOutcome`] plus the
/// **measured** post-continue repository state (#296). The caller records
/// `after` in the oplog instead of the plan's *predicted* head, so a partial or
/// failed continuation is never logged as a clean success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinueResult {
    /// Whether the sequence committed / finished or is still mid-sequence.
    pub outcome: ContinueOutcome,
    /// The real HEAD + working-tree state after the continuation ran.
    pub after: StateSummary,
}

/// Where a [`plan_conflict_continue_route`] routes the Continue action
/// (ADR-0068 — Save/Continue/Commit are distinct operations).
///
/// A **merge** does NOT commit on Continue: it transitions to the commit message
/// panel pre-filled with a merge message, so the user edits it and presses the
/// commit button (which calls [`execute_merge_commit`]).  A **sequencer**
/// operation (rebase / cherry-pick / revert) produces a `--continue`
/// [`OperationPlan`] shown in the confirmation modal before the sequencer runs.
#[derive(Debug, Clone)]
pub enum ContinueRoute {
    /// Merge: open the commit message panel pre-filled with this merge message.
    /// No commit is created yet.
    MergeCommitPanel {
        /// The pre-filled merge commit message ("Merge <incoming> into <current>").
        message: String,
    },
    /// rebase / cherry-pick / revert: confirm this `<op> --continue` plan, then
    /// continue the sequencer.
    SequencerPlan(Box<OperationPlan>),
    /// #309 stash conflict: staging the resolved paths is the whole "continue"
    /// (no commit, no `<op> --continue`). After staging, the UI offers to drop
    /// the kept stash. The resolution is staged by [`execute_conflict_continue`].
    StashComplete,
}

/// Outcome of saving a single file's resolution (ADR-0068 Save resolution).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveOutcome {
    /// The path that was written + staged (repository-relative).
    pub path: PathBuf,
    /// Short hash of the resolved text that was written (for the oplog).
    pub after_short: String,
}

/// Outcome of an executed conflict abort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbortOutcome {
    /// Sha HEAD was restored to (the pre-operation `ORIG_HEAD`), if known.
    pub restored_to: Option<String>,
    /// Path the resolution buffer was preserved at, if a buffer was saved.
    pub buffer_preserved_at: Option<PathBuf>,
}

/// Plan a `continue`: validate that every file is resolved and free of marker
/// residue, then describe writing the buffer → working tree → stage →
/// operation continuation.
///
/// # Blockers (ADR-0056 "continue disabled until fully resolved")
///
/// - Any file still unresolved in the buffer.
/// - Any file whose **buffer text** still contains a conflict marker
///   (`<<<<<<< ` / `=======` / `>>>>>>> `), reusing the `checklist.rs`
///   detection (ADR-0043 rule 4).
///
/// The repository is not modified by this function.
pub fn plan_conflict_continue(
    repo: &Repository,
    session: &ConflictSession,
    buffer: &ResolutionBuffer,
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let current = current_state_summary(repo)?;

    let mut warnings: Vec<PlanNote> = Vec::new();

    // The full ADR-0067 checklist (T-043/044): unresolved + marker residue +
    // index unmerged + binary unresolved + undecided deletion + empty merge
    // message.  Each structured blocker is rendered to a typed plan note here.
    let structured = continue_blockers(repo, session, buffer);
    let blockers: Vec<PlanNote> = structured.iter().map(blocker_note).collect();

    if session.files.is_empty() && structured.is_empty() {
        warnings.push(PlanNote::Conflicts(
            ConflictsNote::NoConflictingFilesDetected,
        ));
    }

    let predicted = StateSummary {
        head: current.head.clone(),
        dirty: "resolved → staged".to_string(),
    };

    let op = session.op.slug().to_string();
    let recovery = PlanRecovery {
        kind: RecoveryKind::Conflicts(ConflictsRecovery::Continue { op: op.clone() }),
        commands: vec![format!("git {} --abort", op)],
    };

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Conflicts(ConflictsTitle::Continue { op }),
        current,
        predicted,
        warnings,
        blockers,
        recovery: Some(recovery),
        head_at_plan: head,
        stash_count_at_plan: 0,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    })
}

/// Execute a `continue`: write each resolved buffer file to the working tree,
/// stage it, and continue the operation.
///
/// For a merge this writes the merge commit (HEAD + MERGE_HEAD parents) and
/// clears the merge state. For sequencer operations (cherry-pick / revert /
/// rebase) this stages the resolution, then shells out `git <op> --continue`
/// (`run_git`, matching every other CLI-driven op in this codebase) to
/// actually commit the resolved step and advance — libgit2 exposes no
/// continue-a-sequence API, and reimplementing rebase's step machine by hand
/// would duplicate real git's own sequencer.
///
/// **Preconditions** (caller must check the plan first): no blockers.  This
/// function re-checks marker residue defensively but trusts resolution presence.
pub fn execute_conflict_continue(
    repo: &Repository,
    repo_path: &Path,
    session: &ConflictSession,
    buffer: &ResolutionBuffer,
) -> Result<ContinueResult, GitError> {
    // 1. Materialize each resolved buffer file to the working tree and stage it
    //    (collapses stage 1/2/3 → stage 0), so the index carries no unmerged
    //    entries.
    stage_conflict_resolution(repo, session, buffer)?;

    // 1b. #309 stash conflict: staging IS the continuation. A stash apply is not
    // a commit — do NOT create a merge commit and do NOT run `git stash
    // --continue` (which is not a command). The unmerged entries are now
    // collapsed to stage 0, so the index is conflict-free / commit-able, and the
    // kept stash is left for the UI's optional drop prompt. HEAD is unchanged.
    if let ConflictOp::StashConflict = session.op {
        return Ok(ContinueResult {
            outcome: ContinueOutcome::Staged,
            after: current_state_summary(repo)?,
        });
    }

    // 2. For a merge, create the merge commit and clear the state.
    if let ConflictOp::Merge { .. } = session.op {
        let mut index = repo
            .index()
            .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
        let oid = create_merge_commit(repo, &mut index, None)?;
        repo.cleanup_state()
            .map_err(|e| GitError::Other(format!("cleanup_state failed: {}", e.message())))?;
        return Ok(ContinueResult {
            outcome: ContinueOutcome::Committed(oid),
            after: current_state_summary(repo)?,
        });
    }

    // 3. Sequencer operations (rebase / cherry-pick / revert): advance via the
    // real git CLI. `--continue` commits the resolved step with the original
    // message (no editor is opened) and, for rebase, keeps auto-continuing
    // through any further non-conflicting commits until it either finishes or
    // stops at the next conflict.
    //
    // A single `--continue` call is *usually* enough to run the whole
    // remaining sequence. Loop, bounded by the sequence length, nudging once
    // per resolved step.
    //
    // #296(a): `--continue` exit status is authoritative. EVERY other run_git
    // caller checks `out.status`; this site used to ignore it, so a hard
    // refusal (e.g. "The previous cherry-pick is now empty", "You must edit all
    // merge conflicts") was swallowed and recorded as success. A non-zero exit
    // that leaves a *fresh* conflict (new unmerged entries, still mid-op) is the
    // normal "stop at the next conflict" — surfaced as `Staged`. A non-zero exit
    // with NO unmerged entries is a genuine failure and is returned as an error.
    let slug = session.op.slug();
    let max_attempts = read_rebase_progress(repo.path()).1.max(1) + 1;
    for _attempt in 0..max_attempts {
        let out = run_git(repo_path, &[slug, "--continue"])
            .map_err(|e| GitError::Other(format!("{} --continue failed to start: {}", slug, e)))?;

        // #296(b): libgit2 caches the index on the long-lived session repo and
        // does NOT re-read it after an external `git … --continue`. Force a
        // fresh read so `has_conflicts()` / state reflect what git just wrote,
        // not the pre-continue snapshot.
        let has_unmerged = fresh_index_has_conflicts(repo);
        let state = repo.state();

        if out.status != 0 {
            if has_unmerged && state != git2::RepositoryState::Clean {
                // Advancing the sequence hit a new conflict on a later commit —
                // legitimate; the reload + re-detect path picks it up.
                break;
            }
            return Err(GitError::Other(format!(
                "{} --continue failed (exit {}): {}",
                slug,
                out.status,
                out.stderr.trim()
            )));
        }

        if state == git2::RepositoryState::Clean {
            break;
        }
        if has_unmerged {
            // Clean exit but a new conflict remains (some backends): stop and let
            // the re-detect path present it.
            break;
        }
    }

    // Measure the REAL post-continue state (#296): `Clean` means the whole
    // sequence finished; anything else means it's still mid-sequence.
    let after = current_state_summary(repo)?;
    let outcome = if repo.state() == git2::RepositoryState::Clean {
        match repo.head().ok().and_then(|h| h.target()) {
            Some(oid) => ContinueOutcome::Committed(CommitId(oid.to_string())),
            None => ContinueOutcome::Staged,
        }
    } else {
        ContinueOutcome::Staged
    };
    Ok(ContinueResult { outcome, after })
}

/// Force a fresh read of the on-disk index and report whether it has unmerged
/// (conflict) entries. libgit2 caches the index on the long-lived `Repository`,
/// so after an external `git … --continue` the cached copy is stale (#296b).
fn fresh_index_has_conflicts(repo: &Repository) -> bool {
    match repo.index() {
        Ok(mut idx) => {
            let _ = idx.read(true);
            idx.has_conflicts()
        }
        Err(_) => false,
    }
}

/// Materialize every resolved buffer file to the working tree and stage it,
/// collapsing index stages 1/2/3 → stage 0 (resolution).
///
/// Shared by [`execute_conflict_continue`] and the UI merge-commit-panel route:
/// when `Continue` routes a merge to the commit panel, the resolutions must be
/// staged here (the per-file `Save` is optional, so the index may still hold
/// unmerged entries). Without this the commit panel sees nothing staged and
/// [`execute_merge_commit`] refuses the still-conflicted index. Writes nothing
/// to refs and creates no commit.
///
/// Defensively refuses if conflict markers remain in the buffer.
pub fn stage_conflict_resolution(
    repo: &Repository,
    session: &ConflictSession,
    buffer: &ResolutionBuffer,
) -> Result<(), GitError> {
    // Defensive re-check: never write markers into a commit.
    let residue = buffer.files_with_marker_residue();
    if !residue.is_empty() {
        return Err(GitError::Other(
            "Refusing to continue: conflict markers remain in the resolution buffer.".to_string(),
        ));
    }

    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("repository has no working tree".to_string()))?
        .to_path_buf();

    let mut index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;

    // ADR-0106: atomic save. A naive per-file `fs::write` loop leaves the
    // working tree half-resolved if a write fails mid-loop (files 1..k
    // overwritten, index never written, original markers gone). Instead we
    // write every resolution to a sibling temp, then rename them all to their
    // targets only once every write succeeded. Rename is atomic on POSIX and
    // Windows for same-filesystem moves, so a failure never produces a
    // WT/index mismatch. Temp files are cleaned up on any error path.
    //
    // Collect (target, resolved_text) up front so a missing resolution aborts
    // before any disk write touches anything. Raw conflicts (binary / symlink /
    // gitlink, #297) carry no text — they are staged by OID below and never
    // written through the working tree (so a conflicted symlink can never be
    // dereferenced, #298).
    let mut resolutions: Vec<(std::path::PathBuf, String)> = Vec::new();
    let mut raw_stages: Vec<(std::path::PathBuf, super::resolution::RawResolution)> = Vec::new();
    for file in &session.files {
        if let Some(raw) = buffer.raw_resolution(&file.path) {
            raw_stages.push((file.path.clone(), raw));
            continue;
        }
        let text = match buffer.resolved_text(&file.path) {
            Some(t) => t,
            None => {
                return Err(GitError::Other(format!(
                    "no resolution for {} — re-plan before executing",
                    file.path.display()
                )));
            }
        };
        resolutions.push((file.path.clone(), text));
    }

    // Phase 1: write each resolution to `<name>.kagi-resolve-tmp-<n>` next to
    // its target (same filesystem → rename is atomic). Create parent dirs.
    let mut temps: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new(); // (temp, target_abs)
    for (n, (rel, text)) in resolutions.iter().enumerate() {
        let abs = workdir.join(rel);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                GitError::Other(format!("mkdir {} failed: {}", parent.display(), e))
            })?;
        }
        // #298: preserve the original file mode. Writing a fresh temp creates
        // 0644; renaming it over a 0755 target and then `index.add_path` would
        // silently drop the exec bit — making Continue disagree with the
        // in-place per-file Save. Capture the target's mode (a symlink target is
        // never reached here: symlinks are raw-staged above) and re-apply it to
        // the temp before the rename.
        let target_mode = original_file_mode(&abs);
        let tmp_abs = abs.with_extension(format!("kagi-resolve-tmp-{}", n));
        match std::fs::write(&tmp_abs, text.as_bytes()) {
            Ok(()) => {
                apply_file_mode(&tmp_abs, target_mode);
                temps.push((tmp_abs, abs));
            }
            Err(e) => {
                // Roll back: delete every temp written so far, leave targets
                // untouched. The working tree is still in its original
                // conflict state.
                for (t, _) in &temps {
                    let _ = std::fs::remove_file(t);
                }
                return Err(GitError::Other(format!(
                    "write {} failed: {} (no files were modified)",
                    abs.display(),
                    e
                )));
            }
        }
    }

    // Phase 2: every write succeeded — atomically swap temps onto targets.
    // `rename` is atomic per-file on POSIX/Windows for same-filesystem moves,
    // but the loop is NOT transactional across files: if file k's rename fails,
    // files 1..k-1 have already been renamed (their targets now hold the new
    // resolution). We accept this (a same-FS rename failing is extremely rare,
    // and the index is never written on the failure path so the repo still
    // reports a conflict — the user can re-resolve). We MUST clean up the
    // unrenamed temps (k..end) so they don't leak as untracked files.
    for (i, (tmp_abs, abs)) in temps.iter().enumerate() {
        if let Err(e) = std::fs::rename(tmp_abs, abs) {
            // Clean up every temp that hasn't been renamed yet (this one + the
            // rest), so no `.kagi-resolve-tmp-*` files leak into the worktree.
            for (unrenamed_tmp, _) in temps.iter().skip(i) {
                let _ = std::fs::remove_file(unrenamed_tmp);
            }
            return Err(GitError::Other(format!(
                "rename {} -> {} failed: {} (files before this point were \
                 already resolved; the index was not written, so the conflict \
                 is still recorded — re-resolve and retry)",
                tmp_abs.display(),
                abs.display(),
                e
            )));
        }
    }

    // Phase 3: stage every resolved path and flush the index once.
    for (rel, _text) in &resolutions {
        index.add_path(rel).map_err(|e| {
            GitError::Other(format!("stage {} failed: {}", rel.display(), e.message()))
        })?;
    }
    // Raw (binary / symlink / gitlink) resolutions: stage the chosen side's OID
    // at stage 0 directly (#297) — byte-identical to the chosen side, exec bit /
    // symlink / gitlink mode intact, and no working-tree write at all (#298).
    for (rel, raw) in &raw_stages {
        stage_raw_entry(&mut index, rel, *raw)?;
    }
    index
        .write()
        .map_err(|e| GitError::Other(format!("index.write() failed: {}", e.message())))?;
    Ok(())
}

/// Stage a raw resolution (a chosen side's blob/commit OID + git mode) at stage
/// 0 by building an [`git2::IndexEntry`] and `index.add` (#297). No working-tree
/// write happens, so a conflicted symlink is never dereferenced (#298) and a
/// binary is staged byte-for-byte.
fn stage_raw_entry(
    index: &mut git2::Index,
    rel: &Path,
    raw: super::resolution::RawResolution,
) -> Result<(), GitError> {
    let path_bytes = path_to_index_bytes(rel).ok_or_else(|| {
        GitError::Other(format!(
            "cannot stage {}: non-representable path",
            rel.display()
        ))
    })?;
    // Drop the stage 1/2/3 conflict entries first: `index.add` of a stage-0
    // entry does NOT displace higher-stage entries at the same path, so without
    // this the path stays unmerged ("not fully merged index" on write_tree).
    index.conflict_remove(rel).map_err(|e| {
        GitError::Other(format!(
            "clear conflict {} failed: {}",
            rel.display(),
            e.message()
        ))
    })?;
    let entry = git2::IndexEntry {
        ctime: git2::IndexTime::new(0, 0),
        mtime: git2::IndexTime::new(0, 0),
        dev: 0,
        ino: 0,
        mode: raw.mode,
        uid: 0,
        gid: 0,
        file_size: 0,
        id: raw.oid,
        // flags == 0 → stage 0 (resolved). Setting a stage would re-conflict it.
        flags: 0,
        flags_extended: 0,
        path: path_bytes,
    };
    index
        .add(&entry)
        .map_err(|e| GitError::Other(format!("stage {} failed: {}", rel.display(), e.message())))
}

/// Repository-relative path → index-entry path bytes (`/`-separated, byte
/// faithful on Unix; lossy elsewhere, where non-UTF-8 is already unsupported).
fn path_to_index_bytes(rel: &Path) -> Option<Vec<u8>> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(rel.as_os_str().as_bytes().to_vec())
    }
    #[cfg(not(unix))]
    {
        rel.to_str().map(|s| s.as_bytes().to_vec())
    }
}

/// The current git file mode of a working-tree path, if it exists as a regular
/// file (`symlink_metadata` — never follows a symlink). `None` when absent.
fn original_file_mode(abs: &Path) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::symlink_metadata(abs).ok()?;
        if meta.file_type().is_symlink() {
            return None;
        }
        Some(meta.permissions().mode())
    }
    #[cfg(not(unix))]
    {
        let _ = abs;
        None
    }
}

/// Re-apply a captured file mode to a freshly-written temp (best effort). On
/// non-Unix, or when no prior mode was captured, this is a no-op.
fn apply_file_mode(abs: &Path, mode: Option<u32>) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Some(m) = mode {
            let _ = std::fs::set_permissions(abs, std::fs::Permissions::from_mode(m));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (abs, mode);
    }
}

/// Build a merge commit from the staged index with HEAD + MERGE_HEAD parents.
///
/// `message_override` (the commit panel's edited message) takes precedence;
/// otherwise the `MERGE_MSG` file is used, falling back to a synthesized line.
fn create_merge_commit(
    repo: &Repository,
    index: &mut git2::Index,
    message_override: Option<&str>,
) -> Result<CommitId, GitError> {
    let tree_oid = index
        .write_tree_to(repo)
        .map_err(|e| GitError::Other(format!("write_tree failed: {}", e.message())))?;
    let tree = repo
        .find_tree(tree_oid)
        .map_err(|e| GitError::Other(format!("find_tree failed: {}", e.message())))?;

    let head_commit = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .and_then(|oid| repo.find_commit(oid).ok())
        .ok_or_else(|| GitError::Other("HEAD commit lookup failed".to_string()))?;

    let merge_head_oid = std::fs::read_to_string(repo.path().join("MERGE_HEAD"))
        .ok()
        .and_then(|s| git2::Oid::from_str(s.trim()).ok())
        .ok_or_else(|| GitError::Other("MERGE_HEAD missing or unreadable".to_string()))?;
    let merge_commit = repo.find_commit(merge_head_oid).map_err(|e| {
        GitError::Other(format!("MERGE_HEAD commit lookup failed: {}", e.message()))
    })?;

    let message = match message_override {
        Some(m) => m.to_string(),
        None => std::fs::read_to_string(repo.path().join("MERGE_MSG"))
            .unwrap_or_else(|_| format!("Merge commit {}", short_sha(&merge_head_oid.to_string()))),
    };

    let sig = super::ops::build_signature(repo)?;
    let oid = repo
        .commit(
            Some("HEAD"),
            &sig,
            &sig,
            &message,
            &tree,
            &[&head_commit, &merge_commit],
        )
        .map_err(|e| GitError::Other(format!("merge commit failed: {}", e.message())))?;

    Ok(CommitId(oid.to_string()))
}

// ────────────────────────────────────────────────────────────
// Save resolution (ADR-0068 — T-CONFLICT-UX-013/014)
// ────────────────────────────────────────────────────────────

/// Save a single file's resolution: write the resolved Result to the working
/// tree, verify no conflict markers remain (a hard block), then **stage** the
/// path so its unmerged index entries (stage 1/2/3) collapse to stage 0.
///
/// This is GitKraken's per-file Save → stage step (ADR-0068): it does NOT create
/// any commit.  After it returns the index reports the path as resolved (stage 0)
/// so external `git status` and the continue gate agree.
///
/// # Errors
/// - the file has no resolution draft in the buffer,
/// - the resolved text still contains conflict markers (marker-residue block),
/// - any working-tree write / index operation fails.
pub fn execute_conflict_save(
    repo: &Repository,
    buffer: &ResolutionBuffer,
    path: &Path,
) -> Result<SaveOutcome, GitError> {
    // #297/#298: a raw (binary / symlink / gitlink) resolution stages the chosen
    // side's OID directly — no working-tree write, so a conflicted symlink is
    // never dereferenced and a binary is saved byte-for-byte, mode intact.
    if let Some(raw) = buffer.raw_resolution(path) {
        let mut index = repo
            .index()
            .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
        stage_raw_entry(&mut index, path, raw)?;
        index
            .write()
            .map_err(|e| GitError::Other(format!("index.write() failed: {}", e.message())))?;
        return Ok(SaveOutcome {
            path: path.to_path_buf(),
            after_short: short_sha(&raw.oid.to_string()),
        });
    }

    let text = buffer.resolved_text(path).ok_or_else(|| {
        GitError::Other(format!(
            "no resolution to save for {} — choose a side or edit the result first",
            path.display()
        ))
    })?;

    // Marker-residue check: a Save that still has markers is blocked (ADR-0066 /
    // ADR-0068).  Reuse the checklist detector so the gate and Save agree.
    if super::checklist::text_has_conflict_marker(&text) {
        return Err(GitError::Other(format!(
            "Cannot save {}: conflict markers (<<<<<<< ======= >>>>>>>) remain. Remove them first.",
            path.display()
        )));
    }

    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("repository has no working tree".to_string()))?
        .to_path_buf();

    // 1. Materialize the resolved text to the working tree.
    let abs = workdir.join(path);
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| GitError::Other(format!("mkdir {} failed: {}", parent.display(), e)))?;
    }
    // #298: never write text *through* a symlink (that escapes the repo). A
    // conflicted symlink is routed to the raw path above, so reaching here with
    // a symlink on disk is anomalous — refuse rather than dereference it.
    if let Ok(meta) = std::fs::symlink_metadata(&abs) {
        if meta.file_type().is_symlink() {
            return Err(GitError::Other(format!(
                "refusing to write {} through a symlink (resolve it as a symlink conflict)",
                abs.display()
            )));
        }
    }
    std::fs::write(&abs, text.as_bytes())
        .map_err(|e| GitError::Other(format!("write {} failed: {}", abs.display(), e)))?;

    // 2. Stage the path: index.add_path collapses stage 1/2/3 → stage 0.
    let mut index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
    index.add_path(path).map_err(|e| {
        GitError::Other(format!("stage {} failed: {}", path.display(), e.message()))
    })?;
    index
        .write()
        .map_err(|e| GitError::Other(format!("index.write() failed: {}", e.message())))?;

    Ok(SaveOutcome {
        path: path.to_path_buf(),
        after_short: short_text_hash(&text),
    })
}

/// A short content hash of resolved text for the oplog (FNV-1a, 8 hex chars;
/// `chars()`-safe — hashes the UTF-8 bytes, never byte-slices the string).
fn short_text_hash(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:08x}", (h & 0xffff_ffff) as u32)
}

// ────────────────────────────────────────────────────────────
// Continue routing (ADR-0068 — T-CONFLICT-FLOW-030/031/032)
// ────────────────────────────────────────────────────────────

/// Decide how Continue should proceed once the gate is clear (ADR-0068).
///
/// Gates on the full [`continue_blockers`] checklist first (returns the first
/// blocker as an error so the caller surfaces it).  Then:
/// - **merge** → [`ContinueRoute::MergeCommitPanel`] with a pre-filled merge
///   message (read from `MERGE_MSG`, else synthesized "Merge <incoming> into
///   <current>").  **No commit is created here** — the commit panel's commit
///   button calls [`execute_merge_commit`].
/// - **rebase / cherry-pick / revert** → [`ContinueRoute::SequencerPlan`] wrapping
///   the existing `<op> --continue` [`OperationPlan`] for the confirmation modal.
///
/// The repository is not modified.
pub fn plan_conflict_continue_route(
    repo: &Repository,
    session: &ConflictSession,
    buffer: &ResolutionBuffer,
    current_branch: &str,
) -> Result<ContinueRoute, GitError> {
    // Hard gate: refuse to route while any blocker stands (ADR-0067).
    let blockers = continue_blockers(repo, session, buffer);
    if let Some(first) = blockers.first() {
        return Err(GitError::Other(blocker_sentence(first)));
    }

    match &session.op {
        ConflictOp::Merge { .. } => {
            let message = prefilled_merge_message(repo, &session.op, current_branch);
            Ok(ContinueRoute::MergeCommitPanel { message })
        }
        // #309: staging is the whole continue for a stash conflict.
        ConflictOp::StashConflict => Ok(ContinueRoute::StashComplete),
        _ => {
            let plan = plan_conflict_continue(repo, session, buffer)?;
            Ok(ContinueRoute::SequencerPlan(Box::new(plan)))
        }
    }
}

/// The pre-filled merge commit message: `MERGE_MSG` (comment lines stripped) when
/// it carries text, else a synthesized "Merge <incoming> into <current>" line
/// using the ADR-0058 role labels (never ours/theirs).  `chars()`-safe joins.
fn prefilled_merge_message(repo: &Repository, op: &ConflictOp, current_branch: &str) -> String {
    if let Ok(raw) = std::fs::read_to_string(repo.path().join("MERGE_MSG")) {
        let meaningful: String = raw
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        if !meaningful.trim().is_empty() {
            return meaningful.trim_end().to_string();
        }
    }
    let labels = side_labels(op, current_branch);
    format!(
        "Merge {} into {}",
        labels.incoming.name, labels.current.name
    )
}

/// Create the merge commit for the commit-panel Commit button (ADR-0068).
///
/// Stages no files (Save already staged them); writes the current index as the
/// tree and commits with **two parents** (HEAD + MERGE_HEAD), then cleans up the
/// merge state (`cleanup_state` removes MERGE_HEAD / MERGE_MSG).  Refuses if the
/// index still has unmerged entries (a defensive re-check of the gate).
///
/// Returns the new merge commit's [`CommitId`].
pub fn execute_merge_commit(repo: &Repository, message: &str) -> Result<CommitId, GitError> {
    if message.trim().is_empty() {
        return Err(GitError::Other(
            "merge commit message must not be empty".to_string(),
        ));
    }

    let mut index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
    if index.has_conflicts() {
        return Err(GitError::Other(
            "Refusing to create the merge commit: the index still has unmerged entries. Save every file first.".to_string(),
        ));
    }

    let oid = create_merge_commit(repo, &mut index, Some(message))?;
    repo.cleanup_state()
        .map_err(|e| GitError::Other(format!("cleanup_state failed: {}", e.message())))?;
    Ok(oid)
}

/// Plan an `abort`: describe restoring the pre-operation state and preserving
/// the resolution buffer.  Always available (no blockers) per ADR-0056.
pub fn plan_conflict_abort(
    repo: &Repository,
    session: &ConflictSession,
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let current = current_state_summary(repo)?;

    let orig = read_orig_head(repo);
    let predicted_head = match &orig {
        Some(sha) => format!("restored to {}", short_sha(sha)),
        None => current.head.clone(),
    };

    let warnings = vec![PlanNote::Conflicts(
        ConflictsNote::PartialResolutionsPreserved,
    )];

    let op = session.op.slug().to_string();
    let recovery = PlanRecovery {
        kind: RecoveryKind::Conflicts(ConflictsRecovery::Abort { op: op.clone() }),
        commands: Vec::new(),
    };

    Ok(OperationPlan {
        disposition: PlanDisposition::Ready,
        title: PlanTitle::Conflicts(ConflictsTitle::Abort { op }),
        current,
        predicted: StateSummary {
            head: predicted_head,
            dirty: "clean".to_string(),
        },
        warnings,
        blockers: Vec::new(),
        recovery: Some(recovery),
        head_at_plan: head,
        stash_count_at_plan: 0,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    })
}

/// Execute an `abort`: clean the operation state, restore HEAD's working tree to
/// the pre-operation `ORIG_HEAD`, and preserve the resolution buffer.
///
/// Restoration is a `checkout_tree` of the ORIG_HEAD tree **restricted to the
/// paths the aborted operation itself wrote** (no `reset --hard`, no `clean`):
/// the index is read back to the pre-op tree, those paths are rewritten from
/// it (files the operation added are removed), then `cleanup_state` removes the
/// `MERGE_HEAD` / sequencer metadata.  Paths the operation never touched are
/// outside the pathspec and are never looked at, so unrelated local work
/// survives.  The branch ref is moved back to ORIG_HEAD so the aborted commit
/// chain is detached (recoverable via reflog).
///
/// The `buffer` is flushed to the autosave directory first so a partial
/// resolution is never lost (ADR-0057); its path is returned for the oplog
/// entry the caller writes.
pub fn execute_conflict_abort(
    repo: &Repository,
    session: &ConflictSession,
    buffer: &ResolutionBuffer,
) -> Result<AbortOutcome, GitError> {
    // 1. Preserve the buffer BEFORE touching the repo (never lose partial work).
    let buffer_preserved_at = buffer.autosave().ok();

    // 2. Resolve ORIG_HEAD (the pre-operation HEAD).
    let orig_sha = read_orig_head(repo);

    // 3. If we know ORIG_HEAD, restore the working tree + index to its tree,
    //    then move the branch ref back.
    if let Some(ref sha) = orig_sha {
        let oid = git2::Oid::from_str(sha)
            .map_err(|e| GitError::Other(format!("bad ORIG_HEAD {}: {}", sha, e.message())))?;
        let commit = repo.find_commit(oid).map_err(|e| {
            GitError::Other(format!("ORIG_HEAD commit lookup failed: {}", e.message()))
        })?;
        let tree = commit.tree().map_err(|e| {
            GitError::Other(format!("ORIG_HEAD tree lookup failed: {}", e.message()))
        })?;

        if repo.workdir().is_none() {
            return Err(GitError::Other(
                "repository has no working tree".to_string(),
            ));
        }

        // Restore the working tree + index to the pre-operation tree.
        //
        // The old implementation only rewrote `session.files` (the *conflicting*
        // paths) and left every cleanly-merged incoming file — and every file
        // the operation added — on disk (issue #278: 9,997 stray "modified"
        // files after a 10k-file merge).  The operation checked out its whole
        // result tree, so the whole result tree has to be rolled back.
        //
        // The path set is computed *before* anything is mutated, so a failure
        // there leaves the conflicted state intact rather than half-restored.
        let touched = op_touched_paths(repo, &tree, session)?;

        // Refuse if the user edited a *cleanly-merged* file while in Conflict
        // Mode.  The force-checkout below is justified by "whatever stands at
        // a touched path is the operation's own output" — which is true when
        // the conflict state was entered, but not necessarily at abort time:
        // Conflict Mode is an editing session, and nothing stops the user
        // from changing a non-conflicted file through the Editor meanwhile.
        // Real git refuses exactly here ("Entry 'b.txt' not uptodate. Cannot
        // merge.") and this matches it.  Conflicted paths are exempt: abort
        // discards resolution progress by design.
        //
        // Checked BEFORE any mutation, so the refusal leaves the conflicted
        // state fully intact.
        let edited = mid_conflict_edits(repo, session, &touched, oid, &tree)?;
        if !edited.is_empty() {
            return Err(GitError::Other(format!(
                "abort refused: {} file(s) were edited during conflict resolution and would be overwritten: {}. Commit, stash or revert those edits first.",
                edited.len(),
                edited.join(", ")
            )));
        }

        {
            let mut index = repo
                .index()
                .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
            index
                .read_tree(&tree)
                .map_err(|e| GitError::Other(format!("index.read_tree failed: {}", e.message())))?;
            index
                .write()
                .map_err(|e| GitError::Other(format!("index.write failed: {}", e.message())))?;
        }

        checkout_paths_from_tree(repo, &tree, &touched)?;

        // Restore the branch ref back to ORIG_HEAD and reattach HEAD to it.
        //
        // #302: during a rebase HEAD is DETACHED, so `repo.head().name()` is
        // literally "HEAD" — the old code then wrote `HEAD` as a *direct* ref
        // pointing at ORIG_HEAD, stranding the user in detached HEAD. Real
        // `git rebase --abort` returns to the branch. The pre-op branch name is
        // recorded in `.git/rebase-merge/head-name` (merge backend) or
        // `.git/rebase-apply/head-name` (apply backend); read it, point that
        // branch at ORIG_HEAD, and `set_head` to it. Merge / cherry-pick /
        // revert keep a symbolic HEAD, so `repo.head().name()` already yields
        // the branch — that path is unchanged. The error is no longer swallowed.
        let reflog = format!("abort {}: restore ORIG_HEAD", session.op.slug());
        let branch_ref: Option<String> = match session.op {
            ConflictOp::Rebase { .. } => read_rebase_head_name(repo.path()),
            _ => repo
                .head()
                .ok()
                .and_then(|h| h.name().map(str::to_string).ok())
                .filter(|n| n != "HEAD"),
        };
        match branch_ref {
            Some(name) => {
                repo.reference(&name, oid, true, &reflog).map_err(|e| {
                    GitError::Other(format!(
                        "restore {} to ORIG_HEAD failed: {}",
                        name,
                        e.message()
                    ))
                })?;
                repo.set_head(&name).map_err(|e| {
                    GitError::Other(format!("reattach HEAD to {} failed: {}", name, e.message()))
                })?;
            }
            None => {
                // Genuinely detached (no branch to return to): point HEAD at
                // ORIG_HEAD directly, as before.
                repo.set_head_detached(oid).map_err(|e| {
                    GitError::Other(format!(
                        "set detached HEAD to ORIG_HEAD failed: {}",
                        e.message()
                    ))
                })?;
            }
        }
    }

    // 4. Clear merge / sequencer metadata (MERGE_HEAD, CHERRY_PICK_HEAD, etc.).
    repo.cleanup_state()
        .map_err(|e| GitError::Other(format!("cleanup_state failed: {}", e.message())))?;

    Ok(AbortOutcome {
        restored_to: orig_sha,
        buffer_preserved_at,
    })
}

/// Abort a stash-conflict (#309 / ADR-0148): restore HEAD's content for the
/// **conflicted paths only** and clear their unmerged index entries, leaving the
/// stash entry intact.
///
/// A conflicted `git stash apply`/`pop` writes **no** `ORIG_HEAD`, `MERGE_HEAD`
/// or sequencer state — the branch never moved and the pre-apply tree is exactly
/// HEAD (apply requires a clean tree). So this must NOT reuse
/// [`execute_conflict_abort`]'s ORIG_HEAD path: there is no ref to move and no
/// operation state to clean up. It only:
///
/// 1. preserves the resolution buffer (ADR-0057),
/// 2. drops the stage 1/2/3 conflict entries for the session's files, and
/// 3. force-checks-out HEAD over exactly those paths (pathspec-bounded — never a
///    repo-wide `reset --hard`), which repopulates them at stage 0 and rewrites
///    the working tree.
///
/// The stash entry is left untouched (dropping it, if wanted, is a separate,
/// explicit stash-drop op).
pub fn execute_stash_conflict_abort(
    repo: &Repository,
    session: &ConflictSession,
    buffer: &ResolutionBuffer,
) -> Result<AbortOutcome, GitError> {
    // 1. Preserve the buffer BEFORE touching the repo (never lose partial work).
    let buffer_preserved_at = buffer.autosave().ok();

    if repo.workdir().is_none() {
        return Err(GitError::Other(
            "repository has no working tree".to_string(),
        ));
    }

    // 2. Resolve the HEAD commit + tree (the pre-apply state).
    let head_commit = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .and_then(|oid| repo.find_commit(oid).ok())
        .ok_or_else(|| GitError::Other("HEAD commit lookup failed".to_string()))?;
    let tree = head_commit
        .tree()
        .map_err(|e| GitError::Other(format!("HEAD tree lookup failed: {}", e.message())))?;

    // 3. Conflicted paths only (pathspec-bounded restore).
    let paths: Vec<String> = session
        .files
        .iter()
        .filter_map(|f| f.path.to_str().map(str::to_string))
        .collect();

    // 4. Drop the stage 1/2/3 conflict entries so the paths are no longer
    //    unmerged; the checkout below repopulates them at stage 0 from HEAD.
    {
        let mut index = repo
            .index()
            .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
        for f in &session.files {
            // Best effort: a path with no conflict entry is already resolved.
            let _ = index.conflict_remove(&f.path);
        }
        index
            .write()
            .map_err(|e| GitError::Other(format!("index.write failed: {}", e.message())))?;
    }

    // 5. Restore HEAD content for exactly those paths (force, pathspec-bounded).
    checkout_paths_from_tree(repo, &tree, &paths)?;

    // NOTE: no `cleanup_state`, no ref move, no ORIG_HEAD — there is none. The
    // stash entry is deliberately left intact.
    Ok(AbortOutcome {
        restored_to: Some(head_commit.id().to_string()),
        buffer_preserved_at,
    })
}

/// Paths among `touched` that are NOT conflicted and that the user edited (or
/// re-created) during Conflict Mode on top of the operation's output.  These
/// are the paths the abort's `read_tree` + force-checkout would destroy, so
/// their presence blocks the abort.
///
/// Two kinds of mid-conflict edit are caught:
///
/// 1. **Unstaged** — working tree differs from the index (`diff_index_to_workdir`).
/// 2. **Staged** (#307) — the user edited a non-conflicted file *and* `git add`ed
///    it, so index == workdir and the diff in (1) sees nothing.  The staged blob
///    would be silently lost by `index.read_tree(&tree)` and the force-checkout.
///    Detect it by reconstructing the operation's *own* clean-merge output and
///    flagging any non-conflicted path whose current index blob differs from it.
///    Reconstruction (not a raw index→tree diff) is what keeps the operation's
///    legitimate clean merges — including auto-merged hunks that match neither
///    parent — from being false-flagged.
///
/// `orig_oid` / `orig_tree` are ORIG_HEAD's oid and tree (the abort target).
fn mid_conflict_edits(
    repo: &Repository,
    session: &ConflictSession,
    touched: &[String],
    orig_oid: git2::Oid,
    orig_tree: &git2::Tree<'_>,
) -> Result<Vec<String>, GitError> {
    let conflicted: std::collections::BTreeSet<&str> = session
        .files
        .iter()
        .filter_map(|f| f.path.to_str())
        .collect();
    let non_conflicted: Vec<&str> = touched
        .iter()
        .map(String::as_str)
        .filter(|p| !conflicted.contains(p))
        .collect();

    let mut edited: Vec<String> = Vec::new();

    // (1) Unstaged edits: working tree differs from the index.
    if !non_conflicted.is_empty() {
        let mut opts = git2::DiffOptions::new();
        // A file the op deleted and the user re-created shows up as untracked.
        opts.include_untracked(true);
        opts.disable_pathspec_match(true);
        for p in &non_conflicted {
            opts.pathspec(*p);
        }
        let diff = repo
            .diff_index_to_workdir(None, Some(&mut opts))
            .map_err(|e| {
                GitError::Other(format!("diff index → workdir failed: {}", e.message()))
            })?;
        edited.extend(
            diff.deltas()
                .filter_map(|d| d.new_file().path().or_else(|| d.old_file().path()))
                .filter_map(|p| p.to_str().map(str::to_string)),
        );
    }

    // (2) Staged edits: current index blob differs from the operation's own
    //     reconstructed clean-merge output (#307).
    if let Some(result) = reconstruct_op_result(repo, session, orig_oid, orig_tree)? {
        let current = repo
            .index()
            .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
        for p in &non_conflicted {
            let path = Path::new(p);
            let cur = current.get_path(path, 0).map(|e| e.id);
            let res = result.get_path(path, 0).map(|e| e.id);
            if cur != res {
                edited.push((*p).to_string());
            }
        }
    }

    edited.sort();
    edited.dedup();
    Ok(edited)
}

/// Reconstruct the operation's own clean-merge result index — what git wrote at
/// conflict time before the user could touch it — so a staged mid-conflict edit
/// (#307) can be told apart from the operation's legitimate output.
///
/// Only the **merge** case is reconstructed (ORIG_HEAD × MERGE_HEAD over their
/// merge-base); it is the case the commit-panel staging path (#307) and the
/// abort test-suite exercise.  For the sequencer ops (rebase / cherry-pick /
/// revert) this returns `None`, so their guard keeps the pre-#307 unstaged-only
/// behavior rather than risk a false refusal — a staged edit there still slips
/// through as before (tracked as the remaining slice of #307).
fn reconstruct_op_result<'r>(
    repo: &'r Repository,
    session: &ConflictSession,
    orig_oid: git2::Oid,
    orig_tree: &git2::Tree<'r>,
) -> Result<Option<git2::Index>, GitError> {
    if !matches!(session.op, ConflictOp::Merge { .. }) {
        return Ok(None);
    }
    let merge_oid = match std::fs::read_to_string(repo.path().join("MERGE_HEAD"))
        .ok()
        .and_then(|s| git2::Oid::from_str(s.trim()).ok())
    {
        Some(o) => o,
        None => return Ok(None),
    };
    let incoming_tree = repo
        .find_commit(merge_oid)
        .and_then(|c| c.tree())
        .map_err(|e| GitError::Other(format!("MERGE_HEAD tree lookup failed: {}", e.message())))?;
    let base_tree =
        match repo.merge_base(orig_oid, merge_oid) {
            Ok(base_oid) => Some(repo.find_commit(base_oid).and_then(|c| c.tree()).map_err(
                |e| GitError::Other(format!("merge-base tree lookup failed: {}", e.message())),
            )?),
            // No common ancestor (unrelated histories): a base-less 2-way merge.
            Err(_) => None,
        };
    let index = repo
        .merge_trees(
            base_tree.as_ref().unwrap_or(orig_tree),
            orig_tree,
            &incoming_tree,
            None,
        )
        .map_err(|e| GitError::Other(format!("merge_trees reconstruct failed: {}", e.message())))?;
    Ok(Some(index))
}

/// Every path the in-progress operation wrote into the working tree.
///
/// That is the diff between the pre-op `tree` and the operation-result index
/// (cleanly-merged modifications, additions and deletions), plus the session's
/// conflicting paths — those carry only stage 1/2/3 entries, so they can be
/// reported inconsistently by a tree↔index diff and are added explicitly.
///
/// Non-UTF-8 paths are dropped: they cannot be expressed as a libgit2
/// pathspec.  (Tracked separately as issue #293 — the whole codebase loses
/// non-UTF-8 paths today; this function does not make that worse.)
fn op_touched_paths(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    session: &ConflictSession,
) -> Result<Vec<String>, GitError> {
    let mut paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    let diff = repo
        .diff_tree_to_index(Some(tree), None, None)
        .map_err(|e| {
            GitError::Other(format!("diff pre-op tree → index failed: {}", e.message()))
        })?;
    for delta in diff.deltas() {
        for file in [delta.old_file(), delta.new_file()] {
            if let Some(p) = file.path().and_then(|p| p.to_str()) {
                paths.insert(p.to_string());
            }
        }
    }

    for file in &session.files {
        if let Some(p) = file.path.to_str() {
            paths.insert(p.to_string());
        }
    }

    Ok(paths.into_iter().collect())
}

/// Check `tree` out over exactly `paths` (and nothing else), removing paths the
/// tree does not contain.
///
/// # Why this is `force()` and still safe
///
/// A *safe* checkout cannot do this job: the index has just been read back to
/// `tree`, so libgit2 sees no index→tree diff for these paths, treats the
/// operation's own output on disk as a local modification and skips it —
/// exactly the residue this is meant to clear.  `force()` compares the working
/// tree against `tree` directly and rewrites it.
///
/// The force is bounded by the pathspec to the paths the aborted operation
/// itself wrote, and whatever wrote them refused to overwrite local
/// modifications: kagi's own merge/cherry-pick/revert use
/// `CheckoutBuilder::safe()`, and the operations that did NOT go through a
/// kagi checkout — rebase (shelled out to `git`, ops/rebase.rs) and conflicts
/// entered from the CLI and picked up by the watcher — were written by real
/// git, which equally refuses to clobber locally-modified files.  So the
/// content standing at these paths is the operation's own output, never
/// pre-operation user work — dropping it is precisely what
/// `git merge --abort` does.  (Edits made DURING Conflict Mode are the one
/// exception, and `mid_conflict_edits` blocks the abort on those first.)  Paths outside the pathspec (including unrelated
/// dirty and untracked files) are not candidates — with the caveat that
/// libgit2's `disable_pathspec_match` disables fnmatch but keeps dirname
/// prefix matching, so a pathspec entry that names a directory (e.g. a
/// gitlink delta) would cover its contents.  `remove_untracked` is
/// likewise pathspec-bounded: it removes the files the operation *added*
/// (untracked once the index is back at `tree`), which is again what real
/// `git merge --abort` does.
fn checkout_paths_from_tree(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    paths: &[String],
) -> Result<(), GitError> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut cb = git2::build::CheckoutBuilder::new();
    cb.force();
    cb.remove_untracked(true);
    cb.disable_pathspec_match(true);
    for path in paths {
        cb.path(path.as_str());
    }
    repo.checkout_tree(tree.as_object(), Some(&mut cb))
        .map_err(|e| GitError::Other(format!("checkout_tree (abort) failed: {}", e.message())))
}

// ────────────────────────────────────────────────────────────
// Skip (T-042, ADR-0067) — sequencer-only
// ────────────────────────────────────────────────────────────

/// Outcome of an executed sequencer skip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipOutcome {
    /// Sha HEAD points at after dropping the skipped step's changes.
    pub head: Option<String>,
    /// Path the resolution buffer was preserved at, if a buffer was saved.
    pub buffer_preserved_at: Option<PathBuf>,
}

/// Plan a `skip` of the current sequencer step (rebase / cherry-pick / revert).
///
/// **Merge has no skip** — a plain merge is a single step, so this errors for
/// [`ConflictOp::Merge`] (the UI hides the button for merge; this is the
/// backend guard).  Skip discards the current pick's changes and advances the
/// sequencer (ADR-0067).  Plan-based: the repository is not modified here.
pub fn plan_conflict_skip(
    repo: &Repository,
    session: &ConflictSession,
) -> Result<OperationPlan, GitError> {
    if !session.op.is_sequencer() {
        return Err(GitError::Other(
            "skip is only available for rebase / cherry-pick / revert (a merge has no skip)."
                .to_string(),
        ));
    }

    let head = resolve_head(repo)?;
    let current = current_state_summary(repo)?;

    let warnings = vec![PlanNote::Conflicts(ConflictsNote::SkipDiscardsStep)];
    let op = session.op.slug().to_string();
    let recovery = PlanRecovery {
        kind: RecoveryKind::Conflicts(ConflictsRecovery::Skip { op: op.clone() }),
        commands: Vec::new(),
    };

    Ok(OperationPlan {
        disposition: PlanDisposition::Ready,
        title: PlanTitle::Conflicts(ConflictsTitle::Skip { op }),
        current,
        predicted: StateSummary {
            head: head_display(&head),
            dirty: "current step dropped".to_string(),
        },
        warnings,
        blockers: Vec::new(),
        recovery: Some(recovery),
        head_at_plan: head,
        stash_count_at_plan: 0,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    })
}

/// Execute a `skip` of the current sequencer step.
///
/// Shells out to `git <op> --skip` (`run_git`, matching
/// [`execute_conflict_continue`]'s `--continue`), which drops the current
/// pick's changes and advances the sequencer to the next one.  The resolution
/// buffer is preserved first (ADR-0057).
///
/// This replaces a hand-rolled per-file restore + `cleanup_state()` (issue
/// #278): `git_repository_state_cleanup` deletes `rebase-merge/` /
/// `rebase-apply/` / `sequencer/` wholesale, so "skip one step" silently threw
/// away every remaining pick and left HEAD detached mid-sequence.  Only real
/// git's sequencer can advance one step; libgit2 exposes no such API.
pub fn execute_conflict_skip(
    repo: &Repository,
    session: &ConflictSession,
    buffer: &ResolutionBuffer,
) -> Result<SkipOutcome, GitError> {
    if !session.op.is_sequencer() {
        return Err(GitError::Other(
            "skip is only available for sequencer operations.".to_string(),
        ));
    }

    // 1. Preserve the buffer first (never lose partial work).
    let buffer_preserved_at = buffer.autosave().ok();

    // 2. Everything else is one fallible step: either `git <op> --skip`
    //    succeeds and the sequencer is coherently on the next pick, or it
    //    fails and git left the current step untouched.  No half state.
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("repository has no working tree".to_string()))?
        .to_path_buf();
    let slug = session.op.slug();
    let out = run_git(&workdir, &[slug, "--skip"])
        .map_err(|e| GitError::Other(format!("{} --skip failed to start: {}", slug, e)))?;
    if out.status != 0 {
        return Err(GitError::Other(format!(
            "{} --skip failed (exit {}): {}",
            slug,
            out.status,
            out.stderr.trim()
        )));
    }

    // 3. HEAD as git left it (unchanged for a dropped single pick; advanced if
    //    the sequencer replayed further commits).
    let head_sha = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .map(|oid| oid.to_string());

    Ok(SkipOutcome {
        head: head_sha,
        buffer_preserved_at,
    })
}

/// Display string for a [`Head`] (mirrors `current_state_summary`'s head line).
fn head_display(head: &Head) -> String {
    match head {
        Head::Attached { branch, .. } => format!("branch: {}", branch),
        Head::Detached { target } => format!("detached: {}", short_sha(target)),
        Head::Unborn { branch } => format!("unborn ({})", branch),
    }
}

/// Read the pre-rebase branch ref name (e.g. `refs/heads/feature`) from
/// `.git/rebase-merge/head-name` (merge backend) or `.git/rebase-apply/head-name`
/// (apply backend). Returns `None` when neither file exists or it doesn't name a
/// ref — the caller then falls back to a detached restore (#302).
fn read_rebase_head_name(git_dir: &Path) -> Option<String> {
    for sub in ["rebase-merge", "rebase-apply"] {
        if let Ok(raw) = std::fs::read_to_string(git_dir.join(sub).join("head-name")) {
            let name = raw.trim();
            if name.starts_with("refs/") {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Read `ORIG_HEAD` as a 40-char sha string, if present.
fn read_orig_head(repo: &Repository) -> Option<String> {
    let raw = std::fs::read_to_string(repo.path().join("ORIG_HEAD")).ok()?;
    let sha = raw.trim();
    if sha.is_empty() {
        None
    } else {
        Some(sha.to_string())
    }
}

/// Build a [`StateSummary`] for the repository's current state.
fn current_state_summary(repo: &Repository) -> Result<StateSummary, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let dirty_parts: Vec<String> = [
        (!status.staged.is_empty()).then(|| format!("{} staged", status.staged.len())),
        (!status.unstaged.is_empty()).then(|| format!("{} modified", status.unstaged.len())),
        (!status.untracked.is_empty()).then(|| format!("{} untracked", status.untracked.len())),
        (!status.conflicted.is_empty()).then(|| format!("{} conflicted", status.conflicted.len())),
    ]
    .into_iter()
    .flatten()
    .collect();
    let dirty = if dirty_parts.is_empty() {
        "clean".to_string()
    } else {
        dirty_parts.join(", ")
    };
    Ok(StateSummary {
        head: head_display(&head),
        dirty,
    })
}

// ────────────────────────────────────────────────────────────
// Unit tests (pure helpers; repo-backed behaviour in tests/conflicts_test.rs)
// ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_stable() {
        let merge = ConflictOp::Merge {
            incoming: None,
            incoming_summary: None,
        };
        assert_eq!(merge.slug(), "merge");
        assert!(!merge.is_sequencer());

        let cp = ConflictOp::CherryPick {
            source: None,
            source_summary: None,
        };
        assert_eq!(cp.slug(), "cherry-pick");
        assert!(cp.is_sequencer());
    }

    #[test]
    fn kind_slugs() {
        assert_eq!(ConflictKind::Content.slug(), "content");
        assert_eq!(ConflictKind::RenameDelete.slug(), "rename-delete");
        assert_eq!(ConflictKind::ModifyDelete.slug(), "modify-delete");
        assert_eq!(ConflictKind::Binary.slug(), "binary");
    }

    #[test]
    fn merge_labels_use_roles_not_ours_theirs() {
        let op = ConflictOp::Merge {
            incoming: Some("abc12345".to_string()),
            incoming_summary: Some("add feature".to_string()),
        };
        let labels = side_labels(&op, "main");
        assert_eq!(labels.current.role, "Current branch");
        assert_eq!(labels.current.name, "main");
        assert_eq!(labels.incoming.role, "Merging in");
        assert!(labels.incoming.name.contains("abc12345"));
        assert!(labels.incoming.name.contains("add feature"));
        assert_no_ours_theirs(&labels);
    }

    #[test]
    fn rebase_labels_translate_direction() {
        let op = ConflictOp::Rebase {
            step: 2,
            total: 5,
            commit: Some("deadbeef".to_string()),
            commit_summary: Some("work in progress".to_string()),
        };
        let labels = side_labels(&op, "main");
        // The rebase target (libgit2 "ours") becomes "New base".
        assert_eq!(labels.current.role, "New base");
        assert_eq!(labels.current.name, "main");
        // The replayed commit (libgit2 "theirs") becomes the replay label.
        assert_eq!(labels.incoming.role, "Your commit being replayed");
        assert!(labels.incoming.name.contains("deadbeef"));
        assert_no_ours_theirs(&labels);
    }

    #[test]
    fn cherry_pick_and_revert_labels() {
        let cp = ConflictOp::CherryPick {
            source: Some("c0ffee".to_string()),
            source_summary: Some("fix bug".to_string()),
        };
        let labels = side_labels(&cp, "main");
        assert_eq!(labels.incoming.role, "Commit being applied");
        assert_no_ours_theirs(&labels);

        let rv = ConflictOp::Revert {
            source: Some("badc0de".to_string()),
            source_summary: Some("undo me".to_string()),
        };
        let labels = side_labels(&rv, "main");
        assert_eq!(labels.incoming.role, "Changes being undone");
        assert_no_ours_theirs(&labels);
    }

    #[test]
    fn base_and_result_roles_always_present() {
        let op = ConflictOp::Merge {
            incoming: None,
            incoming_summary: None,
        };
        let labels = side_labels(&op, "main");
        assert_eq!(labels.base.role, "Base");
        assert_eq!(labels.result.role, "Result");
    }

    #[test]
    fn commit_display_variants() {
        assert_eq!(
            commit_display(&Some("abc".to_string()), &Some("msg".to_string())),
            "abc msg"
        );
        assert_eq!(commit_display(&Some("abc".to_string()), &None), "abc");
        assert_eq!(commit_display(&None, &Some("msg".to_string())), "msg");
        assert_eq!(commit_display(&None, &None), "(unknown commit)");
    }

    #[test]
    fn short_sha_is_char_safe() {
        assert_eq!(short_sha("0123456789abcdef"), "01234567");
        assert_eq!(short_sha("abc"), "abc");
    }

    // ── Issue #285: selection follows PATH across a re-sort/renumber ──

    fn cf(path: &str, status: ConflictStatus) -> ConflictFile {
        ConflictFile {
            path: PathBuf::from(path),
            kind: ConflictKind::Content,
            status,
        }
    }

    #[test]
    fn resolve_selected_follows_path_across_renumber() {
        use ConflictStatus::*;
        // Before Save of c.txt: files sorted a,b,c,d — user on c.txt (index 2).
        let before = [
            cf("a.txt", Unresolved),
            cf("b.txt", Unresolved),
            cf("c.txt", Unresolved),
            cf("d.txt", Unresolved),
        ];
        let prev = before[2].path.clone();
        // After Save: c.txt folded out, list re-sorted a,b,d (index 2 == d.txt).
        let after = [
            cf("a.txt", Unresolved),
            cf("b.txt", Unresolved),
            cf("d.txt", Unresolved),
        ];
        // The OLD index (2) would land on d.txt — the bug. Path resolution must
        // NOT: c.txt is gone, so fall back to the first Unresolved (a.txt), not d.
        let idx = resolve_selected_file(&after, Some(&prev));
        assert_eq!(idx, Some(0), "gone path must fall back to first unresolved");
        assert_eq!(after[idx.unwrap()].path, PathBuf::from("a.txt"));
    }

    #[test]
    fn resolve_selected_keeps_same_path_when_present() {
        use ConflictStatus::*;
        // A file was resolved above the current one, shifting its index down.
        let after = [cf("b.txt", Unresolved), cf("c.txt", Unresolved)];
        let prev = PathBuf::from("c.txt"); // was index 2, now index 1
        let idx = resolve_selected_file(&after, Some(&prev));
        assert_eq!(idx, Some(1));
        assert_eq!(after[idx.unwrap()].path, PathBuf::from("c.txt"));
    }

    #[test]
    fn resolve_selected_prefers_unresolved_then_zero() {
        use ConflictStatus::*;
        // No prev path: land on the first Unresolved even if a Resolved precedes.
        let files = [cf("a.txt", Resolved), cf("b.txt", Unresolved)];
        assert_eq!(resolve_selected_file(&files, None), Some(1));
        // All resolved + no prev: fall back to index 0.
        let all_done = [cf("a.txt", Resolved), cf("b.txt", Resolved)];
        assert_eq!(resolve_selected_file(&all_done, None), Some(0));
        // Empty list: None.
        assert_eq!(resolve_selected_file(&[], Some(&PathBuf::from("x"))), None);
    }

    /// Assert no label role/name contains the forbidden words.
    fn assert_no_ours_theirs(labels: &SideLabels) {
        for l in [
            &labels.current,
            &labels.incoming,
            &labels.base,
            &labels.result,
        ] {
            let role = l.role.to_lowercase();
            assert!(!role.contains("ours"), "role leaked 'ours': {}", l.role);
            assert!(!role.contains("theirs"), "role leaked 'theirs': {}", l.role);
        }
    }
}
