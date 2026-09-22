//! #705: the real merge boundary delivers local cleanup evidence in EN/JA.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{Entity, VisualTestAppContext};
use kagi::ui::{
    e2e,
    i18n::{self, Lang},
    KagiApp,
};
use std::{ffi::OsString, os::unix::fs::PermissionsExt};

struct RestoreEnvironment {
    path: Option<OsString>,
    language: Lang,
}
impl Drop for RestoreEnvironment {
    fn drop(&mut self) {
        match self.path.take() {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
        i18n::set_lang(self.language);
    }
}

/// What the approval promised about the local branch, and what the merge
/// boundary then did with it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cleanup {
    /// The frozen branch is still exactly what was approved: it is deleted.
    Deleted,
    /// Routine absence is recorded without an acknowledgement notice.
    Absent,
    /// The branch moves between approval and execution — the one case that is
    /// a genuine `Partial`: merged, local cleanup refused.
    Drifted,
    /// The PR head is the branch checked out here, so the *approval itself*
    /// keeps it. A promise kept is not a failure.
    KeptCheckedOut,
    /// The local branch is not at the PR head, so deleting it would throw away
    /// commits the merge never took.
    KeptNotAtPrHead,
    /// GitHub accepted the request but has not merged yet (merge queue): the
    /// branch is kept because nothing has landed to make it redundant.
    KeptQueued,
}

impl Cleanup {
    /// The deletion was promised and carried out, or explicitly not promised.
    /// Only a broken promise leaves the merge unconfirmed.
    fn confirmed(self) -> bool {
        self != Self::Drifted
    }

    fn tag(self) -> &'static str {
        match self {
            Self::Deleted => "deleted",
            Self::Absent => "absent",
            Self::Drifted => "drifted",
            Self::KeptCheckedOut => "kept-checked-out",
            Self::KeptNotAtPrHead => "kept-not-at-head",
            Self::KeptQueued => "kept-queued",
        }
    }
}

