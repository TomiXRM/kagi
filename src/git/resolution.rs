//! Resolution buffer + per-file undo / provenance + autosave — W26-CONFLICT-CORE
//! (T-CONFLICT-005, ADR-0057).
//!
//! The pure conflict hunk model lives in `kagi-domain` (ADR-0072). This backend
//! half keeps git2 materialization, autosave, and repository-index access.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use git2::{FileMode, IndexEntry, MergeFileInput, MergeFileOptions, Repository};

use super::GitError;

pub use kagi_domain::resolution::{
    lines_to_text, text_to_lines, ConflictHunk, HunkChoice, HunkModel, LineOrder, LineOrigin,
    LineSelection, Region, ResolutionChoice, ResolvedLine, SelectionSide, TriState,
};

/// The Result draft for a single file: the materialized side texts plus the
/// current resolution lines and an undo / redo history.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileResolution {
    /// Current side text (stage 2), `None` when that side is absent
    /// (modify-delete) or binary.
    current_text: Option<String>,
    /// Incoming side text (stage 3), `None` when absent or binary.
    incoming_text: Option<String>,
    /// Whether this file is a binary / non-text conflict (no line merge).
    binary: bool,
    /// The current Result lines (the editable draft).  `None` until a choice or
    /// manual edit produces a resolution.
    result: Option<Vec<ResolvedLine>>,
    /// Undo stack: prior `result` states (most recent last).
    undo: Vec<Option<Vec<ResolvedLine>>>,
    /// Redo stack: states popped by undo (most recent last).
    redo: Vec<Option<Vec<ResolvedLine>>>,
}

