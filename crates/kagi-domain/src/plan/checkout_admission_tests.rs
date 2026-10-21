use super::*;
use crate::plan_note::{CheckoutNote, CheckoutTitle, CommonNote, NoOpKind, UntrackedCtx};

fn branch_plan() -> OperationPlan {
    let current = StateSummary {
        head: "branch: main".into(),
        dirty: "1 staged, 1 modified, 1 untracked".into(),
    };
    OperationPlan {
        title: PlanTitle::Checkout(CheckoutTitle::Checkout {
            branch: "next".into(),
        }),
        predicted: StateSummary {
            head: "branch: next".into(),
            ..current.clone()
        },
        current,
        warnings: Vec::new(),
        blockers: Vec::new(),
        recovery: None,
        disposition: PlanDisposition::Ready,
        head_at_plan: Head::Attached {
            branch: "main".into(),
            target: "a".repeat(40),
        },
        stash_count_at_plan: 0,
        stash_identity: None,
        pull_identity: None,
        tag_push_identity: None,
        push_identity: None,
        rebase_scope: None,
        approved_index_digest: None,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
        equivalent_command: None,
    }
}

#[test]
fn collision_free_dirty_branch_checkout_runs_quietly() {
    let mut plan = branch_plan();
    assert!(!plan.checkout_needs_choice());
    plan.warnings
        .push(PlanNote::Common(CommonNote::UntrackedRemain {
            count: 1,
            ctx: UntrackedCtx::AfterCheckout,
        }));
    assert!(
        !plan.checkout_needs_choice(),
        "preserved-file information is not a caution"
    );
}

#[test]
fn every_blocker_prevents_quiet_checkout_even_if_disposition_is_ready() {
    for blocker in [
        PlanNote::Checkout(CheckoutNote::CheckoutOverlap {
            count: 1,
            files: vec!["local.txt".into()],
        }),
        PlanNote::Checkout(CheckoutNote::AlreadyCurrent {
            branch: "main".into(),
        }),
        PlanNote::Common(CommonNote::GitIdentityUnavailable),
    ] {
        let mut plan = branch_plan();
        plan.blockers.push(blocker);
        assert!(plan.checkout_needs_choice());
    }
}

#[test]
fn checkout_cautions_and_unrecognized_notes_keep_confirmation() {
    for warning in [
        PlanNote::Checkout(CheckoutNote::DirtyMayFail {
            display: "1 modified".into(),
        }),
        PlanNote::Checkout(CheckoutNote::WillDetachHead),
        PlanNote::Checkout(CheckoutNote::RecommendCreateBranchHereFirst),
        // A checkout blocker unexpectedly arriving as a warning is not information.
        PlanNote::Checkout(CheckoutNote::CommitAlreadyHead),
        PlanNote::Common(CommonNote::GitIdentityUnavailable),
        PlanNote::Common(CommonNote::UntrackedRemain {
            count: 1,
            ctx: UntrackedCtx::AfterSwitching,
        }),
    ] {
        let mut plan = branch_plan();
        plan.warnings.push(warning.clone());
        assert!(
            plan.checkout_needs_choice(),
            "unapproved warning: {warning:?}"
        );
    }
}

#[test]
fn detached_other_operation_and_nonready_plans_never_quiet_admit() {
    let mut plan = branch_plan();
    plan.title = PlanTitle::Checkout(CheckoutTitle::CheckoutCommit {
        sha: "abc1234".into(),
        summary: "target".into(),
    });
    assert!(
        plan.checkout_needs_choice(),
        "detaching requires a choice even without notes"
    );
    plan.title = PlanTitle::Discard {
        single: Some("local.txt".into()),
        count: 1,
    };
    assert!(
        plan.checkout_needs_choice(),
        "this API admits only branch checkout"
    );
    for disposition in [
        PlanDisposition::Blocked,
        PlanDisposition::NoOp(NoOpKind::PullUpToDate),
    ] {
        let mut plan = branch_plan();
        plan.disposition = disposition;
        assert!(plan.checkout_needs_choice());
    }
    let mut plan = branch_plan();
    plan.destructive = true;
    assert!(plan.checkout_needs_choice());
}
