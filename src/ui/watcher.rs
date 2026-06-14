//! T029: working-tree watcher for external-change auto-refresh.
//!
//! Spawns a `notify::RecommendedWatcher` on the repository **working tree** and
//! classifies each event ([`classify`]): git state changes under `.git`
//! (HEAD/refs/index → [`WatchEvent::Git`]) drive a full reload, while file edits
//! outside `.git` ([`WatchEvent::WorkTree`]) drive a cheap working-tree status
//! refresh so the WIP updates when files change on disk, not only on git ops.
//! A 500 ms debounce prevents event storms from multi-step operations.
//!
//! The gpui bridge works as follows:
//!   1. `notify` delivers events on a `std::sync::mpsc::Sender<()>`.
//!   2. `start_git_watcher` returns the `Receiver<()>` and the `Watcher` handle
//!      (kept alive as long as the caller holds it).
//!   3. In `run_app`, after the `Entity<KagiApp>` is created, we call
//!      `cx.spawn` on the entity context, passing a loop that:
//!         a. parks on `background_executor().timer(500ms)` to debounce,
//!         b. drains the channel (to prevent re-firing for already-consumed events),
//!         c. upgrades the `WeakEntity<KagiApp>` and calls `reload()` + sets the
//!            refreshed footer message.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// File-name components that indicate a relevant git state change.
/// Events whose path does not match any of these are discarded.
///
/// We match against path *components* (not the full path) so that nested
/// paths such as `.git/refs/heads/main` are caught by the `refs` entry.
const RELEVANT_NAMES: &[&str] = &[
    "HEAD",
    "refs",
    "packed-refs",
    "index",
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "ORIG_HEAD",
    "REBASE_HEAD",
];

/// Component names that mark a subtree to *skip* entirely:
/// - `objects/` — pack/loose objects, extremely frequent and irrelevant.
/// - `worktrees/` — the gitdirs of **other** linked worktrees. Each contains its
///   own `HEAD`/`index`/`refs`/`logs`, so without this an active sibling worktree
///   (e.g. a Claude Code worktree under `.claude/worktrees/…`) would fire a reload
///   of *this* view on every git op it does — a storm of full UI-thread reloads.
/// - `modules/` — submodule gitdirs, same problem (`.git/modules/<name>/HEAD…`).
/// The current repo's own HEAD/index/refs live at the top of `.git`, never under
/// these subtrees, so skipping them loses no real reactivity for this view.
const SKIP_COMPONENTS: &[&str] = &["objects", "worktrees", "modules"];

/// What kind of change a filesystem event represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchEvent {
    /// A git state change under `.git` (HEAD/refs/index/...): commit, checkout,
    /// fetch, stage, etc. → the commit graph may have changed → full reload.
    Git,
    /// A working-tree file change (outside `.git`): edit/add/delete of a file.
    /// → only the WIP / working-tree status may have changed → light refresh.
    WorkTree,
}

/// Classify a filesystem event. Returns the heaviest relevant kind, or `None`
/// if the event is irrelevant (access-only, or an ignored `.git` subtree like
/// `objects/`/`worktrees/`). A `.git` HEAD/refs/index change wins (`Git`),
/// otherwise any non-`.git` path is a working-tree change (`WorkTree`).
fn classify(event: &Event) -> Option<WatchEvent> {
    match &event.kind {
        EventKind::Access(_) | EventKind::Other => return None,
        _ => {}
    }
    let mut worktree = false;
    for p in &event.paths {
        if has_git_component(p) {
            // Inside `.git`: relevant only for the tracked state names.
            if path_is_relevant(p) {
                return Some(WatchEvent::Git);
            }
        } else {
            // Outside `.git`: a working-tree change.
            worktree = true;
        }
    }
    worktree.then_some(WatchEvent::WorkTree)
}

/// Whether `p` has a `.git` path component (i.e. lives under the git dir). Note
/// `.gitignore`/`.gitattributes` are filenames, not a `.git` component.
fn has_git_component(p: &Path) -> bool {
    p.components().any(|c| c.as_os_str() == ".git")
}

fn path_is_relevant(p: &Path) -> bool {
    for component in p.components() {
        let s = component.as_os_str().to_string_lossy();
        // A skipped subtree short-circuits before any RELEVANT_NAMES match deeper
        // in the path (e.g. `.git/worktrees/x/HEAD` is skipped, not matched on HEAD).
        if SKIP_COMPONENTS.contains(&s.as_ref()) {
            return false;
        }
        if RELEVANT_NAMES.contains(&s.as_ref()) {
            return true;
        }
    }
    false
}