impl FileResolution {
    fn empty() -> Self {
        FileResolution {
            current_text: None,
            incoming_text: None,
            binary: false,
            result: None,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Push the current `result` onto the undo stack and clear redo, before a
    /// mutation.
    fn checkpoint(&mut self) {
        self.undo.push(self.result.clone());
        self.redo.clear();
    }
}

/// The whole resolution buffer for one repository's conflict session.
///
/// Keyed by repository-relative path; serialized per ADR-0057 to
/// `~/.kagi/conflicts/<sha1(repo)>/buffer.json`.
#[derive(Debug, Clone)]
pub struct ResolutionBuffer {
    /// Absolute path to the repository working tree (the autosave key).
    repo_path: PathBuf,
    /// Per-file resolutions, ordered by path for deterministic serialization.
    files: BTreeMap<PathBuf, FileResolution>,
    /// Per-file **hunk-level** editing state for the Conflict Editor (W32).  In
    /// memory only (rebuilt from the materialization on demand): the persisted
    /// artifact is the assembled `result` in [`FileResolution`].  A file appears
    /// here once [`ResolutionBuffer::ensure_hunks`] has decomposed it.
    hunks: BTreeMap<PathBuf, HunkModel>,
}

// ────────────────────────────────────────────────────────────
// Construction / materialization
// ────────────────────────────────────────────────────────────

impl ResolutionBuffer {
    /// Create an empty buffer for the repository at `repo_path`.
    pub fn new(repo_path: &Path) -> Self {
        ResolutionBuffer {
            repo_path: repo_path.to_path_buf(),
            files: BTreeMap::new(),
            hunks: BTreeMap::new(),
        }
    }

    /// Build a buffer from the repository's current index conflicts, materializing
    /// each file's current / incoming side texts via git2's `merge_file_from_index`
    /// with zdiff3 markers (falling back to standard markers, then to no
    /// materialization for binary / single-sided conflicts).
    ///
    /// No draft Result is set yet — the user picks a side or edits later.
    pub fn from_repo(repo: &Repository) -> Result<Self, GitError> {
        let workdir = repo
            .workdir()
            .ok_or_else(|| GitError::Other("repository has no working tree".to_string()))?;
        let mut buffer = ResolutionBuffer::new(workdir);

        let index = repo
            .index()
            .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
        let conflicts = index
            .conflicts()
            .map_err(|e| GitError::Other(format!("index.conflicts() failed: {}", e.message())))?;

        for entry in conflicts {
            let conflict = match entry {
                Ok(c) => c,
                Err(_) => continue,
            };
            let path = match conflict_path(&conflict) {
                Some(p) => p,
                None => continue,
            };

            let mut fr = FileResolution::empty();
            fr.current_text = blob_text(repo, conflict.our.as_ref());
            fr.incoming_text = blob_text(repo, conflict.their.as_ref());
            fr.binary = entry_is_binary(repo, conflict.our.as_ref())
                || entry_is_binary(repo, conflict.their.as_ref())
                || entry_is_binary(repo, conflict.ancestor.as_ref());

            // Seed an initial draft for content conflicts: the zdiff3
            // materialization is informational (markers + base context); the
            // user resolves by choosing a side or editing. We do NOT auto-mark
            // it resolved (the markers would be residue), so `result` stays
            // None until a choice / edit.
            buffer.files.insert(path, fr);
        }

        Ok(buffer)
    }

    /// Return the zdiff3 (or standard fallback) materialized marker text for a
    /// file, for the UI to show inside the 3-way view.  `None` when the file is
    /// not in the buffer or has no usable text merge.
    pub fn materialized_markers(&self, repo: &Repository, path: &Path) -> Option<String> {
        let fr = self.files.get(path)?;
        if fr.binary {
            return None;
        }
        materialize_zdiff3(repo, path)
    }
}

// ────────────────────────────────────────────────────────────
// Choices / manual edits / undo / redo
// ────────────────────────────────────────────────────────────

impl ResolutionBuffer {
    /// Apply a side choice to `path`, replacing its Result draft.  Pushes the
    /// prior state onto the file's undo stack.
    ///
    /// Returns an error if `path` is not a tracked conflict, or if the chosen
    /// side is absent (e.g. choosing `Incoming` on a modify-delete where the
    /// incoming side was deleted).
    pub fn apply_choice(&mut self, path: &Path, choice: ResolutionChoice) -> Result<(), GitError> {
        let fr = self.files.get_mut(path).ok_or_else(|| {
            GitError::Other(format!("not a tracked conflict: {}", path.display()))
        })?;

        let current = fr.current_text.clone();
        let incoming = fr.incoming_text.clone();

        let lines: Vec<ResolvedLine> = match choice {
            ResolutionChoice::Current => side_lines(&current, LineOrigin::Current)
                .ok_or_else(|| missing_side(path, "current"))?,
            ResolutionChoice::Incoming => side_lines(&incoming, LineOrigin::Incoming)
                .ok_or_else(|| missing_side(path, "incoming"))?,
            ResolutionChoice::BothCurrentFirst => {
                let mut v = side_lines(&current, LineOrigin::Current)
                    .ok_or_else(|| missing_side(path, "current"))?;
                v.extend(
                    side_lines(&incoming, LineOrigin::Incoming)
                        .ok_or_else(|| missing_side(path, "incoming"))?,
                );
                v
            }
            ResolutionChoice::BothIncomingFirst => {
                let mut v = side_lines(&incoming, LineOrigin::Incoming)
                    .ok_or_else(|| missing_side(path, "incoming"))?;
                v.extend(
                    side_lines(&current, LineOrigin::Current)
                        .ok_or_else(|| missing_side(path, "current"))?,
                );
                v
            }
        };

        fr.checkpoint();
        fr.result = Some(lines);
        Ok(())
    }

    /// Replace `path`'s Result draft with hand-edited `text`.  Each line is
    /// recorded with [`LineOrigin::Manual`] provenance.  Pushes the prior state
    /// onto the undo stack.
    pub fn set_manual_text(&mut self, path: &Path, text: &str) -> Result<(), GitError> {
        let fr = self.files.get_mut(path).ok_or_else(|| {
            GitError::Other(format!("not a tracked conflict: {}", path.display()))
        })?;
        let lines = text_to_lines(text, LineOrigin::Manual);
        fr.checkpoint();
        fr.result = Some(lines);
        Ok(())
    }

    /// Undo the last choice / edit for `path`.  Returns `true` if a state was
    /// restored, `false` if the undo stack was empty.
    pub fn undo(&mut self, path: &Path) -> bool {
        match self.files.get_mut(path) {
            Some(fr) => match fr.undo.pop() {
                Some(prev) => {
                    fr.redo.push(fr.result.clone());
                    fr.result = prev;
                    true
                }
                None => false,
            },
            None => false,
        }
    }

    /// Redo the last undone choice / edit for `path`.  Returns `true` if a state
    /// was re-applied.
    pub fn redo(&mut self, path: &Path) -> bool {
        match self.files.get_mut(path) {
            Some(fr) => match fr.redo.pop() {
                Some(next) => {
                    fr.undo.push(fr.result.clone());
                    fr.result = next;
                    true
                }
                None => false,
            },
            None => false,
        }
    }
}

// ────────────────────────────────────────────────────────────
// Hunk-level editing (W32-CONFLICT-EDITOR)
// ────────────────────────────────────────────────────────────

impl ResolutionBuffer {
    /// Ensure a [`HunkModel`] exists for `path`, decomposing it from the supplied
    /// `marker_text` (the zdiff3 / standard materialization) on first use.
    ///
    /// Idempotent: if a model already exists (e.g. the user has been editing
    /// hunks), it is preserved so per-hunk choices are not lost.  Returns `false`
    /// if `path` is not a tracked conflict.
    pub fn ensure_hunks(&mut self, path: &Path, marker_text: &str) -> bool {
        if !self.files.contains_key(path) {
            return false;
        }
        self.hunks
            .entry(path.to_path_buf())
            .or_insert_with(|| HunkModel::from_marker_text(marker_text));
        true
    }

    /// The hunk model for `path`, if one has been built via [`Self::ensure_hunks`].
    pub fn hunk_model(&self, path: &Path) -> Option<&HunkModel> {
        self.hunks.get(path)
    }

    /// Apply a per-hunk choice to the `n`-th hunk of `path`, then re-assemble and
    /// commit the file Result (pushing the prior Result onto the undo stack, so
    /// file-level undo still works).  Returns `true` on success.
    ///
    /// The repository is untouched; the caller autosaves (in-memory first).
    pub fn apply_hunk_choice(
        &mut self,
        path: &Path,
        hunk_index: usize,
        choice: HunkChoice,
    ) -> bool {
        let Some(model) = self.hunks.get_mut(path) else {
            return false;
        };
        if !model.set_choice(hunk_index, choice) {
            return false;
        }
        let assembled = model.assemble();
        // Commit the re-assembled Result into the file resolution.
        if let Some(fr) = self.files.get_mut(path) {
            fr.checkpoint();
            fr.result = Some(assembled);
            true
        } else {
            false
        }
    }

    fn commit_hunk_model(&mut self, path: &Path) -> bool {
        let Some(model) = self.hunks.get(path) else {
            return false;
        };
        let assembled = model.assemble();
        if let Some(fr) = self.files.get_mut(path) {
            fr.checkpoint();
            fr.result = Some(assembled);
            true
        } else {
            false
        }
    }

    /// Set every line checkbox for one file side, then re-assemble the Result.
    pub fn set_file_side_selection(
        &mut self,
        path: &Path,
        side: SelectionSide,
        taken: bool,
    ) -> bool {
        let Some(model) = self.hunks.get_mut(path) else {
            return false;
        };
        model.set_file_side(side, taken);
        self.commit_hunk_model(path)
    }

    /// Set every line checkbox for one hunk side, then re-assemble the Result.
    pub fn set_hunk_side_selection(
        &mut self,
        path: &Path,
        hunk_index: usize,
        side: SelectionSide,
        taken: bool,
    ) -> bool {
        let Some(model) = self.hunks.get_mut(path) else {
            return false;
        };
        if !model.set_hunk_side(hunk_index, side, taken) {
            return false;
        }
        self.commit_hunk_model(path)
    }

    /// Set one line checkbox, then re-assemble the Result.
    pub fn set_hunk_line_selection(
        &mut self,
        path: &Path,
        hunk_index: usize,
        side: SelectionSide,
        line_index: usize,
        taken: bool,
    ) -> bool {
        let Some(model) = self.hunks.get_mut(path) else {
            return false;
        };
        if !model.set_hunk_line(hunk_index, side, line_index, taken) {
            return false;
        }
        self.commit_hunk_model(path)
    }

    /// Set the line output order for one hunk, then re-assemble the Result.
    pub fn set_hunk_line_order(
        &mut self,
        path: &Path,
        hunk_index: usize,
        order: LineOrder,
    ) -> bool {
        let Some(model) = self.hunks.get_mut(path) else {
            return false;
        };
        if !model.set_hunk_line_order(hunk_index, order) {
            return false;
        }
        self.commit_hunk_model(path)
    }

    /// Reset the `n`-th hunk of `path` to [`HunkChoice::Unresolved`] and
    /// re-assemble.  Convenience wrapper over [`Self::apply_hunk_choice`].
    pub fn reset_hunk(&mut self, path: &Path, hunk_index: usize) -> bool {
        self.apply_hunk_choice(path, hunk_index, HunkChoice::Unresolved)
    }

    /// Number of conflict hunks in `path`'s model (0 if not built / not tracked).
    pub fn hunk_count(&self, path: &Path) -> usize {
        self.hunks.get(path).map(|m| m.hunk_count()).unwrap_or(0)
    }

    /// Whether every hunk of `path` is resolved (false if the model is absent).
    pub fn hunks_all_resolved(&self, path: &Path) -> bool {
        self.hunks
            .get(path)
            .map(|m| m.all_resolved())
            .unwrap_or(false)
    }
}

// ────────────────────────────────────────────────────────────
// Queries
// ────────────────────────────────────────────────────────────

impl ResolutionBuffer {
    /// Whether `path` has a Result draft (a side was chosen or text edited).
    pub fn has_resolution(&self, path: &Path) -> bool {
        self.files
            .get(path)
            .map(|fr| fr.result.is_some())
            .unwrap_or(false)
    }

    /// The resolved text for `path` (lines joined with `\n`, trailing newline
    /// appended), or `None` if unresolved / unknown.
    pub fn resolved_text(&self, path: &Path) -> Option<String> {
        let fr = self.files.get(path)?;
        let lines = fr.result.as_ref()?;
        Some(lines_to_text(lines))
    }

    /// The per-line provenance of `path`'s Result draft, or `None` if unresolved.
    pub fn provenance(&self, path: &Path) -> Option<Vec<LineOrigin>> {
        let fr = self.files.get(path)?;
        let lines = fr.result.as_ref()?;
        Some(lines.iter().map(|l| l.origin).collect())
    }

    /// The current / incoming materialized side texts of `path` (for the UI's
    /// 3-way view), or `None` when the file is not tracked.
    pub fn sides(&self, path: &Path) -> Option<(Option<String>, Option<String>)> {
        self.files
            .get(path)
            .map(|fr| (fr.current_text.clone(), fr.incoming_text.clone()))
    }

    /// Paths whose **resolved** text still contains a conflict marker, reusing
    /// the checklist's detection (ADR-0043 rule 4 / ADR-0057 residue gate).
    /// Unresolved files are excluded (they have no Result yet).
    pub fn files_with_marker_residue(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for (path, fr) in &self.files {
            if let Some(lines) = &fr.result {
                let text = lines_to_text(lines);
                if super::checklist::text_has_conflict_marker(&text) {
                    out.push(path.clone());
                }
            }
        }
        out
    }

    /// All tracked conflict paths (sorted).
    pub fn tracked_paths(&self) -> Vec<PathBuf> {
        self.files.keys().cloned().collect()
    }
}

// ────────────────────────────────────────────────────────────
// Autosave / load (hand-written JSON, serde-free — like drafts.rs)
// ────────────────────────────────────────────────────────────

impl ResolutionBuffer {
    /// Autosave the buffer under `~/.kagi/conflicts/<sha1(repo)>/buffer.json`
    /// (or `$KAGI_LOG_DIR/conflicts/...` in tests).  Returns the file path.
    ///
    /// Files with no Result draft are still recorded (so their materialized side
    /// texts survive a restart), but the format is compact.
    pub fn autosave(&self) -> Result<PathBuf, GitError> {
        let path = buffer_file_path(&self.repo_path).ok_or_else(|| {
            GitError::Other(
                "conflicts: could not determine autosave dir (no HOME or KAGI_LOG_DIR)".to_string(),
            )
        })?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                GitError::Other(format!(
                    "conflicts: mkdir failed for {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }
        let json = self.to_json();
        std::fs::write(&path, json.as_bytes()).map_err(|e| {
            GitError::Other(format!(
                "conflicts: write failed for {}: {}",
                path.display(),
                e
            ))
        })?;
        Ok(path)
    }

    /// Load a previously autosaved buffer for the repository at `repo_path`,
    /// or `None` when none exists / the file is corrupt (lenient, like drafts).
    pub fn load(repo_path: &Path) -> Option<ResolutionBuffer> {
        let path = buffer_file_path(repo_path)?;
        let content = std::fs::read_to_string(&path).ok()?;
        parse_buffer_json(repo_path, &content)
    }

    /// Delete the autosaved buffer (e.g. after a successful continue).
    pub fn clear(repo_path: &Path) -> Result<(), GitError> {
        let path = match buffer_file_path(repo_path) {
            Some(p) => p,
            None => return Ok(()),
        };
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(GitError::Other(format!(
                "conflicts: delete failed for {}: {}",
                path.display(),
                e
            ))),
        }
    }

    /// Serialize the buffer to a single JSON object string.
    ///
    /// Schema (one line):
    /// `{"repo":"<path>","updated":<u64>,"files":[{...},...]}`
    /// where each file is
    /// `{"path":"<p>","binary":<bool>,"current":<str|null>,"incoming":<str|null>,"result":[{"t":"<line>","o":"c|i|m"},...]|null}`.
    fn to_json(&self) -> String {
        let mut files_json: Vec<String> = Vec::new();
        for (path, fr) in &self.files {
            files_json.push(file_to_json(path, fr));
        }
        format!(
            "{{\"repo\":\"{}\",\"updated\":{},\"files\":[{}]}}",
            escape_json(&self.repo_path.to_string_lossy()),
            now_unix(),
            files_json.join(",")
        )
    }
}

// ────────────────────────────────────────────────────────────
// zdiff3 materialization
// ────────────────────────────────────────────────────────────

/// Materialize the conflict at `path` as a zdiff3-style marker string using
/// git2's `merge_file_from_index`.  Falls back to standard markers if zdiff3
/// fails, and returns `None` for single-sided / unreadable conflicts.
fn materialize_zdiff3(repo: &Repository, path: &Path) -> Option<String> {
    let index = repo.index().ok()?;
    let conflicts = index.conflicts().ok()?;
    for entry in conflicts.flatten() {
        let p = conflict_path(&entry)?;
        if p != path {
            continue;
        }
        // merge_file_from_index needs all three index stages present.
        let (ancestor, our, their) = match (entry.ancestor, entry.our, entry.their) {
            (Some(a), Some(o), Some(t)) => (a, o, t),
            // add/add: the file was added independently on both sides, so there
            // is no common base. merge_file_from_index can't be used (no
            // ancestor stage), but this is still a TEXT conflict — materialize it
            // against an empty base so the editor shows both sides instead of
            // wrongly falling back to the "binary / single-sided" message.
            (None, Some(o), Some(t)) => {
                return merge_addadd(repo, path, &o, &t, true)
                    .or_else(|| merge_addadd(repo, path, &o, &t, false));
            }
            // modify/delete and other single-sided shapes have no text merge.
            _ => return None,
        };
        // Try zdiff3 first, then standard.
        if let Some(text) = merge_with_style(repo, &ancestor, &our, &their, true) {
            return Some(text);
        }
        return merge_with_style(repo, &ancestor, &our, &their, false);
    }
    None
}

/// Run `merge_file_from_index` with either zdiff3 or standard marker style and
/// return the marker text, or `None` on error.
fn merge_with_style(
    repo: &Repository,
    ancestor: &IndexEntry,
    our: &IndexEntry,
    their: &IndexEntry,
    zdiff3: bool,
) -> Option<String> {
    let mut opts = MergeFileOptions::new();
    opts.ancestor_label("Base");
    opts.our_label("Current");
    opts.their_label("Incoming");
    if zdiff3 {
        opts.style_zdiff3(true);
    } else {
        opts.style_standard(true);
    }
    let result = repo
        .merge_file_from_index(ancestor, our, their, Some(&mut opts))
        .ok()?;
    Some(String::from_utf8_lossy(result.content()).into_owned())
}

/// Materialize an **add/add** conflict (no common base): a content merge of
/// `our` vs `their` against an empty ancestor via `git2::merge_file`. Returns
/// `None` if either blob is unreadable or the merge errors. The whole file
/// becomes a conflict region (both sides "added" everything), which is exactly
/// what the 3-way editor needs to show. (Bug: add/add `.h` files were shown as
/// "binary / single-sided" because `merge_file_from_index` requires a base.)
fn merge_addadd(
    repo: &Repository,
    path: &Path,
    our: &IndexEntry,
    their: &IndexEntry,
    zdiff3: bool,
) -> Option<String> {
    let our_blob = repo.find_blob(our.id).ok()?;
    let their_blob = repo.find_blob(their.id).ok()?;
    let path_str = path.to_string_lossy();

    let mut ancestor_in = MergeFileInput::new();
    ancestor_in
        .content(b"")
        .path(path_str.as_ref())
        .mode(Some(FileMode::Blob));
    let mut our_in = MergeFileInput::new();
    our_in
        .content(our_blob.content())
        .path(path_str.as_ref())
        .mode(Some(FileMode::Blob));
    let mut their_in = MergeFileInput::new();
    their_in
        .content(their_blob.content())
        .path(path_str.as_ref())
        .mode(Some(FileMode::Blob));

    let mut opts = MergeFileOptions::new();
    opts.ancestor_label("Base");
    opts.our_label("Current");
    opts.their_label("Incoming");
    if zdiff3 {
        opts.style_zdiff3(true);
    } else {
        opts.style_standard(true);
    }

    let result = git2::merge_file(&ancestor_in, &our_in, &their_in, Some(&mut opts)).ok()?;
    Some(String::from_utf8_lossy(result.content()).into_owned())
}

// ────────────────────────────────────────────────────────────
// Index / blob helpers
// ────────────────────────────────────────────────────────────

/// Extract a conflict's path from whichever stage entry is present.
fn conflict_path(conflict: &git2::IndexConflict) -> Option<PathBuf> {
    let bytes = conflict
        .our
        .as_ref()
        .or(conflict.their.as_ref())
        .or(conflict.ancestor.as_ref())
        .map(|e| e.path.clone())?;
    Some(PathBuf::from(String::from_utf8_lossy(&bytes).into_owned()))
}

/// Read an index entry's blob as decoded text, or `None` for a missing /
/// binary / unreadable blob.
fn blob_text(repo: &Repository, entry: Option<&IndexEntry>) -> Option<String> {
    let entry = entry?;
    if entry.id.is_zero() {
        return None;
    }
    let blob = repo.find_blob(entry.id).ok()?;
    if blob.is_binary() {
        return None;
    }
    Some(String::from_utf8_lossy(blob.content()).into_owned())
}

/// Whether an index entry's blob is binary.
fn entry_is_binary(repo: &Repository, entry: Option<&IndexEntry>) -> bool {
    let entry = match entry {
        Some(e) => e,
        None => return false,
    };
    if entry.id.is_zero() {
        return false;
    }
    match repo.find_blob(entry.id) {
        Ok(blob) => {
            let content = blob.content();
            let probe = &content[..content.len().min(8 * 1024)];
            blob.is_binary() || probe.contains(&0u8)
        }
        Err(_) => false,
    }
}

// ────────────────────────────────────────────────────────────
// Line helpers (chars()-safe; split on '\n' over &str)
// ────────────────────────────────────────────────────────────

/// Convert an optional side text into resolution lines with the given origin,
/// or `None` when the side is absent.
fn side_lines(text: &Option<String>, origin: LineOrigin) -> Option<Vec<ResolvedLine>> {
    text.as_ref().map(|t| text_to_lines(t, origin))
}

/// Error for a missing side on a choice.
fn missing_side(path: &Path, side: &str) -> GitError {
    GitError::Other(format!(
        "cannot take {} side of {}: that side does not exist (modify/delete or binary)",
        side,
        path.display()
    ))
}

// ────────────────────────────────────────────────────────────
// Path resolution (mirrors drafts.rs)
// ────────────────────────────────────────────────────────────

/// Resolve the conflicts autosave directory:
/// 1. `$KAGI_LOG_DIR/conflicts/` when set and non-empty.
/// 2. `$HOME/.kagi/conflicts/` otherwise.
fn conflicts_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("KAGI_LOG_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("conflicts"));
        }
    }
    dirs_home().map(|home| home.join(".kagi").join("conflicts"))
}

/// `$HOME` then `$USERPROFILE`.
fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// `<conflicts_dir>/<sha1(repo_path)>/buffer.json`.
///
/// The key is normalized so that two spellings of the same repository hash
/// identically: it is canonicalized when the path exists (resolving symlinks
/// such as macOS's `/var` → `/private/var` and any `repo.workdir()` trailing
/// slash) and otherwise falls back to the trailing-separator-stripped string.
fn buffer_file_path(repo_path: &Path) -> Option<PathBuf> {
    let canonical = std::fs::canonicalize(repo_path).ok();
    let key_owned = match &canonical {
        Some(p) => p.to_string_lossy().into_owned(),
        None => repo_path
            .to_string_lossy()
            .trim_end_matches(['/', '\\'])
            .to_string(),
    };
    let hash = sha1_hex(key_owned.as_bytes());
    conflicts_dir().map(|dir| dir.join(hash).join("buffer.json"))
}

/// Current wall-clock time in Unix epoch seconds (0 on clock error).
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ────────────────────────────────────────────────────────────
// JSON encode / decode (serde-free; same escaping as drafts.rs)
// ────────────────────────────────────────────────────────────

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn unescape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let hex: String = (0..4).filter_map(|_| chars.next()).collect();
                if let Ok(code) = u32::from_str_radix(&hex, 16) {
                    if let Some(c) = char::from_u32(code) {
                        out.push(c);
                    }
                }
            }
            Some(c) => {
                out.push('\\');
                out.push(c);
            }
            None => {}
        }
    }
    out
}

