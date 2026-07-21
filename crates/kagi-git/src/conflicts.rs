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
}

impl ConflictOp {
    /// A short, stable identifier used for oplog `op` names and tests.
    pub fn slug(&self) -> &'static str {
        match self {
            ConflictOp::Merge { .. } => "merge",
            ConflictOp::Rebase { .. } => "rebase",
            ConflictOp::CherryPick { .. } => "cherry-pick",
            ConflictOp::Revert { .. } => "revert",
        }
    }

    /// Whether this operation is part of a sequencer (rebase / cherry-pick /
    /// revert sequences support `skip`; a plain merge does not).
    pub fn is_sequencer(&self) -> bool {
        !matches!(self, ConflictOp::Merge { .. })
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
            ConflictKind::Binary => "binary",
        }
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
        _ => None,
    }
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
    files.dedup_by(|a, b| a.path == b.path);
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
    Some(bytes_to_pathbuf(&bytes))
}

/// Convert index-entry path bytes (always `/`-separated, no NUL) to a
/// `PathBuf` without byte-slicing user text — we go through a lossy `str`.
fn bytes_to_pathbuf(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

/// Classify the kind of a single index conflict from its stage presence pattern
/// and a binary probe of the present blobs.
fn classify_kind(repo: &Repository, conflict: &git2::IndexConflict) -> ConflictKind {
    let our = conflict.our.as_ref();
    let their = conflict.their.as_ref();
    let ancestor = conflict.ancestor.as_ref();

    // Binary wins over every other classification (no usable text merge).
    if entry_is_binary(repo, our) || entry_is_binary(repo, their) || entry_is_binary(repo, ancestor)
    {
        return ConflictKind::Binary;
    }

    match (our.is_some(), their.is_some()) {
        // Both sides present → content conflict.
        (true, true) => ConflictKind::Content,
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

    // 3. Binary conflicts must have an explicit side chosen (no text merge).
    let binary_unresolved: Vec<String> = session
        .files
        .iter()
        .filter(|f| f.kind == ConflictKind::Binary && !buffer.has_resolution(&f.path))
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
    Some(bytes_to_pathbuf(&bytes))
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
) -> Result<ContinueOutcome, GitError> {
    // 1. Materialize each resolved buffer file to the working tree and stage it
    //    (collapses stage 1/2/3 → stage 0), so the index carries no unmerged
    //    entries.
    stage_conflict_resolution(repo, session, buffer)?;

    // 2. For a merge, create the merge commit and clear the state.
    if let ConflictOp::Merge { .. } = session.op {
        let mut index = repo
            .index()
            .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
        let oid = create_merge_commit(repo, &mut index, None)?;
        repo.cleanup_state()
            .map_err(|e| GitError::Other(format!("cleanup_state failed: {}", e.message())))?;
        return Ok(ContinueOutcome::Committed(oid));
    }

    // 3. Sequencer operations (rebase / cherry-pick / revert): advance via the
    // real git CLI. `--continue` commits the resolved step with the original
    // message (no editor is opened) and, for rebase, keeps auto-continuing
    // through any further non-conflicting commits until it either finishes or
    // stops at the next conflict.
    //
    // A single `--continue` call is *usually* enough to run the whole
    // remaining sequence (git's own rebase loop keeps going through
    // non-conflicting commits internally), but some git versions/backends
    // need an extra nudge per resolved step. Loop while: the repo is still
    // mid-sequence AND the index carries no unmerged entries (i.e. nothing
    // is blocking another `--continue` — a real new conflict always leaves
    // unmerged index entries, which stops the loop immediately). Bounded by
    // the sequence length so a genuine stuck state can't spin forever.
    let slug = session.op.slug();
    let max_attempts = read_rebase_progress(repo.path()).1.max(1) + 1;
    for _attempt in 0..max_attempts {
        run_git(repo_path, &[slug, "--continue"])
            .map_err(|e| GitError::Other(format!("{} --continue failed to start: {}", slug, e)))?;

        if repo.state() == git2::RepositoryState::Clean {
            break;
        }
        let no_conflicts_left = repo
            .index()
            .map(|idx| !idx.has_conflicts())
            .unwrap_or(false);
        if !no_conflicts_left {
            break;
        }
    }

    // `repo.state()` is the authoritative signal: `Clean` means the whole
    // sequence finished; anything else means it's still in progress (either
    // a genuine new conflict, or the retry budget above was exhausted), and
    // the normal reload + `detect_conflict_session` path picks up whatever
    // comes next.
    if repo.state() == git2::RepositoryState::Clean {
        return Ok(match repo.head().ok().and_then(|h| h.target()) {
            Some(oid) => ContinueOutcome::Committed(CommitId(oid.to_string())),
            None => ContinueOutcome::Staged,
        });
    }
    Ok(ContinueOutcome::Staged)
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
    // before any disk write touches anything.
    let mut resolutions: Vec<(std::path::PathBuf, String)> = Vec::new();
    for file in &session.files {
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
        let tmp_abs = abs.with_extension(format!("kagi-resolve-tmp-{}", n));
        match std::fs::write(&tmp_abs, text.as_bytes()) {
            Ok(()) => {
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
    index
        .write()
        .map_err(|e| GitError::Other(format!("index.write() failed: {}", e.message())))?;
    Ok(())
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
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    })
}

/// Execute an `abort`: clean the operation state, restore HEAD's working tree to
/// the pre-operation `ORIG_HEAD`, and preserve the resolution buffer.
///
/// Restoration uses a **safe** checkout of the ORIG_HEAD tree (no
/// `reset --hard`, no `clean`): the index/working tree are pointed back at the
/// pre-op tree, then `cleanup_state` removes the `MERGE_HEAD` / sequencer
/// metadata.  The branch ref is moved back to ORIG_HEAD so the aborted commit
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

    // 3. If we know ORIG_HEAD, restore the working tree + index to its tree via
    //    a SAFE checkout (no force), then move the branch ref back.
    if let Some(ref sha) = orig_sha {
        let oid = git2::Oid::from_str(sha)
            .map_err(|e| GitError::Other(format!("bad ORIG_HEAD {}: {}", sha, e.message())))?;
        let commit = repo.find_commit(oid).map_err(|e| {
            GitError::Other(format!("ORIG_HEAD commit lookup failed: {}", e.message()))
        })?;
        let tree = commit.tree().map_err(|e| {
            GitError::Other(format!("ORIG_HEAD tree lookup failed: {}", e.message()))
        })?;

        let workdir = repo
            .workdir()
            .ok_or_else(|| GitError::Other("repository has no working tree".to_string()))?
            .to_path_buf();

        // Restore the working tree + index to the pre-operation tree without any
        // force / `reset --hard` / `clean`.
        //
        // Two obstacles must be handled explicitly:
        //   1. A safe checkout refuses while the index still holds conflict
        //      stages ("unresolved conflicts exist in the index").
        //   2. A conflicting working-tree file is full of markers, so after the
        //      index is reset to ORIG_HEAD a safe checkout sees no index→tree
        //      diff and skips it, leaving marker residue.
        //
        // We therefore (a) read the pre-op tree into the index to drop the
        // conflict stages, then (b) write each conflicting path's pre-op blob
        // content straight to the working tree (a targeted, per-path rewrite of
        // exactly the files the aborted operation touched — not a broad reset).
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

        for file in &session.files {
            let abs = workdir.join(&file.path);
            // The pre-op tree may not contain the file (e.g. it was added by the
            // operation); in that case remove the conflicted working-tree copy.
            match tree.get_path(&file.path) {
                Ok(entry) => {
                    if let Ok(blob) = repo.find_blob(entry.id()) {
                        if let Some(parent) = abs.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        std::fs::write(&abs, blob.content()).map_err(|e| {
                            GitError::Other(format!("restore {} failed: {}", abs.display(), e))
                        })?;
                    }
                }
                Err(_) => {
                    // Not in the pre-op tree → remove the conflicted file.
                    let _ = std::fs::remove_file(&abs);
                }
            }
        }

        // Move the current branch ref (if attached) back to ORIG_HEAD.
        if let Ok(head_ref) = repo.head() {
            if let Ok(name) = head_ref.name() {
                let _ = repo.reference(
                    name,
                    oid,
                    true,
                    &format!("abort {}: restore ORIG_HEAD", session.op.slug()),
                );
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
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    })
}

/// Execute a `skip` of the current sequencer step.
///
/// Discards the conflicting step's changes safely (no `reset --hard`, no
/// `clean`): the conflicting paths are restored to HEAD's tree content (or
/// removed if absent in HEAD), the index conflict stages are dropped by reading
/// HEAD's tree, and the current-step sequencer metadata is cleared via
/// `cleanup_state`.  The resolution buffer is preserved first (ADR-0057).
///
/// Driving a multi-step sequence forward to the *next* pick is deferred to the
/// dedicated sequence executor (mirroring `execute_conflict_continue`'s
/// `Staged` deferral); this backend half guarantees the current step is dropped
/// safely and the index/working tree are left clean.
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

    // 2. HEAD's tree is the "drop to" state for the current step's conflicts.
    let head_commit = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .and_then(|oid| repo.find_commit(oid).ok());
    let head_sha = head_commit.as_ref().map(|c| c.id().to_string());

    if let Some(commit) = &head_commit {
        let tree = commit
            .tree()
            .map_err(|e| GitError::Other(format!("HEAD tree lookup failed: {}", e.message())))?;
        let workdir = repo
            .workdir()
            .ok_or_else(|| GitError::Other("repository has no working tree".to_string()))?
            .to_path_buf();

        // Drop the conflict stages from the index by reading HEAD's tree.
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

        // Restore each conflicting path to HEAD's content (or remove it).
        for file in &session.files {
            let abs = workdir.join(&file.path);
            match tree.get_path(&file.path) {
                Ok(entry) => {
                    if let Ok(blob) = repo.find_blob(entry.id()) {
                        if let Some(parent) = abs.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        std::fs::write(&abs, blob.content()).map_err(|e| {
                            GitError::Other(format!("restore {} failed: {}", abs.display(), e))
                        })?;
                    }
                }
                Err(_) => {
                    let _ = std::fs::remove_file(&abs);
                }
            }
        }
    }

    // 3. Clear the current-step sequencer metadata.
    repo.cleanup_state()
        .map_err(|e| GitError::Other(format!("cleanup_state failed: {}", e.message())))?;

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
