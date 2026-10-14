//! #1534: English Revert confirmation must not contain Japanese generated prose.
use crate::macos::{build_fixture, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::{
    i18n::{self, Lang},
    KagiApp,
};
use kagi_git::CommitId;

pub fn scenario_revert_card_en_summary(cx: &mut VisualTestAppContext) {
    let previous = i18n::lang();
    let saved = kagi::ui::settings::read_setting("lang");
    struct Restore(Lang, Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            i18n::set_lang(self.0);
            kagi::ui::settings::write_setting("lang", self.1.as_deref());
        }
    }
    let _restore = Restore(previous, saved);
    i18n::set_lang(Lang::En);
    let fixture = build_fixture();
    let mut backend = kagi_git::Backend::open(fixture.path()).unwrap();
    let oid = match backend.snapshot(100).unwrap().head {
        kagi_git::Head::Attached { target, .. } => target,
        _ => panic!("fixture must have an attached HEAD"),
    };
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app: &mut KagiApp, _| {
        app.open_revert_modal(CommitId(oid))
    });
    cx.run_until_parked();
    app.update(cx, |app, _| {
        let plan = &app.revert_modal().expect("Revert card").plan;
        assert!(plan.blockers.is_empty());
        assert_eq!(
            plan.predicted.head,
            "branch: main (+1 revert commit が新規作成されます: Revert \"second commit\")",
            "receipt state must retain the legacy bytes"
        );
    });
    kagi::ui::dialog_a11y::clear_recorded_a11y();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    let (_, after) =
        kagi::ui::dialog_a11y::recorded_note("plan-state-after").expect("rendered AFTER group");
    assert!(after.contains("+1 new revert commit"), "{after}");
    assert!(!after.contains("が新規作成されます"), "{after}");
    app.update(cx, |app, cx| app.start_revert(cx));
    cx.run_until_parked();
    let expected = "[kagi] footer: revert: branch: main → branch: main (+1 revert commit が新規作成されます: Revert \"second commit\")";
    assert!(
        kagi_ui_core::klog::tail()
            .iter()
            .any(|line| line == expected),
        "legacy Revert footer contract missing: {:?}",
        kagi_ui_core::klog::tail()
    );
    let records = kagi_git::oplog::read_oplog_tail_for_repo(fixture.path(), 100);
    let receipt = records
        .iter()
        .find(|record| record.op == "revert")
        .expect("Revert receipt");
    match &receipt.outcome {
        kagi_git::oplog::OpOutcome::Success { after } => assert_eq!(
            after.head,
            "branch: main (+1 revert commit が新規作成されます: Revert \"second commit\")"
        ),
        other => panic!("Revert must succeed: {other:?}"),
    }
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS revert_card_en_summary");
}