/// Serialize one file resolution to its JSON object.
fn file_to_json(path: &Path, fr: &FileResolution) -> String {
    let current = opt_str_json(&fr.current_text);
    let incoming = opt_str_json(&fr.incoming_text);
    let result = match &fr.result {
        None => "null".to_string(),
        Some(lines) => {
            let items: Vec<String> = lines
                .iter()
                .map(|l| {
                    format!(
                        "{{\"t\":\"{}\",\"o\":\"{}\"}}",
                        escape_json(&l.text),
                        l.origin.tag()
                    )
                })
                .collect();
            format!("[{}]", items.join(","))
        }
    };
    format!(
        "{{\"path\":\"{}\",\"binary\":{},\"current\":{},\"incoming\":{},\"result\":{}}}",
        escape_json(&path.to_string_lossy()),
        fr.binary,
        current,
        incoming,
        result
    )
}

/// `null` or a quoted+escaped string.
fn opt_str_json(s: &Option<String>) -> String {
    match s {
        None => "null".to_string(),
        Some(v) => format!("\"{}\"", escape_json(v)),
    }
}

/// Parse a buffer JSON object produced by [`ResolutionBuffer::to_json`].
///
/// Lenient: missing / unparseable fields default sensibly; a non-object input
/// yields `None`.  The history (undo/redo) is intentionally not persisted —
/// only the current Result and side texts round-trip (ADR-0057: resume the
/// resolution, not the keystroke history).
fn parse_buffer_json(repo_path: &Path, content: &str) -> Option<ResolutionBuffer> {
    let content = content.trim();
    if !content.starts_with('{') {
        return None;
    }
    let mut buffer = ResolutionBuffer::new(repo_path);

    // Find the `"files":[ ... ]` array and split it into top-level objects.
    let files_start = content.find("\"files\":[")? + "\"files\":[".len();
    let array = &content[files_start..];
    for obj in split_top_level_objects(array) {
        if let Some((path, fr)) = parse_file_object(&obj) {
            buffer.files.insert(path, fr);
        }
    }
    Some(buffer)
}