/// Start watching the repository **working tree** (recursively) for changes.
///
/// This catches both git state changes (under `.git` → [`WatchEvent::Git`]) and
/// working-tree file edits (outside `.git` → [`WatchEvent::WorkTree`]), so the WIP
/// can refresh when files change on disk, not only on git operations. Events are
/// classified by [`classify`]; the caller debounces and routes by kind (a
/// `Git` event does a full reload; a `WorkTree` event does a cheap status check).
///
/// Returns `(receiver, watcher)`. The watcher **must** be kept alive by the
/// caller; dropping it stops the watch. Returns `None` if the working tree does
/// not exist or the watcher could not be created (e.g. inotify limit exceeded) —
/// treat as a no-op rather than fatal.
pub fn start_git_watcher(
    repo_root: &PathBuf,
) -> Option<(mpsc::Receiver<WatchEvent>, RecommendedWatcher)> {
    if !repo_root.exists() {
        eprintln!(
            "[kagi] watcher: workdir not found at {}",
            repo_root.display()
        );
        return None;
    }

    let (tx, rx) = mpsc::channel::<WatchEvent>();

    let watcher_result = notify::recommended_watcher(move |res: notify::Result<Event>| match res {
        Ok(event) => {
            if let Some(kind) = classify(&event) {
                // Ignore send errors — the receiver may already be gone.
                let _ = tx.send(kind);
            }
        }
        Err(e) => {
            eprintln!("[kagi] watcher: notify error: {}", e);
        }
    });

    let mut watcher = match watcher_result {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[kagi] watcher: failed to create watcher: {}", e);
            return None;
        }
    };

    if let Err(e) = watcher.watch(repo_root, RecursiveMode::Recursive) {
        eprintln!(
            "[kagi] watcher: failed to watch {}: {}",
            repo_root.display(),
            e
        );
        return None;
    }

    eprintln!(
        "[kagi] watcher: watching {} (working tree)",
        repo_root.display()
    );
    Some((rx, watcher))
}

/// Minimum idle time between consecutive reloads (debounce window).
pub const DEBOUNCE: Duration = Duration::from_millis(500);

#[cfg(test)]
mod tests {
    use super::path_is_relevant;
    use std::path::Path;

    #[test]
    fn main_repo_state_is_relevant() {
        for p in [
            ".git/HEAD",
            ".git/index",
            ".git/refs/heads/main",
            ".git/packed-refs",
            ".git/ORIG_HEAD",
            ".git/MERGE_HEAD",
        ] {
            assert!(path_is_relevant(Path::new(p)), "{p} should be relevant");
        }
    }

    #[test]
    fn objects_are_skipped() {
        assert!(!path_is_relevant(Path::new(".git/objects/ab/cdef")));
        assert!(!path_is_relevant(Path::new(".git/objects/pack/pack-1.idx")));
    }

    #[test]
    fn sibling_worktree_and_submodule_gitdirs_are_skipped() {
        // Regression: an active sibling worktree / submodule writing its own
        // HEAD/index/refs must NOT fire a reload of this view (component match on
        // HEAD/index/refs deeper in the path is short-circuited by the skip).
        for p in [
            ".git/worktrees/charming-archimedes-98a4d8/HEAD",
            ".git/worktrees/charming-archimedes-98a4d8/index",
            ".git/worktrees/some-wt/refs/bisect/bad",
            ".git/worktrees/some-wt/logs/HEAD",
            ".git/modules/vendor/HEAD",
            ".git/modules/vendor/index",
        ] {
            assert!(!path_is_relevant(Path::new(p)), "{p} should be skipped");
        }
    }

    #[test]
    fn git_component_detection() {
        use super::has_git_component;
        assert!(has_git_component(Path::new("repo/.git/HEAD")));
        assert!(has_git_component(Path::new(".git/refs/heads/main")));
        // working-tree files (incl. .gitignore as a filename) are NOT git-internal
        assert!(!has_git_component(Path::new("repo/src/main.rs")));
        assert!(!has_git_component(Path::new("repo/.gitignore")));
        assert!(!has_git_component(Path::new(
            "embedded/.claude/worktrees/x/foo.py"
        )));
    }
}
