//! #1374/#1378: renderer attribution after edits and recoverable full identity.
use crate::{
    macos::{build_fixture, git, unmount},
    pr_viewed::paint,
};
use gpui::{px, size, AnyWindowHandle, AppContext, Entity, VisualTestAppContext};
use kagi::ui::{e2e, editor_workspace::EditorWorkspaceView, KagiApp};
use kagi_ui_core::{
    i18n::{self, Lang},
    theme,
};
use kagi_ui_editor::BlameProbe;
use std::{cell::RefCell, path::PathBuf, rc::Rc};

const CONTENT: &str = "fn original_one() {}\nfn original_two() {}\nfn original_three() {}\n";
const AUTHOR: &str = concat!(
    "Alice With A Very Long Identity 日本語 Author ",
    "Repeated full identity remains available in accessibility metadata 日本語 ",
    "No truncation of the author model is allowed even at high zoom 日本語 ",
    "The complete tail of this author identity must survive"
);
const SUBJECT: &str = concat!(
    "Preserve the complete subject 日本語 even when the inline blame cell is much too narrow to display all of this commit identity. ",
    "This long subject deliberately exceeds five hundred characters so neither a shortened model nor a clipped caption can satisfy the reveal oracle. ",
    "The tooltip and accessible name must contain every part of this subject including the later clauses 日本語 and its final distinguishing tail. ",
    "Layout-only middle ellipsis retains the beginning and end visually while the complete identity remains available without pointer interaction. ",
    "Final subject identity tail 日本語."
);

fn wait_loaded(
    cx: &mut VisualTestAppContext,
    view: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        paint(cx, window);
        if cx.read(|cx| {
            let v = view.read(cx);
            v.editor.is_some()
                && v.blame.is_some()
                && !v.file_loading
                && !v.blame_map_pending_for_e2e()
        }) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "real blame/editor backend read did not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn save(
    cx: &mut VisualTestAppContext,
    view: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) {
    view.update(cx, |view, cx| view.save(cx));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        paint(cx, window);
        if cx.read(|cx| !view.read(cx).dirty) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "real editor Save did not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn wait_map(
    cx: &mut VisualTestAppContext,
    view: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        paint(cx, window);
        if cx.read(|cx| !view.read(cx).blame_map_pending_for_e2e()) {
            paint(cx, window);
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "background blame line map did not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn cursor(
    cx: &mut VisualTestAppContext,
    view: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    row: u32,
    text: Option<&str>,
) -> BlameProbe {
    cx.update_window(window, |_, window, cx| {
        let input = view.read(cx).editor.clone().unwrap();
        input.update(cx, |input, cx| {
            if let Some(text) = text {
                input.replace_all(text.to_owned(), window, cx);
            }
            input.set_cursor_position(
                gpui_component::input::Position {
                    line: row,
                    character: 0,
                },
                window,
                cx,
            );
        });
    })
    .unwrap();
    wait_map(cx, view, window);
    cx.read(|cx| view.read(cx).blame_probe.borrow().clone())
        .expect("actual cursor-line blame rendered")
}

fn blame_fixture() -> tempfile::TempDir {
    let fixture = build_fixture();
    let initial = CONTENT
        .replace("original_two", "seed_two")
        .replace("original_three", "seed_three");
    std::fs::write(fixture.path().join("blame.rs"), &initial).unwrap();
    git(fixture.path(), &["add", "blame.rs"]);
    git(
        fixture.path(),
        &[
            "commit",
            "-q",
            "--author",
            &format!("{AUTHOR} <alice@example.test>"),
            "-m",
            SUBJECT,
        ],
    );
    let second = initial.replace("seed_two", "original_two");
    std::fs::write(fixture.path().join("blame.rs"), second).unwrap();
    git(fixture.path(), &["add", "blame.rs"]);
    git(
        fixture.path(),
        &[
            "commit",
            "-q",
            "--author",
            "Bob <bob@example.test>",
            "-m",
            "second line subject",
        ],
    );
    std::fs::write(fixture.path().join("blame.rs"), CONTENT).unwrap();
    git(fixture.path(), &["add", "blame.rs"]);
    git(
        fixture.path(),
        &[
            "commit",
            "-q",
            "--author",
            "Carol <carol@example.test>",
            "-m",
            "third line subject",
        ],
    );
    fixture
}

fn matrix(
    cx: &mut VisualTestAppContext,
    check: impl Fn(&mut VisualTestAppContext, &Entity<EditorWorkspaceView>, AnyWindowHandle, Lang),
) {
    let _saved =
        crate::gui_isolation::SavedKeys::keep(&["theme", "ui_zoom", "lang", "blame_inline"]);
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    let old_theme = theme::theme().slug.to_owned();
    let fixture = blame_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    crate::gui_evidence::fixture(&repo);
    for (width, height) in [(940., 660.), (1392., 883.)] {
        for slug in ["apple-light", "apple-dark"] {
            for lang in [Lang::En, Lang::Ja] {
                for zoom in [1., 1.67] {
                    i18n::set_lang(lang);
                    theme::set_zoom(zoom);
                    let state = e2e::app_state(&repo).unwrap();
                    let captured: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::default();
                    let output = captured.clone();
                    let window = crate::macos::open_offscreen(
                        cx,
                        size(px(width), px(height)),
                        move |window, cx| e2e::mount_root(state, window, cx, &output),
                    )
                    .into();
                    let app = captured.borrow().clone().unwrap();
                    drop(captured);
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        app.update(cx, |app, cx| {
                            app.set_theme(slug, cx);
                            app.open_editor_workspace(cx);
                        });
                        let view = cx
                            .read(|cx| app.read(cx).ui().editor_workspace.clone())
                            .unwrap();
                        view.update(cx, |v, cx| {
                            v.show_blame = true;
                            v.activate_tab(PathBuf::from("blame.rs"), cx);
                        });
                        wait_loaded(cx, &view, window);
                        check(cx, &view, window, lang);
                        drop(view);
                    }));
                    app.update(cx, |app, cx| app.set_theme(&old_theme, cx));
                    cx.update_window(window, |_, window, cx| {
                        window.replace_root(cx, |window, cx| {
                            let empty = cx.new(|_| gpui::Empty);
                            gpui_component::Root::new(empty, window, cx)
                        });
                    })
                    .ok();
                    unmount(cx, app, window);
                    if let Err(error) = result {
                        std::panic::resume_unwind(error);
                    }
                }
            }
        }
    }
}

