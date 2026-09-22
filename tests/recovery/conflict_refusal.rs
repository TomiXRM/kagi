//! #711: real conflict controls must name the refusal, not just count blockers.
use super::{click_control, content_fixture, stuck_merging_fixture, wait_idle};
use crate::macos::{git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::{
    e2e,
    i18n::{self, Lang},
};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use std::path::Path;

struct RestoreLanguage(Lang);

impl Drop for RestoreLanguage {
    fn drop(&mut self) {
        i18n::set_lang(self.0);
    }
}

pub(super) fn save_and_abort(cx: &mut VisualTestAppContext) {
    let _restore_language = RestoreLanguage(i18n::lang());
    for (language, markers, changed) in [
        (
            Lang::En,
            "conflict markers remain",
            "conflict changed since it was observed",
        ),
        (
            Lang::Ja,
            "conflict marker が残っています",
            "conflict の状態が変わりました",
        ),
    ] {
        i18n::set_lang(language);
        save(cx, markers);
        abort(cx, changed, false);
        abort(cx, changed, true);
    }
}

fn save(cx: &mut VisualTestAppContext, expected: &str) {
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    conflict.update(cx, |view, cx| {
        view.conflict_open_editor(Path::new("file.txt"));
        view.conflict_editor_reset_all(Path::new("file.txt"));
        cx.notify();
    });
    assert!(cx.read(|cx| {
        let view = conflict.read(cx);
        let mode = view.mode.as_ref().expect("conflict mode");
        matches!(
            mode.buffer.conflict_draft(Path::new("file.txt")),
            Some(kagi_domain::conflict_family::ConflictDraft::Text(bytes))
                if bytes.windows(b"<<<<<<<".len()).any(|window| window == b"<<<<<<<")
        )
    }));
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let before = std::fs::read(repo.join("file.txt")).unwrap();
    click_control(cx, window, "conflict-save");
    cx.run_until_parked();
    let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "conflict-save:merge")
        .collect();
    assert_eq!(entries.len(), 1);
    assert!(
        matches!(&entries[0].outcome, OpOutcome::Refused { blockers }
        if blockers.iter().any(|reason| reason.contains("conflict markers remain")))
    );
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(repo.join("file.txt")).unwrap(), before);
    assert_refusal(
        cx,
        &app,
        expected,
        "conflict-save:merge: refused (1 blocker)",
    );
    drop(conflict);
    unmount(cx, app, window);
}

fn abort(cx: &mut VisualTestAppContext, expected: &str, banner: bool) {
    let fixture = if banner {
        stuck_merging_fixture()
    } else {
        content_fixture()
    };
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let control = if banner {
        "operation-strip-abort"
    } else {
        "conflict-abort"
    };
    click_control(cx, window, control);
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).conflict_abort_modal().is_some()));
    // Drift after the confirmation opens. What the user confirms is still the
    // revision they saw, so the family must refuse it by name.
    std::fs::write(repo.join("file.txt"), "resolved outside Kagi\n").unwrap();
    git(&repo, &["add", "file.txt"]);
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let merge_head = std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap();
    app.update(cx, |app, cx| app.confirm_conflict_abort(cx));
    wait_idle(cx, &app);
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert_refusal(cx, &app, expected, "merge-abort: refused (1 blocker)");
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(
        std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap(),
        merge_head
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("file.txt")).unwrap(),
        "resolved outside Kagi\n"
    );
    let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "merge-abort")
        .collect();
    assert_eq!(entries.len(), 1);
    assert!(
        matches!(&entries[0].outcome, OpOutcome::Refused { blockers }
        if blockers.iter().any(|reason| reason.contains("conflict changed since it was observed")))
    );

    // #755: a reload that lands while such a confirmation is open closes it
    // rather than refreshing it in place, and executes nothing on the way.
    click_control(cx, window, "app-notice-confirm");
    cx.run_until_parked();
    click_control(cx, window, control);
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).conflict_abort_modal().is_some()),
        "the control reopens a confirmation against the current state"
    );
    app.update(cx, |app, cx| app.reload(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).conflict_abort_modal().is_none()),
        "an accepted reload must sweep the stale abort confirmation"
    );
    assert_eq!(
        read_oplog_tail_for_repo(&repo, 100)
            .into_iter()
            .filter(|entry| entry.op == "merge-abort")
            .count(),
        1,
        "sweeping a confirmation must not run the operation it was showing"
    );
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(
        std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap(),
        merge_head
    );
    unmount(cx, app, window);
}

fn assert_refusal(
    cx: &VisualTestAppContext,
    app: &gpui::Entity<kagi::ui::KagiApp>,
    expected: &str,
    footer: &str,
) {
    cx.read(|cx| {
        let state = app.read(cx);
        let notice = e2e::app_notice_message(state).expect("refusal notice");
        assert!(
            notice.contains(expected),
            "notice must name the localized blocker: {notice}"
        );
        let toast = state
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .last()
            .unwrap();
        assert!(
            toast.message.contains(expected),
            "toast must name the localized blocker: {}",
            toast.message
        );
        assert!(
            matches!(&state.status_footer, kagi::ui::FooterStatus::Failed(message)
            if message.as_ref() == footer),
            "footer/klog contract must remain unchanged"
        );
        assert!(!state.app_sessions.has_leases());
    });
}
