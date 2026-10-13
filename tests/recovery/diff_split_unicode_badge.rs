//! #1445: both split columns preserve the unified unsafe-Unicode feedback.
use crate::keyboard_nav::keys;
use crate::macos::{git, unmount};
use crate::recovery_layout::{draw, GlobalSettings};
use gpui::{px, size, AnyWindowHandle, Entity, Modifiers, VisualTestAppContext};
use kagi::ui::{commit_panel::CommitPanelFileRef, e2e, DiffRow, KagiApp};
use kagi_ui_core::{
    i18n::{self, Lang, Msg},
    theme,
};
use std::{cell::RefCell, path::Path, rc::Rc, time::Duration};

pub(crate) fn fixture(old: &str, new: &str) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-q", "-b", "main"]);
    git(temp.path(), &["config", "core.autocrlf", "false"]);
    std::fs::write(temp.path().join("f.txt"), old).unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-qm", "base"]);
    std::fs::write(temp.path().join("f.txt"), new).unwrap();
    temp
}

pub(crate) fn mount_size(
    cx: &mut VisualTestAppContext,
    repo: &Path,
    dimensions: (f32, f32),
    slug: &str,
) -> (Entity<KagiApp>, AnyWindowHandle) {
    let state = e2e::app_state(repo).unwrap();
    let captured = Rc::new(RefCell::new(None));
    let output = captured.clone();
    let window = crate::macos::open_offscreen(
        cx,
        size(px(dimensions.0), px(dimensions.1)),
        move |window, cx| e2e::mount_root(state, window, cx, &output),
    );
    let app = captured.borrow().clone().unwrap();
    app.update(cx, |app, cx| {
        app.set_theme(slug, cx);
        e2e::open_local_panel_no_inputs(app, repo.to_path_buf(), cx);
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.open_main_diff_wip(CommitPanelFileRef::Unstaged { index: 0 }, cx);
    });
    cx.run_until_parked();
    (app, window.into())
}

pub(crate) fn frame(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    dimensions: (f32, f32),
) {
    e2e::diff_content::clear();
    for _ in 0..3 {
        app.update(cx, |app, cx| {
            if let Some(pane) = &app.ui().main_diff {
                pane.update(cx, |_, cx| cx.notify());
            }
            cx.notify();
        });
        cx.run_until_parked();
        draw(cx, win, dimensions);
    }
}

pub(crate) fn clipboard(cx: &mut VisualTestAppContext) -> String {
    cx.read(|cx| {
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default()
    })
}

pub(crate) fn copy_body(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    let body = cx.read(|cx| {
        app.read(cx)
            .ui()
            .main_diff
            .as_ref()
            .unwrap()
            .read(cx)
            .diff_row_bounds_for_e2e(0)
            .0
    });
    cx.simulate_click(
        window,
        body.origin + gpui::point(px(10.), px(10.)),
        Modifiers::none(),
    );
    keys(cx, window, "cmd-a cmd-c");
}

pub(crate) fn reveal(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    dimensions: (f32, f32),
    row: usize,
) {
    let pane = cx.read(|cx| app.read(cx).ui().main_diff.clone().unwrap());
    pane.update(cx, |pane, cx| pane.reveal_diff_row_for_e2e(row, cx));
    frame(cx, app, window, dimensions);
}

pub fn scenario_diff_split_unicode_badge(cx: &mut VisualTestAppContext) {
    let _globals = GlobalSettings::capture();
    let _keys = crate::gui_isolation::SavedKeys::keep(&["diff_split", "lang", "theme"]);
    let temp = fixture(
        "context\u{202e}\u{200b}\nold\u{202e}\u{200b}\nplain old\n",
        "context\u{202e}\u{200b}\nnew\u{202e}\u{200b}\nplain new\n",
    );
    let repo = temp.path().canonicalize().unwrap();
    let backend = kagi_git::Backend::open(&repo).unwrap();
    let raw = backend.unstaged_file_diff(Path::new("f.txt")).unwrap();
    let approvals: Vec<_> = raw.hunks.iter().map(|hunk| hunk.approval()).collect();
    let (app, window) = mount_size(cx, &repo, (1392., 883.), "apple-light");
    // Give native tooltip targets real review width through the normal
    // details/staging visibility states, after the WIP read has completed.
    app.update(cx, |app, cx| {
        app.inspector_visible = false;
        app.ui_mut().unwrap().commit_panel_open = false;
        cx.notify();
    });
    cx.run_until_parked();
    for slug in ["apple-light", "apple-dark"] {
        app.update(cx, |app, cx| app.set_theme(slug, cx));
        for dimensions in [(940., 660.), (1392., 883.)] {
            crate::pr_detail_adaptive::resize(cx, window, dimensions);
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                for lang in [Lang::En, Lang::Ja] {
                    i18n::set_lang(lang);
                    for split in [false, true] {
                        theme::set_diff_split(split);
                        frame(cx, &app, window, dimensions);
                        check_warnings(cx, &app, window, dimensions, split);
                    }
                }
            }
        }
    }
    unmount(cx, app, window);
    assert_eq!(
        backend
            .unstaged_file_diff(Path::new("f.txt"))
            .unwrap()
            .hunks
            .iter()
            .map(|hunk| hunk.approval())
            .collect::<Vec<_>>(),
        approvals,
        "render/copy must not rewrite raw hunk approval"
    );
    eprintln!("[gui-e2e] PASS diff_split_unicode_badge 940/1392 100%/167% EN/JA light/dark");
}

