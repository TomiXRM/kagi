use super::*;
use crate::plan_note::{push::PushPunct, DirtyParts, PullNote, PullTitle, PushNote, PushTitle};

fn plan(title: PlanTitle) -> OperationPlan {
    OperationPlan {
        title,
        current: StateSummary {
            head: String::new(),
            dirty: String::new(),
        },
        predicted: StateSummary {
            head: String::new(),
            dirty: String::new(),
        },
        warnings: Vec::new(),
        blockers: Vec::new(),
        recovery: None,
        disposition: PlanDisposition::Ready,
        head_at_plan: Head::Unborn {
            branch: "main".into(),
        },
        stash_count_at_plan: 0,
        stash_identity: None,
        pull_identity: None,
        tag_push_identity: None,
        rebase_scope: None,
        approved_index_digest: None,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
        equivalent_command: None,
    }
}

fn pull_titles() -> [PlanTitle; 2] {
    [
        PlanTitle::Pull(PullTitle::Pull {
            branch: "main".into(),
            remote: "origin".into(),
            behind: 1,
        }),
        PlanTitle::Pull(PullTitle::PullBranchFf {
            branch: "other".into(),
            remote: "origin".into(),
            behind: 1,
        }),
    ]
}

#[test]
fn sync_admission_push_and_publish_ignore_only_no_force_information() {
    for set_upstream in [false, true] {
        for title in [
            PushTitle::Push {
                branch: "main".into(),
                remote: "origin".into(),
                set_upstream,
            },
            PushTitle::PushBranch {
                branch: "other".into(),
                remote: "origin".into(),
                set_upstream,
            },
        ] {
            let mut plan = plan(PlanTitle::Push(title));
            assert!(
                !plan.sync_needs_choice(),
                "empty notes remain quiet after warning cleanup"
            );
            for punct in [PushPunct::EmDash, PushPunct::Semicolon] {
                plan.warnings = vec![PlanNote::Push(PushNote::NoForceUsed { punct })];
                assert!(!plan.sync_needs_choice());
            }
            plan.warnings
                .push(PlanNote::Push(PushNote::PublishRemoteAmbiguous {
                    candidates: vec!["company".into(), "personal".into()],
                }));
            assert!(
                plan.sync_needs_choice(),
                "publishing needs an unambiguous destination"
            );
            plan.warnings.pop();
            plan.warnings
                .push(PlanNote::Pull(PullNote::MergePrediction));
            assert!(
                plan.sync_needs_choice(),
                "an unrecognized note is not silently admitted"
            );
            plan.warnings.clear();
            plan.blockers
                .push(PlanNote::Push(PushNote::NoUpstreamNoRemotes {
                    branch: "other".into(),
                }));
            assert!(plan.sync_needs_choice());
        }
    }
}

#[test]
fn sync_admission_pull_keeps_every_risk_and_blocker_explicit() {
    let risks = [
        PullNote::DirtyPullGuard {
            parts: DirtyParts {
                staged: 1,
                modified: 0,
            },
        },
        PullNote::LocalChangesOverlap {
            paths: vec!["README.md".into()],
        },
        PullNote::AutoStash {
            parts: DirtyParts {
                staged: 1,
                modified: 0,
            },
            untracked: 0,
        },
        PullNote::MergePrediction,
        PullNote::RestoreConflict {
            paths: vec!["README.md".into()],
        },
        PullNote::RestoreConflictPossible {
            paths: vec!["README.md".into()],
        },
        PullNote::RemoteDirty,
    ];
    for title in pull_titles() {
        let mut plan = plan(title);
        assert!(!plan.sync_needs_choice());
        for risk in &risks {
            plan.warnings = vec![PlanNote::Pull(risk.clone())];
            assert!(plan.sync_needs_choice(), "risk {risk:?}");
        }
        plan.warnings.clear();
        plan.blockers
            .push(PlanNote::Pull(PullNote::CannotFastForward {
                branch: "main".into(),
            }));
        assert!(plan.sync_needs_choice());
        plan.blockers.clear();
        plan.destructive = true;
        assert!(plan.sync_needs_choice());
    }
}

#[test]
fn sync_admission_ref_only_information_does_not_authorize_checkout_risk() {
    let [current_title, ref_only_title] = pull_titles();
    for note in [
        PullNote::DirtyRefOnly,
        PullNote::ConflictedRefOnly { count: 1 },
    ] {
        let mut ref_only = plan(ref_only_title.clone());
        ref_only.warnings.push(PlanNote::Pull(note.clone()));
        assert!(!ref_only.sync_needs_choice());
        let mut current = plan(current_title.clone());
        current.warnings.push(PlanNote::Pull(note));
        assert!(current.sync_needs_choice());
        ref_only
            .warnings
            .push(PlanNote::Pull(PullNote::LocalChangesOverlap {
                paths: vec!["a".into()],
            }));
        assert!(ref_only.sync_needs_choice());
    }
}

#[test]
fn sync_admission_does_not_make_remote_pull_or_other_operations_quiet() {
    for title in [
        PlanTitle::Pull(PullTitle::PullRemote {
            branch: "main".into(),
            upstream: "origin/main".into(),
            behind: 1,
        }),
        PlanTitle::Push(PushTitle::SetUpstream {
            branch: "main".into(),
            upstream: "origin/main".into(),
        }),
    ] {
        assert!(plan(title).sync_needs_choice());
    }
}