/// Pump until the background plan latch releases.
///
/// PR merge planning runs off the UI thread (ADR-0141), so the confirmation
/// does not exist until this returns — and neither does any partially built
/// plan the user could confirm early.
pub(super) fn settle_plan(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while cx.read(|cx| app.read(cx).planning.is_some()) {
        cx.run_until_parked();
        assert!(
            std::time::Instant::now() < deadline,
            "the pr-merge plan never settled"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    cx.run_until_parked();
}

pub(super) fn local_cleanup_notices(cx: &mut VisualTestAppContext) {
    let restore = RestoreEnvironment {
        path: std::env::var_os("PATH"),
        language: i18n::lang(),
    };
    let bin = tempfile::tempdir().unwrap();
    let gh = bin.path().join("gh");
    // The two answers each scenario rewrites: what `gh pr merge` printed, and
    // what the authoritative `gh pr view --json mergedAt` re-read says. A
    // queued merge is exit-zero with a null `mergedAt`, which is exactly how
    // the merge queue is told apart from a landed merge.
    let merge_out = bin.path().join("merge.out");
    let view_json = bin.path().join("view.json");
    std::fs::write(
        &gh,
        format!(
            "#!/bin/sh\ncase \"$1 $2\" in\n'pr merge') cat '{merge}' ;;\n'pr view') cat '{view}' ;;\n*) echo '[]' ;;\nesac\n",
            merge = merge_out.display(),
            view = view_json.display(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut paths = vec![bin.path().to_path_buf()];
    paths.extend(std::env::split_paths(
        restore.path.as_deref().unwrap_or_default(),
    ));
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        for cleanup in [
            Cleanup::Deleted,
            Cleanup::Absent,
            Cleanup::Drifted,
            Cleanup::KeptCheckedOut,
            Cleanup::KeptNotAtPrHead,
            Cleanup::KeptQueued,
        ] {
            if cleanup == Cleanup::KeptQueued {
                std::fs::write(&merge_out, "queued\n").unwrap();
                std::fs::write(&view_json, "{\"mergedAt\":null}\n").unwrap();
            } else {
                std::fs::write(&merge_out, "merged\n").unwrap();
                std::fs::write(&view_json, "{\"mergedAt\":\"2026-09-22T00:00:00Z\"}\n").unwrap();
            }
            exercise_cleanup(cx, language, cleanup);
        }
    }
    eprintln!("[gui-e2e] PASS PR local cleanup: EN/JA fork warning; async plan; deleted, drifted, kept (checked out / not at head / queued) receipts");
}

fn exercise_cleanup(cx: &mut VisualTestAppContext, language: Lang, cleanup: Cleanup) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    git(&repo, &["branch", "feature", "HEAD~1"]);
    let raw = git2::Repository::open(&repo).unwrap();
    let feature_tip = raw.refname_to_id("refs/heads/feature").unwrap().to_string();
    let main_tip = raw.refname_to_id("refs/heads/main").unwrap().to_string();
    // `main` is the checked-out branch, so a PR whose head is `main` is the
    // "cannot delete what you are standing on" case; `feature` is deletable.
    let (head, head_sha) = match cleanup {
        Cleanup::Absent => ("missing-feature", feature_tip.clone()),
        Cleanup::KeptCheckedOut => ("main", main_tip.clone()),
        // The PR merged `main`'s tip; the local `feature` still points a commit
        // behind it, so it is not the branch that landed.
        Cleanup::KeptNotAtPrHead => ("feature", main_tip.clone()),
        _ => ("feature", feature_tip.clone()),
    };
    let pr = kagi_domain::github::PullRequest {
        number: 705,
        head: head.into(),
        head_sha,
        base: "main".into(),
        base_repo: "github.example/acme/base".into(),
        cross_repository: true,
        ..Default::default()
    };
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.open_pr_merge_modal(&pr, kagi_git::github::MergeMethod::Squash, true, cx);
        // ADR-0141: the whole plan — including the delete-branch probe over the
        // PR head — is built off the UI thread, so in the turn that started it
        // there is a busy state and nothing to confirm.
        assert!(
            app.pr_merge_modal().is_none(),
            "no confirmable PR merge plan may exist before it is built"
        );
        assert_eq!(app.planning, Some("merge-plan"));
        assert_eq!(
            e2e::busy_snackbar_label(app),
            Some(match language {
                Lang::En => "Planning merge…",
                Lang::Ja => "merge を計画中…",
            })
        );
    });
    settle_plan(cx, &app);
    app.update(cx, |app, _| {
        assert_eq!(app.planning, None, "the plan latch must terminalize");
        assert!(
            !matches!(app.status_footer, kagi::ui::FooterStatus::Busy(_)),
            "a settled plan must end its Busy footer: {:?}",
            app.status_footer
        );
        let plan = &app.pr_merge_modal().expect("merge confirmation").plan;
        assert!(
            plan.blockers.is_empty(),
            "a branch that cannot be deleted keeps the branch; it does not block the merge"
        );
        let warning = match language {
            Lang::En => "The remote branch in the fork is not deleted by gh.",
            Lang::Ja => "fork 側の remote branch は gh では削除されません。",
        };
        assert!(plan
            .warnings
            .iter()
            .any(|note| i18n::plan_note_text(note) == warning));
        // A planned keep is stated before the user confirms, not discovered
        // afterwards from the receipt.
        let planned_keep = match language {
            Lang::En => "is kept, not deleted",
            Lang::Ja => "local branch は削除しません",
        };
        let announced = plan
            .warnings
            .iter()
            .any(|note| i18n::plan_note_text(note).contains(planned_keep));
        assert_eq!(
            announced,
            matches!(cleanup, Cleanup::KeptCheckedOut | Cleanup::KeptNotAtPrHead),
            "only an approval that keeps the branch may say so in the plan"
        );
    });
    let capture_tag = format!("pr-merge-local-{language:?}-{}", cleanup.tag());
    crate::macos::capture_screenshot_best_effort(cx, window, &format!("{capture_tag}-plan"));
    if cleanup == Cleanup::Drifted {
        git(&repo, &["branch", "-f", "feature", "HEAD"]);
    }
    app.update(cx, |app, cx| app.start_pr_merge(cx));
    cx.run_until_parked();
    // Every fragment must be in the one notice: the localized sentence for the
    // outcome *and*, where the reason names them, the exact objects it is
    // about. Fragments rather than one literal only where the reason
    // interpolates an OID whose rendering is the reason's business.
    let expected: Vec<String> = match (language, cleanup) {
        (_, Cleanup::Absent) => vec![],
        (Lang::En, Cleanup::Deleted) => vec!["local branch deleted: feature@".into()],
        (Lang::Ja, Cleanup::Deleted) => vec!["local branch を削除しました: feature@".into()],
        (Lang::En, Cleanup::Drifted) => {
            vec!["local branch not deleted: the branch moved after approval".into()]
        }
        (Lang::Ja, Cleanup::Drifted) => {
            vec![
                "merge は完了しました。local branch は削除していません: 承認後に branch が動きました"
                    .into(),
            ]
        }
        (Lang::En, Cleanup::KeptCheckedOut) => vec![
            "local branch kept: main (".into(),
            "currently checked-out branch".into(),
        ],
        (Lang::Ja, Cleanup::KeptCheckedOut) => vec![
            "local branch を残しました: main (".into(),
            "checkout 中の branch は削除できません".into(),
        ],
        (Lang::En, Cleanup::KeptNotAtPrHead) => vec![
            "local branch kept: feature (the local branch is at ".into(),
            "not the merged PR head".into(),
        ],
        (Lang::Ja, Cleanup::KeptNotAtPrHead) => vec![
            "local branch を残しました: feature (local branch は ".into(),
            "merge された PR head".into(),
            "と一致しません".into(),
        ],
        (Lang::En, Cleanup::KeptQueued) => vec![
            "local branch kept: feature (GitHub queued the merge, so nothing has been merged yet)"
                .into(),
        ],
        (Lang::Ja, Cleanup::KeptQueued) => vec![
            "local branch を残しました: feature (GitHub が merge を queue に入れたため、まだ merge されていません)"
                .into(),
        ],
    };
    cx.read(|cx| {
        let state = app.read(cx);
        if cleanup == Cleanup::Absent {
            assert!(e2e::app_notice_message(state).is_none());
            assert!(!e2e::queued_notice_contains(state, "local branch"));
        }
        for fragment in &expected {
            assert!(
                e2e::app_notice_message(state).is_some_and(|message| message.contains(fragment))
                    || e2e::queued_notice_contains(state, fragment),
                "missing cleanup notice: {fragment}"
            );
        }
        assert!(!state.app_sessions.has_leases());
    });
    crate::macos::capture_screenshot_best_effort(cx, window, &format!("{capture_tag}-notice"));
    let entries = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100);
    let merges: Vec<_> = entries
        .iter()
        .filter(|entry| entry.op == "pr-merge")
        .collect();
    assert_eq!(merges.len(), 1);
    match cleanup {
        Cleanup::Absent => {
            assert!(raw.find_reference("refs/heads/missing-feature").is_err());
            let kagi_git::oplog::OpOutcome::Success { after } = &merges[0].outcome else {
                panic!("absence fulfills cleanup without a warning modal");
            };
            assert!(after
                .dirty
                .contains("local branch already absent: missing-feature"));
            assert!(merges[0].backup_refs.is_empty());
        }
        Cleanup::Deleted => {
            assert!(raw.find_reference("refs/heads/feature").is_err());
            assert!(matches!(
                merges[0].outcome,
                kagi_git::oplog::OpOutcome::Success { .. }
            ));
            assert_eq!(merges[0].backup_refs.len(), 1);
        }
        Cleanup::Drifted => {
            assert_eq!(
                raw.refname_to_id("refs/heads/feature").unwrap(),
                raw.head().unwrap().target().unwrap()
            );
            assert!(matches!(
                merges[0].outcome,
                kagi_git::oplog::OpOutcome::Partial { .. }
            ));
        }
        // A kept branch is a kept promise: the branch survives untouched, with
        // nothing to restore and nothing left unfinished.
        Cleanup::KeptCheckedOut | Cleanup::KeptNotAtPrHead | Cleanup::KeptQueued => {
            assert_eq!(
                raw.refname_to_id(&format!("refs/heads/{head}"))
                    .unwrap()
                    .to_string(),
                match cleanup {
                    Cleanup::KeptCheckedOut => main_tip.clone(),
                    _ => feature_tip.clone(),
                },
                "a kept branch must not be moved or deleted"
            );
            assert!(
                matches!(
                    merges[0].outcome,
                    kagi_git::oplog::OpOutcome::Success { .. }
                ),
                "keeping the branch on purpose is a success, not a partial: {:?}",
                merges[0].outcome
            );
            assert!(
                merges[0].backup_refs.is_empty(),
                "nothing was deleted, so nothing was backed up"
            );
        }
    }
    // The transport hold is what refuses a second merge of the same PR. Only
    // an unfinished promise earns one: a confirmed receipt must leave the
    // button working, and the proof is that pressing it plans again.
    app.update(cx, |app, cx| {
        app.open_pr_merge_modal(&pr, kagi_git::github::MergeMethod::Squash, true, cx);
        if cleanup.confirmed() {
            assert_eq!(
                app.planning,
                Some("merge-plan"),
                "a confirmed merge holds nothing back: the next plan must start"
            );
        } else {
            assert_eq!(
                app.planning, None,
                "an unfinished merge is refused by the hold, before any planning starts"
            );
            assert!(
                app.pr_merge_modal().is_none(),
                "local cleanup failure must not offer re-merge"
            );
        }
    });
    settle_plan(cx, &app);
    unmount(cx, app, window);
}