fn saved_mapping(
    cx: &mut VisualTestAppContext,
    view: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    dirty: &str,
    expected: &str,
) {
    cursor(cx, view, window, 0, Some(dirty));
    save(cx, view, window);
    assert_eq!(
        cursor(cx, view, window, 0, None).label,
        expected,
        "saving does not turn worktree-only text into HEAD attribution"
    );
    view.update(cx, |v, cx| {
        v.open_tabs.push("README.md".into());
        v.activate_tab("README.md".into(), cx);
    });
    wait_loaded(cx, view, window);
    view.update(cx, |v, cx| v.activate_tab("blame.rs".into(), cx));
    wait_loaded(cx, view, window);
    assert_eq!(
        cursor(cx, view, window, 0, None).label,
        expected,
        "saved changed text maps after tab reload"
    );
    assert!(cursor(cx, view, window, 2, None).label.contains(AUTHOR));
    cursor(cx, view, window, 0, Some(CONTENT));
    save(cx, view, window);
    assert!(
        cursor(cx, view, window, 0, None).label.contains(AUTHOR),
        "saved restoration regains HEAD author"
    );
}

pub fn dirty_mapping(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, view, window, lang| {
        let dirty = format!("// inserted one\n// inserted two\n{CONTENT}");
        let expected = match lang {
            Lang::En => "Uncommitted",
            Lang::Ja => "未 commit",
        };
        let first = cursor(cx, view, window, 0, Some(&dirty));
        assert_eq!(
            first.label, expected,
            "inserted row 1 must not inherit HEAD row 1 author"
        );
        assert_eq!(
            cursor(cx, view, window, 1, None).label,
            expected,
            "inserted row 2 is uncommitted"
        );
        let original = cursor(cx, view, window, 2, None);
        assert!(
            original.label.contains(AUTHOR),
            "original line 1 keeps its author at row 3: {original:?}"
        );
        assert!(
            cursor(cx, view, window, 3, None).label.contains("Bob"),
            "original row 2 keeps distinct author at row 4"
        );
        assert!(
            cursor(cx, view, window, 4, None).label.contains("Carol"),
            "original row 3 keeps distinct author at row 5"
        );
        // Replay the real pre-edit backend response through its production
        // acceptance boundary: mapping must use current text, not request text.
        view.update(cx, |v, cx| {
            let result = v.blame.clone();
            v.seed_blame(
                v.blame_request_for_e2e(),
                std::path::Path::new("blame.rs"),
                result,
                Some(CONTENT.to_owned()),
                cx,
            );
        });
        assert_eq!(
            cursor(cx, view, window, 0, None).label,
            expected,
            "late HEAD response maps against the edited buffer"
        );
        assert!(cursor(cx, view, window, 2, None).label.contains(AUTHOR));
        let modified = dirty.replace("fn original_two() {}", "fn changed_two() {}");
        assert_eq!(
            cursor(cx, view, window, 3, Some(&modified)).label,
            expected,
            "modified line is uncommitted"
        );
        let deleted = "fn original_three() {}\n";
        assert!(
            cursor(cx, view, window, 0, Some(deleted))
                .label
                .contains("Carol"),
            "deletion maps retained row to HEAD row 3"
        );
        assert!(
            cursor(cx, view, window, 0, Some(CONTENT))
                .label
                .contains(AUTHOR),
            "undo/restored buffer regains attribution"
        );
        saved_mapping(cx, view, window, &dirty, expected);
    });
}