fn check_warnings(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    dimensions: (f32, f32),
    split: bool,
) {
    let rows = cx.read(|cx| {
        app.read(cx)
            .ui()
            .main_diff
            .as_ref()
            .unwrap()
            .read(cx)
            .view
            .rows
            .clone()
    });
    let warnings = e2e::diff_content::warnings();
    let mut expected = 0;
    for (index, row) in rows.iter().enumerate() {
        if let DiffRow::Line { kind, text, .. } = row {
            let sides: &[&str] = if !split {
                &["unified"]
            } else {
                match kind {
                    kagi_domain::diff::DiffLineKind::Context => &["old", "new"],
                    kagi_domain::diff::DiffLineKind::Removed => &["old"],
                    kagi_domain::diff::DiffLineKind::Added => &["new"],
                }
            };
            for side in sides {
                let marker = warnings.get(&(side.to_string(), index));
                if kagi_domain::text_safety::has_unsafe_unicode(text) {
                    expected += 1;
                    let marker = marker.unwrap_or_else(|| panic!("unsafe Unicode feedback must exist: split={split} side={side} row={index}, rendered={warnings:?}"));
                    assert_eq!(marker.label, Msg::UnsafeUnicodeBadge.t());
                    assert_eq!(marker.help, Msg::UnsafeUnicodeTooltip.t());
                    reveal(cx, app, window, dimensions, index);
                    let bounds = e2e::control_bounds(
                        window.window_id(),
                        &format!("diff-unsafe-{side}-{index}"),
                    )
                    .unwrap();
                    assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
                } else {
                    assert!(
                        marker.is_none(),
                        "ordinary content must not carry a warning"
                    );
                }
            }
        }
    }
    assert_eq!(warnings.len(), expected, "no missing/extra warnings");
    let (side, row) = warnings.keys().next().unwrap();
    check_tooltip(cx, app, window, dimensions, split, side, *row);
    copy_body(cx, app, window);
    let expected: String = rows
        .iter()
        .filter_map(|row| {
            if let DiffRow::Line { text, .. } = row {
                Some(format!("{}\n", &text[1..]))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        clipboard(cx),
        expected,
        "copy preserves original Unicode, not warning text"
    );
    assert!(clipboard(cx).contains('\u{202e}') && clipboard(cx).contains('\u{200b}'));
    frame(cx, app, window, dimensions);
}

fn check_tooltip(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    dimensions: (f32, f32),
    split: bool,
    side: &str,
    row: usize,
) {
    reveal(cx, app, window, dimensions, row);
    let badge =
        e2e::control_bounds(window.window_id(), &format!("diff-unsafe-{side}-{row}")).unwrap();
    let viewport = cx.read(|cx| {
        app.read(cx)
            .ui()
            .main_diff
            .as_ref()
            .unwrap()
            .read(cx)
            .diff_row_bounds_for_e2e(0)
            .0
    });
    assert!(badge.left() >= viewport.left() && badge.right() <= viewport.right() && badge.top() >= viewport.top() && badge.bottom() <= viewport.bottom(),
        "hover target must be visible: split={split} side={side} row={row} badge={badge:?} viewport={viewport:?}");
    e2e::diff_content::take_tooltip();
    cx.simulate_mouse_move(window, gpui::point(px(2.), px(2.)), None, Modifiers::none());
    cx.run_until_parked();
    cx.simulate_mouse_move(window, badge.center(), None, Modifiers::none());
    cx.run_until_parked();
    for _ in 0..3 {
        cx.advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
        draw(cx, window, dimensions);
    }
    assert_eq!(e2e::diff_content::take_tooltip().as_deref(), Some(Msg::UnsafeUnicodeTooltip.t()), "hover builds localized warning help: split={split} side={side} row={row} dimensions={dimensions:?} zoom={} lang={:?}", theme::zoom(), i18n::lang());
}
