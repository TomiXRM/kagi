//! #1463 / #1467: Finder-inspired, vertically balanced changed-file slots.
use crate::macos::{build_fixture, git, unmount};
use crate::recovery_layout::{draw, GlobalSettings};
use gpui::{px, size, AnyWindowHandle, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_ui_core::{i18n, theme};
use std::{cell::RefCell, rc::Rc};

pub fn scenario(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let fixture = build_fixture();
    // Do not let a machine-global template consume the geometry fixture's lists.
    git(fixture.path(), &["config", "commit.template", "/dev/null"]);
    for n in 0..213 {
        let path = fixture
            .path()
            .join(format!("density/group-{:03}/file-{n:03}.txt", n / 3));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("staged {n}\n")).unwrap();
    }
    git(fixture.path(), &["add", "density"]);
    for n in 0..213 {
        std::fs::write(
            fixture
                .path()
                .join(format!("density/group-{:03}/file-{n:03}.txt", n / 3)),
            format!("unstaged {n}\n"),
        )
        .unwrap();
    }
    for (width, height) in [(1392., 883.), (940., 660.)] {
        for zoom in [1., 1.67] {
            for lang in [i18n::Lang::En, i18n::Lang::Ja] {
                for tree in [false, true] {
                    i18n::set_lang(lang);
                    theme::set_zoom(zoom);
                    let state = e2e::app_state(fixture.path()).unwrap();
                    let captured = Rc::new(RefCell::new(None));
                    let output = captured.clone();
                    let window = crate::macos::open_offscreen(
                        cx,
                        size(px(width), px(height)),
                        move |window, cx| e2e::mount_root(state, window, cx, &output),
                    );
                    let app = captured.borrow().clone().unwrap();
                    cx.update_window(window.into(), |_, window, cx| {
                        app.update(cx, |app, cx| app.open_commit_panel(window, cx));
                        let panel = app.read(cx).ui().commit_panel.clone().unwrap();
                        panel.update(cx, |panel, cx| {
                            panel.state.tree_view = tree;
                            cx.notify();
                        });
                    })
                    .unwrap();
                    for _ in 0..4 {
                        cx.run_until_parked();
                        draw(cx, window.into(), (width, height));
                    }
                    for staged in [false, true] {
                        check(window.into(), staged, tree, zoom);
                    }
                    unmount(cx, app, window.into());
                }
            }
        }
    }
    eprintln!("[gui-e2e] PASS commit_panel_density");
}

fn check(window: AnyWindowHandle, staged: bool, tree: bool, zoom: f32) {
    let section = if staged { "st" } else { "us" };
    let bounds = |id: &str| {
        e2e::control_bounds(window.window_id(), id).unwrap_or_else(|| panic!("missing {id}"))
    };
    // scaled_px includes Kagi's shared 0.9 density; the button floor is logical.
    let expected = theme::scaled_px(22.).max(px(20.));
    let header = bounds(if staged {
        "cp-staged-header"
    } else {
        "cp-unstaged-header"
    });
    let viewport = bounds(&format!("cp-{section}-viewport"));
    assert!(viewport.top() - header.bottom() <= px(1.));
    if staged {
        let heading = bounds("cp-staged-heading");
        assert!(
            heading.top() - header.top() <= px(1.5),
            "compact header top padding"
        );
        assert!(
            header.bottom() - heading.bottom() <= px(1.5),
            "compact header bottom padding"
        );
    }
    if tree {
        // The narrow/high-zoom viewport may show only the root directory.
        // A fresh window avoids satisfying these checks with stale flat-row probes.
        let dir_id = format!("cp-{section}-dir-density");
        let label_id = format!("cp-{section}-dir-label-density");
        let directory = bounds(&dir_id);
        let label = bounds(&label_id);
        assert!((directory.size.height - expected).abs() <= px(1.));
        let above = label.top() - directory.top();
        let below = directory.bottom() - label.bottom();
        assert!(
            (above - below).abs() <= px(1.),
            "directory label must be vertically centered: above={above:?}, below={below:?}"
        );
        let paint = e2e::control_paint(window.window_id(), &label_id).unwrap();
        assert!((paint.font_size - theme::scaled_px(theme::FILE_TREE_FONT_PX)).abs() <= px(0.1));
        eprintln!("commit-panel density {section}/tree@{zoom}: directory={directory:?}, padding={above:?}/{below:?}, font={:?}", paint.font_size);
    } else {
        let first = bounds(&format!("cp-{section}-density-file-0"));
        let second = bounds(&format!("cp-{section}-density-file-1"));
        let slot = second.top() - first.top();
        eprintln!("commit-panel density {section}/flat@{zoom}: slot={slot:?}, row={first:?}");
        assert!((slot - expected).abs() <= px(1.), "file slot must be scaled 22 reference px with 20 logical px floor; measured old slot={slot:?}");
        // Inside probes exclude RowList's 2px focus border on each side.
        assert!((first.size.height + px(4.) - expected).abs() <= px(1.));
        let action = bounds(if staged {
            "cp-row-unstage"
        } else {
            "cp-row-stage"
        });
        // The button probe excludes its two 1px borders, not its clickable frame.
        assert!(
            action.size.height + px(2.) >= px(20.),
            "Stage/Unstage tap target: {action:?}"
        );
        assert!(
            first.top() - viewport.top() <= px(3.),
            "first flat row starts immediately within border"
        );
    }
}
