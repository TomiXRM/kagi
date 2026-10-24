//! Conflict mode gate tests.
use super::*;
// ────────────────────────────────────────────────────────────
// Unit tests — the pure Conflict Mode gate / status / heading logic.
// (The render functions need a gpui window and are exercised manually.)
// ────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn git(dir: &std::path::Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .expect("git runs")
            .success();
        assert!(ok, "git {:?} failed", args);
    }

    /// `git merge` is allowed to exit non-zero (the conflict).
    fn git_allow_fail(dir: &std::path::Path, args: &[&str]) {
        let _ = Command::new("git").args(args).current_dir(dir).status();
    }

    /// Build a real merge-conflict repo and return its TempDir.
    fn merge_conflict_repo() -> TempDir {
        let td = TempDir::new().unwrap();
        let p = td.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.email", "t@e.com"]);
        git(p, &["config", "user.name", "t"]);
        std::fs::write(p.join("f.txt"), "line1\nline2\nline3\n").unwrap();
        git(p, &["add", "f.txt"]);
        git(p, &["commit", "-qm", "base"]);
        git(p, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(p.join("f.txt"), "line1\nFEATURE\nline3\n").unwrap();
        git(p, &["commit", "-qam", "feature"]);
        git(p, &["checkout", "-q", "main"]);
        std::fs::write(p.join("f.txt"), "line1\nMAIN\nline3\n").unwrap();
        git(p, &["commit", "-qam", "main"]);
        git_allow_fail(p, &["merge", "feature"]);
        td
    }

    /// Build a ConflictMode from a repo path, mirroring `detect_conflict_mode`.
    fn detect(repo_path: &std::path::Path, branch: &str) -> ConflictMode {
        let backend = kagi_git::Backend::open(repo_path).unwrap();
        let snapshot = backend
            .conflict_snapshot()
            .unwrap()
            .expect("conflict session");
        let mut session = snapshot.session;
        let buffer = backend.resolution_buffer_from_repo().unwrap();
        let residue = buffer.files_with_marker_residue();
        for f in &mut session.files {
            f.status = if buffer.has_resolution(&f.path) {
                if residue.contains(&f.path) {
                    ConflictStatus::NeedsReview
                } else {
                    ConflictStatus::Resolved
                }
            } else {
                ConflictStatus::Unresolved
            };
        }
        ConflictMode {
            revision: snapshot.observation.revision,
            session,
            buffer,
            current_branch: branch.to_string(),
            selected_file: Some(0),
            editing_file: None,
            skip_approval: None,
        }
    }

    #[test]
    fn continue_gate_blocks_until_resolved() {
        let td = merge_conflict_repo();
        let mut mode = detect(td.path(), "main");

        // Detected: one unresolved content conflict → continue is blocked.
        assert_eq!(mode.session.total_count(), 1);
        assert_eq!(mode.resolved_count(), 0);
        assert_eq!(mode.conflicted_count(), 1);
        assert!(!mode.can_continue(), "gate must block while unresolved");
        assert_eq!(
            mode.continue_blocker(),
            Some(Msg::ConflictBlockerUnresolved)
        );

        // Apply a side choice → resolved, no marker residue → gate opens.
        let path = mode.session.files[0].path.clone();
        mode.buffer
            .apply_choice(&path, kagi_git::ResolutionChoice::Current)
            .unwrap();
        // Recompute status as the UI handler does.
        let residue = mode.buffer.files_with_marker_residue();
        mode.session.files[0].status = if residue.contains(&path) {
            ConflictStatus::NeedsReview
        } else {
            ConflictStatus::Resolved
        };

        assert_eq!(mode.resolved_count(), 1);
        assert!(
            mode.can_continue(),
            "gate must open once all files resolved"
        );
        assert_eq!(mode.continue_blocker(), None);
    }

    #[test]
    fn marker_residue_keeps_gate_closed() {
        let td = merge_conflict_repo();
        let mut mode = detect(td.path(), "main");
        let path = mode.session.files[0].path.clone();

        // A manual edit that still contains conflict markers is residue.
        mode.buffer
            .set_manual_text(
                &path,
                "<<<<<<< HEAD\nMAIN\n=======\nFEATURE\n>>>>>>> feature\n",
            )
            .unwrap();
        assert!(mode.buffer.has_resolution(&path));
        assert!(
            !mode.can_continue(),
            "marker residue must keep the continue gate closed"
        );
        assert_eq!(mode.continue_blocker(), Some(Msg::ConflictBlockerMarker));
    }

    /// ADR-0058 guard, retargeted: the op-summary banner is gone (ADR-0135),
    /// so the workspace's side labels now come from `mode.labels()`
    /// — assert THOSE never leak ours/theirs and name the branch verbatim.
    #[test]
    fn side_labels_use_roles_not_ours_theirs() {
        let td = merge_conflict_repo();
        let mode = detect(td.path(), "main");
        let labels = mode.labels();
        for text in [
            &labels.current.role,
            &labels.current.name,
            &labels.incoming.role,
            &labels.incoming.name,
        ] {
            let lower = text.to_lowercase();
            assert!(!lower.contains("ours"), "label leaked 'ours': {}", text);
            assert!(!lower.contains("theirs"), "label leaked 'theirs': {}", text);
        }
        assert_eq!(labels.current.name, "main", "current side names the branch");
    }
}
