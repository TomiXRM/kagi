//! Disabled toolbar buttons are still reachable and explain themselves (#972).
//! F19 is a harmless focus probe: a toolbar button records its id only if it
//! actually receives the key after GPUI's Tab traversal.

use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::i18n::Msg;
use kagi::ui::{e2e, FooterStatus};

pub fn scenario_toolbar_keyboard_reasons(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);
    cx.run_until_parked();
    e2e::clear_toolbar_unavailable();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
        app.read(cx).root_focus.clone().unwrap().focus(window, cx);
    })
    .unwrap();

    let disabled = [
        ("tb-pull", Msg::PullNoUpstream.t(), "enter"),
        ("tb-push", Msg::PushNoRemote.t(), "space"),
        ("tb-stash", Msg::StashClean.t(), "enter"),
        ("tb-pop", Msg::PopEmpty.t(), "space"),
        ("tb-undo", Msg::NothingToUndo.t(), "enter"),
        ("tb-redo", Msg::NothingToRedo.t(), "space"),
    ];
    let expected = [
        "tb-refresh",
        "tb-pull",
        "tb-push",
        "tb-branch",
        "tb-stash",
        "tb-pop",
        "tb-undo",
        "tb-redo",
        "tb-terminal",
        "tb-graph-mode",
        "tb-pr-mode",
        "tb-editor-ws",
        "tb-ecosystem",
        "tb-settings",
    ];
    let mut reached = Vec::new();
    for _ in 0..120 {
        cx.update_window(window, |_, window, cx| {
            window.focus_next(cx);
            window.draw(cx).clear();
        })
        .unwrap();
        let _ = e2e::take_focus_probe();
        cx.simulate_keystrokes(window, "f19");
        let Some(id) = e2e::take_focus_probe() else {
            continue;
        };
        reached.push(id);
        if let Some((_, reason, key)) = disabled.iter().find(|(button, _, _)| *button == id) {
            assert_eq!(
                e2e::toolbar_unavailable(id),
                Some(true),
                "{id}: AX disabled"
            );
            assert_eq!(
                e2e::toolbar_description(id),
                Some(*reason),
                "{id}: AX description"
            );
            keys(cx, window, key);
            cx.read(|cx| {
                assert!(
                    matches!(&app.read(cx).status_footer, FooterStatus::Idle(text) if text.as_ref() == *reason),
                    "{id}: {key} must show the pointer-click reason in the footer; got {:?}",
                    app.read(cx).status_footer
                );
                assert!(
                    !e2e::active_modal_present(app.read(cx)),
                    "{id}: {key} must not fall through to the root's modal action"
                );
            });
        }
        if id == "tb-settings" {
            break;
        }
    }
    let expected: Vec<_> = expected
        .into_iter()
        .filter(|id| *id != "tb-pr-mode" || e2e::toolbar_unavailable(id).is_some())
        .collect();
    assert_eq!(
        reached, expected,
        "Tab must reach each toolbar button once in visual order"
    );
    assert_eq!(
        repo_fingerprint(&repo),
        before,
        "disabled key press mutated repository"
    );

    // #975 review: while another operation holds the latch, `render` turns
    // Stash and Pop off whatever the tree and the stash list say, so their
    // reason is the busy one — never "clean" / "empty" for a dirty tree or
    // an existing stash.
    app.update(cx, |app, cx| {
        app.planning = Some("toolbar-busy-probe");
        cx.notify();
    });
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    assert!(
        cx.read(|cx| e2e::op_latched(app.read(cx))),
        "precondition: latched"
    );
    for (id, reason) in [
        ("tb-stash", Msg::StashBusy.t()),
        ("tb-pop", Msg::PopBusy.t()),
    ] {
        assert_eq!(
            e2e::toolbar_description(id),
            Some(reason),
            "toolbar-busy-reason: {id} must give the busy reason while latched"
        );
    }
    app.update(cx, |app, _| app.planning = None);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS toolbar_keyboard_reasons");
}
