use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};
use gpui::VisualTestAppContext;
use kagi_ui_core::{
    i18n::{self, Lang},
    theme,
};

pub fn scenario_rebase_card_scope(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom"]);
    let language = i18n::lang();
    let zoom = theme::zoom();
    let fixture = build_fixture();
    let path = fixture.path();
    git(path, &["checkout", "-qb", "feature", "HEAD~1"]);
    for n in 1..=3 {
        std::fs::write(path.join(format!("scope-{n}")), format!("{n}\n")).unwrap();
        git(path, &["add", "."]);
        git(path, &["commit", "-qm", &format!("scope commit {n}")]);
    }
    let before = repo_fingerprint(path);
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        for scale in [1., 1.67] {
            theme::set_zoom(scale);
            let (app, window) = mount(cx, path);
            app.update(cx, |app, cx| app.open_rebase_modal("main".into(), cx));
            app.update(cx, |app, cx| {
                assert_eq!(app.planning, Some("rebase-plan"));
                assert!(matches!(&app.status_footer,
                    kagi::ui::FooterStatus::Busy(text) if text.as_ref() == match lang {
                        Lang::En => "Planning rebase…",
                        Lang::Ja => "rebase を計画中…",
                    }), "the shared planning tag has a localized footer label");
                app.open_rebase_modal("feature".into(), cx);
                assert!(matches!(&app.status_footer,
                    kagi::ui::FooterStatus::Idle(text) if text.as_ref() == i18n::Msg::OpInProgress.t()),
                    "a second Rebase plan must be refused with OpInProgress");
                assert_eq!(app.planning, Some("rebase-plan"), "refusal retains the first planning latch");
            });
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                cx.run_until_parked();
                if cx.read(|cx| app.read(cx).planning.is_none()) {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "rebase planning did not finish"
                );
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            cx.update_window(window, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear();
            })
            .unwrap();
            cx.read(|cx| {
                let plan = &app.read(cx).rebase_current_onto_modal().unwrap().plan;
                assert_eq!(
                    plan.preview_commits.len(),
                    3,
                    "rebase card must disclose three replay candidates"
                );
                let scope = plan.rebase_scope.as_ref().unwrap();
                assert_eq!(scope.total_commits, 3);
                assert_eq!(plan.current.head, "branch: feature");
                assert_eq!(plan.predicted.head, "branch: feature");
                assert_eq!(
                    plan.predicted.dirty, "clean",
                    "receipt input remains semantic"
                );
                assert_ne!(scope.current_oid, scope.onto_oid);
                assert_eq!(
                    app.read(cx).rebase_current_onto_modal().unwrap().onto,
                    "main",
                    "only the first Rebase request may be planned"
                );
                for (i, entry) in plan.preview_commits.iter().enumerate() {
                    assert!(entry.ends_with(&format!("scope commit {}", i + 1)));
                }
            });
            assert_eq!(
                repo_fingerprint(path),
                before,
                "opening rebase scope is read-only"
            );
            crate::native_ax::activate(cx);
            cx.update_window(window, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear();
            })
            .unwrap();
            let tree = crate::native_ax::tree(cx, window).to_string();
            assert!(
                tree.contains(match lang {
                    Lang::En => "Replay candidates: 3",
                    Lang::Ja => "replay 候補の commit: 3",
                }),
                "native AX must expose the localized replay scope heading: {tree}"
            );
            assert!(
                tree.contains(match lang {
                    Lang::En => "Result OID is not yet known",
                    Lang::Ja => "実行後の OID は未確定",
                }),
                "AFTER must disclose that the result OID is unknown"
            );
            let scope = cx.read(|cx| {
                app.read(cx)
                    .rebase_current_onto_modal()
                    .unwrap()
                    .plan
                    .rebase_scope
                    .clone()
                    .unwrap()
            });
            assert!(tree.contains(&format!("{} @ {}", scope.current_ref, scope.current_oid)));
            assert!(tree.contains(&format!(
                "{} onto {} @ {}",
                scope.current_ref, scope.onto_ref, scope.onto_oid
            )));
            let entries = cx.read(|cx| {
                app.read(cx)
                    .rebase_current_onto_modal()
                    .unwrap()
                    .plan
                    .preview_commits
                    .clone()
            });
            let list = kagi::ui::e2e::control_bounds(window.window_id(), "modal-target-list")
                .expect("replay list drawn");
            for entry in entries {
                let bounds = kagi::ui::e2e::control_bounds(
                    window.window_id(),
                    &format!("modal-commit-{entry}"),
                )
                .expect("replay candidate drawn");
                assert!(
                    bounds.origin.y >= list.origin.y && bounds.bottom() <= list.bottom(),
                    "all three replay candidates fit the scope list"
                );
            }
            if lang == Lang::Ja && scale == 1.67 {
                let label = cx.read(|cx| {
                    app.read(cx)
                        .rebase_current_onto_modal()
                        .unwrap()
                        .confirm_label()
                        .to_string()
                });
                assert!(
                    crate::native_ax::press(cx, window, &label),
                    "native confirmation must execute Rebase"
                );
                crate::recovery_operations::wait_idle(cx, &app);
                let entries: Vec<_> = kagi_git::oplog::read_oplog_tail_for_repo(path, 100)
                    .into_iter()
                    .filter(|entry| entry.op == "rebase")
                    .collect();
                assert_eq!(
                    entries.len(),
                    1,
                    "one admitted request produces one receipt"
                );
                let kagi_git::oplog::OpOutcome::Success { after } = &entries[0].outcome else {
                    panic!(
                        "expected a successful durable Rebase receipt: {:?}",
                        entries[0].outcome
                    );
                };
                assert_eq!(after.head, "branch: feature");
                assert_eq!(
                    after.dirty, "clean",
                    "UI-only unknown result wording must not reach receipts"
                );
            }
            unmount(cx, app, window);
        }
    }
    i18n::set_lang(language);
    theme::set_zoom(zoom);
}
