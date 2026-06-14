//! T029: .git directory watcher for external-change auto-refresh.
//!
//! Spawns a `notify::RecommendedWatcher` on the `.git` directory of the current
//! repository and filters events to the subset that represents a meaningful git
//! state change (HEAD, refs, index, …).  A 500 ms debounce prevents event
//! storms that occur when git performs multi-step operations.
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

/// Returns `true` if the event path is relevant to the display state.
fn is_relevant_event(event: &Event) -> bool {
    // Only care about non-access events (create/modify/remove/rename).
    match &event.kind {
        EventKind::Access(_) => return false,
        EventKind::Other => return false,
        _ => {}
    }

    // At least one path in the event must match.
    event.paths.iter().any(|p| path_is_relevant(p))
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

/// Start watching `<repo_root>/.git` for relevant git state changes.
///
/// Returns `(receiver, watcher)`.  The watcher **must** be kept alive by the
/// caller; dropping it stops the watch.  The receiver delivers a `()` signal
/// whenever a relevant event is observed (multiple events are coalesced: only
/// the fact that *something* changed is signalled, not details).
///
/// Returns `None` if the `.git` directory does not exist or the watcher could
/// not be created (e.g. inotify limit exceeded).  The caller should treat this
/// as a no-op rather than a fatal error.
pub fn start_git_watcher(repo_root: &PathBuf) -> Option<(mpsc::Receiver<()>, RecommendedWatcher)> {
    let git_dir = repo_root.join(".git");
    if !git_dir.exists() {
        eprintln!(
            "[kagi] watcher: .git dir not found at {}",
            git_dir.display()
        );
        return None;
    }

    let (tx, rx) = mpsc::channel::<()>();

    let watcher_result = notify::recommended_watcher(move |res: notify::Result<Event>| {
        match res {
            Ok(event) => {
                if is_relevant_event(&event) {
                    // Ignore send errors — the receiver may already be gone.
                    let _ = tx.send(());
                }
            }
            Err(e) => {
                eprintln!("[kagi] watcher: notify error: {}", e);
            }
        }
    });

    let mut watcher = match watcher_result {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[kagi] watcher: failed to create watcher: {}", e);
            return None;
        }
    };

    if let Err(e) = watcher.watch(&git_dir, RecursiveMode::Recursive) {
        eprintln!(
            "[kagi] watcher: failed to watch {}: {}",
            git_dir.display(),
            e
        );
        return None;
    }

    eprintln!("[kagi] watcher: watching {}", git_dir.display());
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
}
