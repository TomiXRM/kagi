//! #705: the real merge boundary delivers local cleanup evidence in EN/JA.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::{
    e2e,
    i18n::{self, Lang},
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

pub(super) fn local_cleanup_notices(cx: &mut VisualTestAppContext) {
    let restore = RestoreEnvironment {
        path: std::env::var_os("PATH"),
        language: i18n::lang(),
    };
    let bin = tempfile::tempdir().unwrap();
    let gh = bin.path().join("gh");
    std::fs::write(&gh, "#!/bin/sh\ncase \"$1 $2\" in\n'pr merge') echo merged ;;\n'pr view') echo '{\"mergedAt\":\"2026-09-22T00:00:00Z\"}' ;;\n*) echo '[]' ;;\nesac\n").unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut paths = vec![bin.path().to_path_buf()];
    paths.extend(std::env::split_paths(
        restore.path.as_deref().unwrap_or_default(),
    ));
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        for moved in [false, true] {
            exercise_cleanup(cx, language, moved);
        }
    }
    eprintln!("[gui-e2e] PASS PR local cleanup: EN/JA fork warning, deleted and kept notices, exact ref and receipt");
}

fn exercise_cleanup(cx: &mut VisualTestAppContext, language: Lang, moved: bool) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    git(&repo, &["branch", "feature", "HEAD~1"]);
    let raw = git2::Repository::open(&repo).unwrap();
    let tip = raw.refname_to_id("refs/heads/feature").unwrap().to_string();
    let pr = kagi_domain::github::PullRequest {
        number: 705,
        head: "feature".into(),
        head_sha: tip,
        base: "main".into(),
        base_repo: "github.example/acme/base".into(),
        cross_repository: true,
        ..Default::default()
    };
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.open_pr_merge_modal(&pr, kagi_git::github::MergeMethod::Squash, true, cx);
        let plan = &app.pr_merge_modal().expect("merge confirmation").plan;
        assert!(plan.blockers.is_empty());
        let warning = match language {
            Lang::En => "The remote branch in the fork is not deleted by gh.",
            Lang::Ja => "fork 側の remote branch は gh では削除されません。",
        };
        assert!(plan
            .warnings
            .iter()
            .any(|note| i18n::plan_note_text(note) == warning));
    });
    if moved {
        git(&repo, &["branch", "-f", "feature", "HEAD"]);
    }
    app.update(cx, |app, cx| app.start_pr_merge(cx));
    cx.run_until_parked();
    let expected = match (language, moved) {
        (Lang::En, false) => "local branch deleted: feature@",
        (Lang::Ja, false) => "local branch を削除しました: feature@",
        (Lang::En, true) => "local branch not deleted:",
        (Lang::Ja, true) => "merge は完了しました。local branch は削除していません:",
    };
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            e2e::app_notice_message(state).is_some_and(|message| message.contains(expected))
                || e2e::queued_notice_contains(state, expected),
            "missing cleanup notice: {expected}"
        );
        assert!(!state.app_sessions.has_leases());
    });
    let entries = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100);
    let merges: Vec<_> = entries
        .iter()
        .filter(|entry| entry.op == "pr-merge")
        .collect();
    assert_eq!(merges.len(), 1);
    if moved {
        assert_eq!(
            raw.refname_to_id("refs/heads/feature").unwrap(),
            raw.head().unwrap().target().unwrap()
        );
        assert!(matches!(
            merges[0].outcome,
            kagi_git::oplog::OpOutcome::Partial { .. }
        ));
        app.update(cx, |app, cx| {
            app.open_pr_merge_modal(&pr, kagi_git::github::MergeMethod::Squash, true, cx);
            assert!(
                app.pr_merge_modal().is_none(),
                "local cleanup failure must not offer re-merge"
            );
        });
    } else {
        assert!(raw.find_reference("refs/heads/feature").is_err());
        assert!(matches!(
            merges[0].outcome,
            kagi_git::oplog::OpOutcome::Success { .. }
        ));
        assert_eq!(merges[0].backup_refs.len(), 1);
    }
    unmount(cx, app, window);
}