/// Parse a single file object fragment (without surrounding braces handling
/// beyond what `extract_*` needs).
fn parse_file_object(obj: &str) -> Option<(PathBuf, FileResolution)> {
    let path_str = extract_string(obj, "path")?;
    let path = PathBuf::from(path_str);

    let binary = obj.contains("\"binary\":true");
    let current = extract_nullable_string(obj, "current");
    let incoming = extract_nullable_string(obj, "incoming");
    let result = parse_result_array(obj);

    let fr = FileResolution {
        current_text: current,
        incoming_text: incoming,
        binary,
        result,
        undo: Vec::new(),
        redo: Vec::new(),
    };
    Some((path, fr))
}

/// Parse the `"result":[...]` array of `{"t":..,"o":..}` items, or `None` when
/// it is `null`.
fn parse_result_array(obj: &str) -> Option<Vec<ResolvedLine>> {
    let key = "\"result\":";
    let pos = obj.find(key)?;
    let after = obj[pos + key.len()..].trim_start();
    if after.starts_with("null") {
        return None;
    }
    if !after.starts_with('[') {
        return None;
    }
    // Slice from '[' to the matching ']'.
    let mut depth = 0usize;
    let mut end = None;
    let mut in_str = false;
    let mut escaped = false;
    for (i, ch) in after.char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let arr = &after[..=end?];
    let mut lines = Vec::new();
    // `split_top_level_objects` scans to the first `{`, so the leading `[` is
    // tolerated — no byte slicing of the string is needed here.
    for item in split_top_level_objects(arr) {
        let t = extract_string(&item, "t").unwrap_or_default();
        let o = extract_string(&item, "o").unwrap_or_else(|| "m".to_string());
        let origin = LineOrigin::from_tag(o.chars().next().unwrap_or('m'));
        lines.push(ResolvedLine { text: t, origin });
    }
    Some(lines)
}

