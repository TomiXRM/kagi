//! #1316: shrinking chrome gives Git commands priority over secondary controls.
use crate::macos::{build_fixture, unmount};
use crate::recovery_layout::contained;
use crate::toolbar_overflow::{frame, resize_overflow_window, ToolbarRestore};
use gpui::{px, size, AnyWindowHandle, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_domain::update::{Asset, ReleaseInfo, UpdatePlan, Version};
use kagi_ui_core::{i18n, settings, theme};
use std::{cell::RefCell, rc::Rc};

const IDS: [&str; 14] = [
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
    "tb-update",
];
const ORDER: [usize; 14] = [13, 12, 11, 10, 9, 7, 6, 5, 4, 3, 2, 1, 0, 8];

pub fn scenario_toolbar_overflow_priority(cx: &mut VisualTestAppContext) {
    let _restore = ToolbarRestore::capture(cx);
    let fixture = build_fixture();
    assert!(
        kagi_git::github::gh_available(),
        "PR mode must participate in priority coverage"
    );
    for slug in ["apple-light", "apple-dark"] {
        for language in ["en", "ja"] {
            std::env::set_var("KAGI_LANG", language);
            i18n::init_lang();
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                settings::set_toolbar_display(settings::ToolbarDisplay::IconOnly);
                let state = e2e::app_state(fixture.path()).unwrap();
                let captured: Rc<RefCell<Option<gpui::Entity<KagiApp>>>> = Rc::default();
                let output = captured.clone();
                let initial_width = 1600. * zoom;
                let win: AnyWindowHandle = crate::macos::open_offscreen(
                    cx,
                    size(px(initial_width), px(660.)),
                    move |window, cx| e2e::mount_root(state, window, cx, &output),
                )
                .into();
                let app = captured.borrow().clone().unwrap();
                app.update(cx, |app, cx| {
                    app.set_theme(slug, cx);
                    offer_update(app);
                    cx.notify();
                });
                for _ in 0..8 {
                    frame(cx, win);
                }
                let mut width = initial_width;
                let mut history = Vec::new();
                let mut previous = 0;
                let mut selected_only_reclaims_git_width = false;
                loop {
                    let hidden = hidden_at_width(cx, win, width);
                    let count = hidden.count_ones() as usize;
                    let expected = ORDER[..count]
                        .iter()
                        .fold(0, |mask, index| mask | (1 << index));
                    assert_eq!(hidden, expected,
                        "{slug}/{language}/{zoom}/{width}: Update, Settings and non-selected modes must hide before Terminal/Redo or other Git commands; selected Graph hides after Pull");
                    assert_eq!(
                        hidden & previous,
                        previous,
                        "shrinking must not restore commands"
                    );
                    if count == 11 {
                        // Only Pull, Push and selected Graph remain. The old
                        // symmetric 100px floor would already have hidden Push.
                        let repo = e2e::control_bounds(win.window_id(), "tb-repo-actions").unwrap();
                        let graph = e2e::control_bounds(win.window_id(), "tb-graph-mode").unwrap();
                        let overflow = e2e::control_bounds(win.window_id(), "tb-overflow").unwrap();
                        let workspace = f32::from(graph.size.width + overflow.size.width);
                        let old_required =
                            f32::from(repo.size.width) + 2. * 100. * zoom + 24. * zoom + 80.
                                - 12. * zoom
                                + 28. * zoom;
                        selected_only_reclaims_git_width |=
                            workspace < 100. * zoom && width.round() < old_required - 1.;
                    }
                    if history.is_empty() || hidden != previous {
                        history.push((width, hidden));
                    }
                    previous = hidden;
                    if count == IDS.len() {
                        break;
                    }
                    width -= 16. * zoom;
                    assert!(width >= 160., "must reach fully overflowed toolbar");
                }
                assert!(selected_only_reclaims_git_width,
                    "{slug}/{language}/{zoom}: removing the selected-only 100px floor must keep Pull/Push visible below their old fit threshold");
                for count in [0, 1, 2, 11, 12, 13, 14] {
                    assert!(history.iter().any(|(_, mask)| mask.count_ones() == count),
                        "must observe Update, Settings, Pull/Push, Pull, selected mode and full overflow stages");
                }
                for (width, expected) in history.into_iter().rev() {
                    assert_eq!(hidden_at_width(cx, win, width), expected,
                        "{slug}/{language}/{zoom}/{width}: widening restores the exact reverse priority");
                }
                unmount(cx, app, win);
            }
        }
    }
}

fn hidden_at_width(cx: &mut VisualTestAppContext, win: AnyWindowHandle, width: f32) -> u16 {
    resize_overflow_window(cx, win, width);
    for _ in 0..3 {
        frame(cx, win);
    }
    for id in IDS {
        e2e::clear_control_bounds(win.window_id(), id);
    }
    frame(cx, win);
    let row = e2e::control_bounds(win.window_id(), "toolbar-row").unwrap();
    let identity = e2e::control_bounds(win.window_id(), "toolbar-identity").unwrap();
    contained(row, identity, "readable repository/branch identity");
    assert!(
        identity.size.width > px(30.),
        "identity must remain readable"
    );
    let hidden = IDS.iter().enumerate().fold(0, |mask, (index, id)| {
        if let Some(bounds) =
            e2e::control_bounds(win.window_id(), id).filter(|b| b.size.width > px(0.))
        {
            contained(row, bounds, id);
            assert!(
                bounds.left() >= identity.right(),
                "{id} must not overlap identity"
            );
            mask
        } else {
            mask | (1 << index)
        }
    });
    if hidden != 0 {
        contained(
            row,
            e2e::control_bounds(win.window_id(), "tb-overflow").unwrap(),
            "overflow",
        );
    }
    hidden
}

fn offer_update(app: &mut KagiApp) {
    let latest = Version::parse("1.0.1").unwrap();
    let asset = Asset {
        name: "Kagi-test.dmg".into(),
        url: "https://example.invalid/Kagi-test.dmg".into(),
        size: 1,
    };
    app.update_available = Some((
        UpdatePlan {
            current: Version::parse("1.0.0").unwrap(),
            latest: latest.clone(),
            tag: "v1.0.1".into(),
            notes: String::new(),
            asset: asset.clone(),
        },
        ReleaseInfo {
            tag: "v1.0.1".into(),
            version: latest,
            notes: String::new(),
            assets: vec![asset],
        },
    ));
}
