//! What the terminal shells Kagi started mean for removing a worktree (#867).
//!
//! Kagi never ends a user's process. A shell that is still running in the
//! worktree blocks the removal: the user exits it in its terminal first.
//! Processes that outlived an exited shell (`nohup`, `disown`, a bash `&` job)
//! only warn. They are found by the shell's session (ADR-0171 Follow-up 1),
//! so processes started outside Kagi are never seen.

use std::path::{Path, PathBuf};

use crate::plan_note::WorktreeNote;

/// A shell Kagi started in one of its terminals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KagiShell {
    /// The worktree the terminal belongs to (canonical).
    pub worktree: PathBuf,
    /// The shell's session id (its PID: the PTY spawn runs `setsid`), when
    /// the platform reported one.
    pub session: Option<u32>,
    /// Kagi has not observed the shell exit.
    pub live: bool,
}

/// The blocker and the warning a remove plan for `target` gains. `members`
/// counts the processes left in a session (`None` = could not tell, which
/// claims nothing).
pub fn remove_notes(
    target: &Path,
    shells: &[KagiShell],
    members: impl Fn(u32) -> Option<usize>,
) -> (Option<WorktreeNote>, Option<WorktreeNote>) {
    let path = target.display().to_string();
    let here = || shells.iter().filter(|shell| shell.worktree == target);
    let blocker = here()
        .any(|shell| shell.live)
        .then(|| WorktreeNote::RemoveLiveShell { path: path.clone() });
    let mut sessions: Vec<u32> = here()
        .filter(|shell| !shell.live)
        .filter_map(|shell| shell.session)
        .collect();
    sessions.sort_unstable();
    sessions.dedup();
    let count: usize = sessions.into_iter().filter_map(members).sum();
    let warning = (count > 0).then_some(WorktreeNote::RemoveLeftoverProcesses { path, count });
    (blocker, warning)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(worktree: &str, session: u32, live: bool) -> KagiShell {
        KagiShell {
            worktree: PathBuf::from(worktree),
            session: Some(session),
            live,
        }
    }

    fn left(session: u32) -> Option<usize> {
        match session {
            10 => Some(2),
            11 => Some(0),
            20 => Some(5),
            _ => None,
        }
    }

    #[test]
    fn a_live_shell_in_the_target_blocks() {
        let (blocker, _) = remove_notes(Path::new("/wt/a"), &[shell("/wt/a", 10, true)], left);
        assert_eq!(
            blocker,
            Some(WorktreeNote::RemoveLiveShell {
                path: "/wt/a".into()
            })
        );
    }

    #[test]
    fn a_live_shell_in_another_worktree_does_not_block() {
        let (blocker, warning) =
            remove_notes(Path::new("/wt/a"), &[shell("/wt/b", 20, true)], left);
        assert_eq!((blocker, warning), (None, None));
    }

    #[test]
    fn exited_shells_warn_with_what_their_sessions_left() {
        let shells = [
            shell("/wt/a", 10, false),
            shell("/wt/a", 10, false),
            shell("/wt/a", 11, false),
            shell("/wt/b", 20, false),
        ];
        let (blocker, warning) = remove_notes(Path::new("/wt/a"), &shells, left);
        assert_eq!(blocker, None, "an exited shell never blocks");
        assert_eq!(
            warning,
            Some(WorktreeNote::RemoveLeftoverProcesses {
                path: "/wt/a".into(),
                count: 2
            }),
            "one session counted once; another worktree's not at all"
        );
    }

    #[test]
    fn an_empty_or_unknown_session_claims_nothing() {
        let shells = [shell("/wt/a", 11, false), shell("/wt/a", 99, false)];
        assert_eq!(
            remove_notes(Path::new("/wt/a"), &shells, left),
            (None, None)
        );
    }

    #[test]
    fn a_live_shell_session_is_not_counted_as_left_behind() {
        let (blocker, warning) =
            remove_notes(Path::new("/wt/a"), &[shell("/wt/a", 10, true)], left);
        assert!(blocker.is_some());
        assert_eq!(warning, None);
    }
}