/// Split a JSON array body (the text after the opening `[`) into its top-level
/// `{...}` object fragments, respecting nested strings / braces.
fn split_top_level_objects(s: &str) -> Vec<String> {
    let mut objects = Vec::new();
    let mut depth = 0usize;
    let mut start = None;
    let mut in_str = false;
    let mut escaped = false;
    for (i, ch) in s.char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(st) = start {
                            objects.push(s[st..=i].to_string());
                        }
                        start = None;
                    }
                }
            }
            // Stop at the array's closing bracket when not nested.
            ']' if depth == 0 => break,
            _ => {}
        }
    }
    objects
}

/// Extract the **unescaped** string value for `"key":"…"`, or `None`.
fn extract_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":\"", key);
    let pos = json.find(needle.as_str())?;
    let after = &json[pos + needle.len()..];
    let mut escaped = false;
    let mut end = None;
    for (i, ch) in after.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            end = Some(i);
            break;
        }
    }
    end.map(|e| unescape_json(&after[..e]))
}

/// Extract a nullable string: returns `Some(string)` for `"key":"v"`, `None`
/// for `"key":null` or absent.
fn extract_nullable_string(json: &str, key: &str) -> Option<String> {
    let null_needle = format!("\"{}\":null", key);
    if json.contains(&null_needle) {
        return None;
    }
    extract_string(json, key)
}