pub fn full_identity(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, view, window, _| {
        let probe = cursor(cx, view, window, 0, None);
        let identity = probe
            .aria_label
            .expect("blame cell exposes complete accessible identity");
        assert!(identity.contains(AUTHOR), "full author preserved");
        assert!(identity.contains(SUBJECT), "full subject preserved");
        assert_eq!(
            probe.tooltip.as_deref(),
            Some(identity.as_str()),
            "tooltip and AX receive identical full author/date/subject, not ellipsized text"
        );
        assert_eq!(probe.label, identity, "ellipsis is layout-only");
        assert!(
            probe.bounds.size.width > px(0.)
                && probe.bounds.size.width <= theme::scaled_px(560.) + px(1.),
            "full metadata retains existing bounded annotation geometry: {:?}",
            probe.bounds
        );
        cx.simulate_mouse_move(window, probe.bounds.center(), None, gpui::Modifiers::none());
        cx.advance_clock(std::time::Duration::from_secs(1));
        paint(cx, window);
        let shown = cx.read(|cx| {
            view.read(cx)
                .blame_probe
                .borrow()
                .as_ref()
                .and_then(|p| p.tooltip_shown.clone())
        });
        assert_eq!(
            shown.as_deref(),
            Some(identity.as_str()),
            "invoked native Tooltip builder receives complete identity"
        );
        let next = cursor(cx, view, window, 1, None);
        assert!(next.aria_label.as_ref().unwrap().contains("Bob"));
        assert!(next
            .tooltip
            .as_ref()
            .unwrap()
            .contains("second line subject"));
        assert!(
            !next.label.contains(AUTHOR),
            "cursor movement updates full identity"
        );
    });
}

pub fn crlf_identity(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, view, window, _| {
        let repo = cx.read(|cx| view.read(cx).repo_path.clone());
        std::fs::write(repo.join(".gitattributes"), "blame.rs text eol=crlf\n").unwrap();
        std::fs::write(repo.join("blame.rs"), CONTENT.replace('\n', "\r\n")).unwrap();
        view.update(cx, |v, cx| {
            v.activate_tab("README.md".into(), cx);
        });
        wait_loaded(cx, view, window);
        view.update(cx, |v, cx| v.activate_tab("blame.rs".into(), cx));
        wait_loaded(cx, view, window);
        assert!(
            cursor(cx, view, window, 0, None).label.contains(AUTHOR),
            "clean CRLF checkout keeps HEAD line 1 identity"
        );
        assert!(
            cursor(cx, view, window, 1, None).label.contains("Bob"),
            "clean CRLF checkout keeps HEAD line 2 identity"
        );
        let changed = CONTENT
            .replace("original_two", "changed_two")
            .replace('\n', "\r\n");
        let probe = cursor(cx, view, window, 1, Some(&changed));
        assert!(
            !probe.label.contains("Bob"),
            "changed CRLF row has no old author"
        );
        std::fs::write(repo.join("blame.rs"), CONTENT).unwrap();
    });
}

pub fn hidden_mapping(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, view, window, lang| {
        let count = cx.read(|cx| view.read(cx).blame_map_computations_for_e2e());
        view.update(cx, |view, cx| view.toggle_blame(cx));
        let dirty = format!("// inserted\n{CONTENT}");
        cx.update_window(window, |_, window, cx| {
            let input = view.read(cx).editor.clone().unwrap();
            input.update(cx, |input, cx| {
                input.replace_all("// preliminary edit\n", window, cx);
                input.replace_all(dirty.clone(), window, cx);
            });
        })
        .unwrap();
        paint(cx, window);
        cx.read(|cx| {
            let view = view.read(cx);
            assert!(!view.show_blame);
            assert!(
                !view.blame_map_pending_for_e2e(),
                "hidden blame schedules no map task"
            );
            assert_eq!(
                view.blame_map_computations_for_e2e(),
                count,
                "hidden changes do not recompute mapping"
            );
        });
        view.update(cx, |view, cx| view.toggle_blame(cx));
        let expected = match lang {
            Lang::En => "Uncommitted",
            Lang::Ja => "未 commit",
        };
        assert_eq!(cursor(cx, view, window, 0, None).label, expected);
        assert!(
            cursor(cx, view, window, 1, None).label.contains(AUTHOR),
            "toggle on maps latest hidden edits"
        );
        cx.read(|cx| assert_eq!(view.read(cx).blame_map_computations_for_e2e(), count + 1));
    });
}
