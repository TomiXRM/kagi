//! #1588: legacy receipt data stays raw while native presentation is localized.
use crate::macos::{build_fixture, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::{
    i18n::{self, Lang},
    oplog_panel::{self, keyboard, OpLogPanel},
    BottomTab, FooterStatus,
};
use kagi_git::{
    oplog::{OpLogEntry, OpOutcome},
    ops::StateSummary,
    CommitId,
};

fn user_text(text: &str) {
    for internal in [
        "loaded window",
        "session=",
        "hunks=[",
        "stage 0",
        "before=",
        "conflict-save:",
        "repo.index",
        "preflight",
        "plan failed",
    ] {
        assert!(!text.contains(internal), "UI exposes {internal}: {text}");
    }
}

pub fn scenario_user_text_native(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    let mut receipt = OpLogEntry::new(
        "conflict-save:merge",
        fixture.path().to_str().unwrap(),
        StateSummary {
            head: "session=merge file=feature_session-name.rs".into(),
            dirty: "hunks=[0] before=12345678".into(),
        },
        OpOutcome::Success {
            after: StateSummary {
                head: "staged (stage 0) before=12345678 after=abcdef12".into(),
                dirty: "staged (stage 0)".into(),
            },
        },
    );
    receipt.ref_moves = Some(Vec::new());
    let original = format!("{receipt:?}");
    let panel = cx.read(|cx| app.read(cx).op_log.clone().unwrap());
    for language in [Lang::En, Lang::Ja] {
        app.update(cx, |app, cx| {
            app.set_lang(language, cx);
            app.bottom_panel_open = true;
            app.bottom_tab = BottomTab::OperationLog;
            app.bottom_panel_height = 360.;
            app.jump_to_commit(&CommitId("0123456789abcdef0123456789abcdef01234567".into()));
            cx.notify();
        });
        panel.update(cx, |panel, cx| {
            *panel = OpLogPanel::from_entries([receipt.clone()].into());
            cx.notify();
        });
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
        cx.read(|cx| {
            let FooterStatus::Idle(message) = &app.read(cx).status_footer else {
                panic!("missing jump guidance");
            };
            user_text(message);
            assert!(message.contains("01234567"));
        });
        user_text(
            &keyboard::recorded_disclosure(0)
                .expect("native disclosure")
                .name,
        );
        let details = oplog_panel::detail_lines(&receipt).join("\n");
        user_text(&details);
        assert!(
            details.contains("feature_session-name.rs"),
            "user path must be verbatim"
        );
        user_text(&oplog_panel::outcome_summary(&receipt));
        panel.update(cx, |panel, cx| panel.copy_entry(0, cx));
        let copied = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap();
        user_text(&copied);
        assert_eq!(
            format!("{receipt:?}"),
            original,
            "presentation cannot rewrite receipt data"
        );
        user_text(&i18n::op_failed(
            i18n::Op::Preflight,
            "repo.index() failed: permission denied",
        ));
    }
    drop(panel);
    unmount(cx, app, window);
}