// ────────────────────────────────────────────────────────────
// Self-contained SHA-1 (filename key only; matches drafts.rs)
// ────────────────────────────────────────────────────────────

/// SHA-1 of `data` as 40-char lowercase hex (RFC 3174). Used only as a stable
/// filename key for the autosave directory (no security properties relied on).
fn sha1_hex(data: &[u8]) -> String {
    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xEFCD_AB89;
    let mut h2: u32 = 0x98BA_DCFE;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xC3D2_E1F0;

    let ml: u64 = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&ml.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let j = i * 4;
            *word = u32::from_be_bytes([chunk[j], chunk[j + 1], chunk[j + 2], chunk[j + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999_u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = String::with_capacity(40);
    for half in [h0, h1, h2, h3, h4] {
        out.push_str(&format!("{:08x}", half));
    }
    out
}

// ────────────────────────────────────────────────────────────
// Unit tests (pure logic; repo-backed behaviour in tests/conflicts_test.rs)
// ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn buf_with_sides(current: &str, incoming: &str) -> (ResolutionBuffer, PathBuf) {
        let mut b = ResolutionBuffer::new(Path::new("/tmp/repo"));
        let path = PathBuf::from("a.txt");
        let fr = FileResolution {
            current_text: Some(current.to_string()),
            incoming_text: Some(incoming.to_string()),
            binary: false,
            result: None,
            undo: Vec::new(),
            redo: Vec::new(),
        };
        b.files.insert(path.clone(), fr);
        (b, path)
    }

    #[test]
    fn choice_current_and_incoming() {
        let (mut b, p) = buf_with_sides("current line\n", "incoming line\n");
        b.apply_choice(&p, ResolutionChoice::Current).unwrap();
        assert_eq!(b.resolved_text(&p).unwrap(), "current line\n");
        assert_eq!(b.provenance(&p).unwrap(), vec![LineOrigin::Current]);

        b.apply_choice(&p, ResolutionChoice::Incoming).unwrap();
        assert_eq!(b.resolved_text(&p).unwrap(), "incoming line\n");
        assert_eq!(b.provenance(&p).unwrap(), vec![LineOrigin::Incoming]);
    }

    #[test]
    fn choice_both_orders() {
        let (mut b, p) = buf_with_sides("A\n", "B\n");
        b.apply_choice(&p, ResolutionChoice::BothCurrentFirst)
            .unwrap();
        assert_eq!(b.resolved_text(&p).unwrap(), "A\nB\n");
        assert_eq!(
            b.provenance(&p).unwrap(),
            vec![LineOrigin::Current, LineOrigin::Incoming]
        );

        b.apply_choice(&p, ResolutionChoice::BothIncomingFirst)
            .unwrap();
        assert_eq!(b.resolved_text(&p).unwrap(), "B\nA\n");
        assert_eq!(
            b.provenance(&p).unwrap(),
            vec![LineOrigin::Incoming, LineOrigin::Current]
        );
    }

    #[test]
    fn manual_text_and_provenance() {
        let (mut b, p) = buf_with_sides("A\n", "B\n");
        b.set_manual_text(&p, "hand\nedited\n").unwrap();
        assert_eq!(b.resolved_text(&p).unwrap(), "hand\nedited\n");
        assert_eq!(
            b.provenance(&p).unwrap(),
            vec![LineOrigin::Manual, LineOrigin::Manual]
        );
    }

    #[test]
    fn undo_redo_round_trip() {
        let (mut b, p) = buf_with_sides("A\n", "B\n");
        assert!(!b.has_resolution(&p));
        b.apply_choice(&p, ResolutionChoice::Current).unwrap();
        b.apply_choice(&p, ResolutionChoice::Incoming).unwrap();
        assert_eq!(b.resolved_text(&p).unwrap(), "B\n");

        assert!(b.undo(&p));
        assert_eq!(b.resolved_text(&p).unwrap(), "A\n");
        assert!(b.undo(&p));
        assert!(!b.has_resolution(&p)); // back to no resolution
        assert!(!b.undo(&p)); // stack empty

        assert!(b.redo(&p));
        assert_eq!(b.resolved_text(&p).unwrap(), "A\n");
        assert!(b.redo(&p));
        assert_eq!(b.resolved_text(&p).unwrap(), "B\n");
        assert!(!b.redo(&p));
    }

    #[test]
    fn marker_residue_detected_in_manual_edit() {
        let (mut b, p) = buf_with_sides("A\n", "B\n");
        b.set_manual_text(&p, "<<<<<<< HEAD\nA\n=======\nB\n>>>>>>> other\n")
            .unwrap();
        let residue = b.files_with_marker_residue();
        assert_eq!(residue, vec![p.clone()]);

        // Clean resolution has no residue.
        b.set_manual_text(&p, "A and B\n").unwrap();
        assert!(b.files_with_marker_residue().is_empty());
    }

    #[test]
    fn missing_side_choice_errors() {
        let mut b = ResolutionBuffer::new(Path::new("/tmp/repo"));
        let p = PathBuf::from("md.txt");
        let fr = FileResolution {
            current_text: Some("only current\n".to_string()),
            incoming_text: None, // modify/delete: incoming deleted
            binary: false,
            result: None,
            undo: Vec::new(),
            redo: Vec::new(),
        };
        b.files.insert(p.clone(), fr);
        assert!(b.apply_choice(&p, ResolutionChoice::Current).is_ok());
        assert!(b.apply_choice(&p, ResolutionChoice::Incoming).is_err());
    }

    #[test]
    fn json_round_trips_result_and_provenance() {
        let (mut b, p) = buf_with_sides("A\nC\n", "B\n");
        b.apply_choice(&p, ResolutionChoice::BothCurrentFirst)
            .unwrap();
        let json = b.to_json();
        let parsed = parse_buffer_json(Path::new("/tmp/repo"), &json).expect("parse");
        assert_eq!(parsed.resolved_text(&p).unwrap(), "A\nC\nB\n");
        assert_eq!(
            parsed.provenance(&p).unwrap(),
            vec![
                LineOrigin::Current,
                LineOrigin::Current,
                LineOrigin::Incoming
            ]
        );
        // Side texts survive.
        let (cur, inc) = parsed.sides(&p).unwrap();
        assert_eq!(cur.as_deref(), Some("A\nC\n"));
        assert_eq!(inc.as_deref(), Some("B\n"));
    }

    #[test]
    fn json_round_trips_special_chars_and_null_side() {
        let mut b = ResolutionBuffer::new(Path::new("/tmp/re\"po"));
        let p = PathBuf::from("x.txt");
        let fr = FileResolution {
            current_text: Some("quote \"x\"\ttab\n".to_string()),
            incoming_text: None,
            binary: false,
            result: None,
            undo: Vec::new(),
            redo: Vec::new(),
        };
        b.files.insert(p.clone(), fr);
        let json = b.to_json();
        let parsed = parse_buffer_json(Path::new("/tmp/re\"po"), &json).expect("parse");
        let (cur, inc) = parsed.sides(&p).unwrap();
        assert_eq!(cur.as_deref(), Some("quote \"x\"\ttab\n"));
        assert_eq!(inc, None);
        assert!(!parsed.has_resolution(&p));
    }

    #[test]
    fn text_to_lines_trailing_newline_stable() {
        let lines = text_to_lines("a\nb\n", LineOrigin::Manual);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines_to_text(&lines), "a\nb\n");

        let no_trailing = text_to_lines("a\nb", LineOrigin::Manual);
        assert_eq!(no_trailing.len(), 2);
        assert_eq!(lines_to_text(&no_trailing), "a\nb\n");
    }

    #[test]
    fn sha1_known_answer() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn parse_rejects_non_object() {
        assert!(parse_buffer_json(Path::new("/tmp/repo"), "nope").is_none());
    }

    // ── Hunk-level model (W32-CONFLICT-EDITOR) ──────────────────────

    /// A two-hunk zdiff3 materialization with passthrough context around and
    /// between the conflicts.
    const TWO_HUNK_ZDIFF3: &str = "\
keep top
<<<<<<< Current
a-current
||||||| Base
a-base
=======
a-incoming
>>>>>>> Incoming
keep middle
<<<<<<< Current
b-current
=======
b-incoming
>>>>>>> Incoming
keep bottom
";

    #[test]
    fn hunk_split_multiple_regions_in_one_file() {
        let m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        // passthrough, hunk, passthrough, hunk, passthrough = 5 regions.
        assert_eq!(m.regions.len(), 5);
        assert_eq!(m.hunk_count(), 2);
        assert!(matches!(m.regions[0], Region::Passthrough(_)));
        assert!(matches!(m.regions[1], Region::Hunk(_)));
        assert!(matches!(m.regions[2], Region::Passthrough(_)));
        assert!(matches!(m.regions[3], Region::Hunk(_)));
        assert!(matches!(m.regions[4], Region::Passthrough(_)));

        let hunks = m.hunks();
        assert_eq!(hunks[0].current, vec!["a-current".to_string()]);
        assert_eq!(hunks[0].base, vec!["a-base".to_string()]);
        assert_eq!(hunks[0].incoming, vec!["a-incoming".to_string()]);
        // Second hunk had no base group (standard-style block).
        assert!(hunks[1].base.is_empty());
        assert_eq!(hunks[1].current, vec!["b-current".to_string()]);
    }

    #[test]
    fn hunk_accept_variants_assemble_with_provenance() {
        // Accept current on hunk 0, accept both (incoming first) on hunk 1.
        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        assert!(m.set_choice(0, HunkChoice::AcceptCurrent));
        assert!(m.set_choice(1, HunkChoice::BothIncomingFirst));
        assert!(m.all_resolved());

        let lines = m.assemble();
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "keep top",
                "a-current",
                "keep middle",
                "b-incoming",
                "b-current",
                "keep bottom",
            ]
        );
        let origins: Vec<LineOrigin> = lines.iter().map(|l| l.origin).collect();
        assert_eq!(
            origins,
            vec![
                LineOrigin::Context,  // keep top
                LineOrigin::Current,  // a-current
                LineOrigin::Context,  // keep middle
                LineOrigin::Incoming, // b-incoming (incoming first)
                LineOrigin::Current,  // b-current
                LineOrigin::Context,  // keep bottom
            ]
        );
    }

    #[test]
    fn hunk_accept_incoming_and_both_current_first() {
        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        m.set_choice(0, HunkChoice::AcceptIncoming);
        m.set_choice(1, HunkChoice::BothCurrentFirst);
        let texts: Vec<String> = m.assemble().into_iter().map(|l| l.text).collect();
        assert_eq!(
            texts,
            vec![
                "keep top".to_string(),
                "a-incoming".to_string(),
                "keep middle".to_string(),
                "b-current".to_string(),
                "b-incoming".to_string(),
                "keep bottom".to_string(),
            ]
        );
    }

    #[test]
    fn line_selection_includes_and_excludes_individual_lines() {
        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        assert!(m.set_hunk_line(0, SelectionSide::Current, 0, true));
        assert!(m.set_hunk_line(0, SelectionSide::Incoming, 0, false));
        assert!(m.set_hunk_side(1, SelectionSide::Incoming, true));

        let texts: Vec<String> = m.assemble().into_iter().map(|l| l.text).collect();
        assert_eq!(
            texts,
            vec![
                "keep top".to_string(),
                "a-current".to_string(),
                "keep middle".to_string(),
                "b-incoming".to_string(),
                "keep bottom".to_string(),
            ]
        );
        assert!(m.all_resolved());
    }

    #[test]
    fn line_selection_order_is_explicit() {
        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        assert!(m.set_hunk_side(0, SelectionSide::Current, true));
        assert!(m.set_hunk_side(0, SelectionSide::Incoming, true));
        assert!(m.set_hunk_line_order(0, LineOrder::IncomingFirst));

        let texts: Vec<String> = m.assemble().into_iter().map(|l| l.text).collect();
        assert_eq!(texts[1], "a-incoming");
        assert_eq!(texts[2], "a-current");
    }

    #[test]
    fn assemble_falls_back_to_hunk_choice_without_line_selection() {
        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        assert!(m.set_choice(0, HunkChoice::AcceptIncoming));
        let hunk = m.hunks()[0];
        assert!(hunk.line_select.is_none());
        let texts: Vec<String> = m.assemble().into_iter().map(|l| l.text).collect();
        assert_eq!(texts[1], "a-incoming");
    }

    #[test]
    fn tri_state_helpers_and_parent_child_propagation() {
        assert_eq!(TriState::from_bools(&[false, false]), TriState::None);
        assert_eq!(TriState::from_bools(&[true, true]), TriState::All);
        assert_eq!(TriState::from_bools(&[true, false]), TriState::Partial);
        assert_eq!(
            TriState::from_children(&[TriState::All, TriState::None]),
            TriState::Partial
        );

        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        assert_eq!(m.file_side_state(SelectionSide::Current), TriState::None);
        m.set_file_side(SelectionSide::Current, true);
        assert_eq!(m.file_side_state(SelectionSide::Current), TriState::All);
        assert_eq!(
            m.hunk_side_state(0, SelectionSide::Current),
            Some(TriState::All)
        );
        assert!(m.set_hunk_line(0, SelectionSide::Current, 0, false));
        assert_eq!(m.file_side_state(SelectionSide::Current), TriState::Partial);
        assert_eq!(
            m.hunk_side_state(0, SelectionSide::Current),
            Some(TriState::None)
        );
    }

    #[test]
    fn hunk_manual_edit_provenance() {
        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        m.set_choice(0, HunkChoice::Manual("merged a\n".to_string()));
        m.set_choice(1, HunkChoice::AcceptCurrent);
        let lines = m.assemble();
        // Find the manual line.
        let manual = lines.iter().find(|l| l.text == "merged a").unwrap();
        assert_eq!(manual.origin, LineOrigin::Manual);
    }

    #[test]
    fn hunk_reset_re_emits_markers_and_keeps_gate_closed() {
        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        m.set_choice(0, HunkChoice::AcceptCurrent);
        m.set_choice(1, HunkChoice::AcceptIncoming);
        assert!(m.all_resolved());

        // Reset hunk 1 → unresolved again.
        assert!(m.set_choice(1, HunkChoice::Unresolved));
        assert!(!m.all_resolved());
        assert_eq!(m.resolved_hunk_count(), 1);

        // The assembled text still carries the markers of the unresolved hunk.
        let text = m.assembled_text();
        assert!(super::super::checklist::text_has_conflict_marker(&text));
    }

    #[test]
    fn all_resolved_file_assembled_result_is_marker_free() {
        let mut m = HunkModel::from_marker_text(TWO_HUNK_ZDIFF3);
        m.set_choice(0, HunkChoice::AcceptCurrent);
        m.set_choice(1, HunkChoice::AcceptIncoming);
        let text = m.assembled_text();
        assert!(
            !super::super::checklist::text_has_conflict_marker(&text),
            "fully resolved Result must contain no conflict markers: {:?}",
            text
        );
    }

    #[test]
    fn buffer_apply_hunk_choice_updates_result_and_undo() {
        let (mut b, p) = buf_with_sides("x\n", "y\n");
        assert!(b.ensure_hunks(&p, TWO_HUNK_ZDIFF3));
        assert_eq!(b.hunk_count(&p), 2);

        assert!(b.apply_hunk_choice(&p, 0, HunkChoice::AcceptCurrent));
        assert!(b.apply_hunk_choice(&p, 1, HunkChoice::AcceptIncoming));
        assert!(b.hunks_all_resolved(&p));

        let text = b.resolved_text(&p).unwrap();
        assert!(text.contains("a-current"));
        assert!(text.contains("b-incoming"));
        assert!(text.contains("keep top"));
        assert!(!super::super::checklist::text_has_conflict_marker(&text));

        // File-level undo unwinds the last hunk commit.
        assert!(b.undo(&p));
        let after_undo = b.resolved_text(&p).unwrap();
        // After undoing hunk 1's commit, only hunk 0 was applied → hunk 1 still
        // contributes its markers, so residue is present again.
        assert!(super::super::checklist::text_has_conflict_marker(
            &after_undo
        ));
    }

    #[test]
    fn ensure_hunks_is_idempotent() {
        let (mut b, p) = buf_with_sides("x\n", "y\n");
        b.ensure_hunks(&p, TWO_HUNK_ZDIFF3);
        b.apply_hunk_choice(&p, 0, HunkChoice::AcceptCurrent);
        // A second ensure with different text must NOT clobber existing choices.
        b.ensure_hunks(&p, "<<<<<<< Current\nz\n=======\nw\n>>>>>>> Incoming\n");
        assert_eq!(b.hunk_count(&p), 2);
        assert_eq!(b.hunk_model(&p).unwrap().resolved_hunk_count(), 1);
    }
}
