//! #495: WIP / compare / File History diffs show their text first, highlight
//! off the UI thread once per (rows, theme), and a late highlight or read never
//! lands on a newer surface.
//!
//! Frames are drawn explicitly (`frame`) and the dispatcher is parked
//! separately, so a highlight request can be left outstanding while the
//! surface changes under it.
use crate::evidence_support::deferred;
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{AnyWindowHandle, App, Entity, VisualTestAppContext};
use kagi::ui::commit_panel::CommitPanelFileRef;
use kagi::ui::diff_view::highlight::e2e;
use kagi::ui::diff_view::{DiffRow, MainDiffSource, MainDiffView};
use kagi::ui::{e2e as seam, theme, KagiApp};
use std::cell::RefCell;
use std::rc::Rc;

const MAIN_RS: &str = "fn main() {\n    let answer = 42;\n    println!(\"{answer}\");\n}\n";
const LIB_RS: &str = "pub struct Point {\n    pub x: i32,\n}\n";

/// The base fixture plus two committed Rust files, both edited in the
/// working tree.
fn rust_fixture() -> tempfile::TempDir {
    let fixture = build_fixture();
    let p = fixture.path();
    std::fs::write(p.join("lib.rs"), LIB_RS).unwrap();
    std::fs::write(p.join("main.rs"), MAIN_RS).unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-q", "-m", "rust files"]);
    std::fs::write(p.join("lib.rs"), format!("{LIB_RS}pub fn edited() {{}}\n")).unwrap();
    std::fs::write(p.join("main.rs"), MAIN_RS.replace("42", "43")).unwrap();
    fixture
}

/// Draw one frame without running any task it spawns.
fn frame(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
}

fn settle(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    frame(cx, window);
    cx.run_until_parked();
}

fn main_view(cx: &mut VisualTestAppContext, kagi: &Entity<KagiApp>) -> Option<MainDiffView> {
    cx.read(|app| {
        kagi.read(app)
            .ui()
            .main_diff
            .as_ref()
            .map(|pane| pane.read(app).view.clone())
    })
}

fn has_spans(view: &MainDiffView) -> bool {
    view.rows
        .iter()
        .any(|row| matches!(row, DiffRow::Line { highlights, .. } if !highlights.is_empty()))
}

fn rows_id(view: &MainDiffView) -> usize {
    std::sync::Arc::as_ptr(&view.rows) as usize
}

fn counts() -> (usize, usize, usize) {
    (
        e2e::highlight_runs(),
        e2e::stale_highlights(),
        e2e::split_projections(),
    )
}

fn other_theme() -> &'static str {
    if theme::theme().slug == "dracula" {
        "tokyo-night"
    } else {
        "dracula"
    }
}

/// The key of the installed theme `slug` names — what a view records as the
/// theme its spans were computed with.
fn key_of(slug: &str) -> theme::ThemeKey {
    theme::theme_by_slug(slug).expect("built-in theme").key()
}

fn open_wip(cx: &mut VisualTestAppContext, kagi: &Entity<KagiApp>, index: usize) {
    kagi.update(cx, |app, cx| {
        app.open_main_diff_wip(CommitPanelFileRef::Unstaged { index }, cx)
    });
}

/// Every state `read` reports when `entity` notifies, kept alive with it.
type Seen = (
    Rc<RefCell<Vec<Option<theme::ThemeKey>>>>,
    gpui::Subscription,
);

fn record<T: 'static>(
    cx: &mut VisualTestAppContext,
    entity: &Entity<T>,
    read: impl Fn(&T, &App) -> Option<Option<theme::ThemeKey>> + 'static,
) -> Seen {
    let seen: Rc<RefCell<Vec<Option<theme::ThemeKey>>>> = Rc::default();
    let log = seen.clone();
    let subscription = cx.update(|cx| {
        cx.observe(entity, move |entity, cx| {
            if let Some(state) = read(entity.read(cx), cx) {
                log.borrow_mut().push(state);
            }
        })
    });
    (seen, subscription)
}

/// The diff was on screen as text before any span reached it.
fn assert_first_text(seen: &Seen, surface: &str) {
    let seen = seen.0.borrow();
    assert_eq!(
        seen.first(),
        Some(&None),
        "{surface}: first shown state {seen:?} was not unhighlighted text"
    );
}

/// Text first, one highlight and one projection per (rows, theme): repeated
/// frames, a reload that re-reads the same text, and a theme switch.
pub fn scenario_diff_highlight_once(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["diff_split"]);
    let split_before = theme::diff_split();
    theme::set_diff_split(true);
    let fixture = rust_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (kagi, window) = mount(cx, &repo);
    let panel_repo = repo.clone();
    kagi.update(cx, |app, cx| {
        seam::open_local_panel_no_inputs(app, panel_repo, cx)
    });
    cx.run_until_parked();

    // WIP: the read lands as text; the spans follow in a later update.
    let before = counts();
    let seen = record(cx, &kagi, |app, cx| {
        let view = &app.ui().main_diff.as_ref()?.read(cx).view;
        matches!(view.source, MainDiffSource::Unstaged { .. }).then_some(view.highlighted)
    });
    open_wip(cx, &kagi, 0);
    cx.run_until_parked();
    assert_first_text(&seen, "WIP");
    settle(cx, window);
    let lit = main_view(cx, &kagi).expect("the WIP diff opens");
    assert_eq!(lit.lang, Some("rust"));
    assert_eq!(lit.highlighted, Some(theme::theme().key()));
    assert!(has_spans(&lit), "the spans landed on the WIP diff");
    assert_eq!(counts().0, before.0 + 1, "exactly one highlight");
    settle(cx, window);

    // Unchanged input: neither frames nor a same-text re-read repeat work.
    let steady = counts();
    let rows = rows_id(&main_view(cx, &kagi).unwrap());
    for _ in 0..20 {
        settle(cx, window);
    }
    assert_eq!(counts(), steady, "20 frames re-highlighted or re-projected");
    std::fs::write(repo.join("unrelated.txt"), "x\n").unwrap();
    git(&repo, &["add", "unrelated.txt"]);
    git(&repo, &["commit", "-q", "-m", "unrelated"]);
    kagi.update(cx, |app, cx| app.reload_external(cx));
    cx.run_until_parked();
    for _ in 0..3 {
        settle(cx, window);
    }
    let reread = main_view(cx, &kagi).expect("the reload keeps the WIP diff");
    assert_eq!(rows_id(&reread), rows, "the same text keeps the same rows");
    assert_eq!(counts(), steady, "a same-text re-read repeated work");

    // A theme switch highlights once more, under the new theme.
    let target = other_theme();
    let (theme_before, saved_theme) = (
        theme::theme().slug.to_string(),
        kagi::ui::settings::read_setting("theme"),
    );
    kagi.update(cx, |app, cx| app.set_theme(target, cx));
    settle(cx, window);
    settle(cx, window);
    let themed = main_view(cx, &kagi).unwrap();
    assert_eq!(themed.highlighted, Some(key_of(target)));
    assert_eq!(counts().0, steady.0 + 1, "one highlight per theme");

    // Compare: the same text-first path.
    kagi.update(cx, |app, _| app.close_main_diff());
    let head = kagi.update(cx, |app, cx| {
        let head = app.view().rows[0].id.clone();
        app.open_compare_with_working_tree(head.clone(), Some(cx));
        head
    });
    cx.run_until_parked();
    let seen = record(cx, &kagi, |app, cx| {
        let view = &app.ui().main_diff.as_ref()?.read(cx).view;
        matches!(view.source, MainDiffSource::Compare { .. }).then_some(view.highlighted)
    });
    kagi.update(cx, |app, cx| app.open_main_diff_compare(0, cx));
    cx.run_until_parked();
    assert_first_text(&seen, "compare");
    settle(cx, window);
    let compare = main_view(cx, &kagi).expect("the compare diff opens");
    assert!(
        matches!(&compare.source, MainDiffSource::Compare { base, .. } if *base == head),
        "the compare file diff is shown"
    );
    assert_eq!(compare.highlighted, Some(key_of(target)));

    // File History: the embedded diff, same pipeline.
    kagi.update(cx, |app, cx| {
        app.close_main_diff();
        app.open_file_history("main.rs".into(), None, cx)
    });
    let pane = cx
        .read(|app| {
            let view = kagi.read(app).ui().file_history.clone()?;
            let pane = view.read(app).diff_pane.clone();
            pane.downcast::<kagi::ui::file_history::FhDiffPane>().ok()
        })
        .expect("File History has a diff pane");
    let seen = record(cx, &pane, |pane, _| {
        pane.diff.as_ref().map(|d| d.highlighted)
    });
    cx.run_until_parked();
    assert_first_text(&seen, "File History");
    settle(cx, window);
    let fh = cx
        .read(|app| pane.read(app).diff.clone())
        .expect("File History shows the selected entry's diff");
    assert_eq!(fh.highlighted, Some(key_of(target)));
    assert!(has_spans(&fh));

    theme::set_diff_split(split_before);
    // The scenarios that follow render under the theme they found (#516).
    kagi.update(cx, |app, cx| app.set_theme(&theme_before, cx));
    kagi::ui::settings::write_setting("theme", saved_theme.as_deref());
    unmount(cx, kagi, window);
    eprintln!("[gui-e2e] PASS diff_highlight_once");
}

/// A late highlight or read never lands on the surface that replaced it: a
/// newer file, a newer theme, a newer open, or a close.
pub fn scenario_diff_highlight_stale(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    let fixture = rust_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (kagi, window) = mount(cx, &repo);
    let panel_repo = repo.clone();
    kagi.update(cx, |app, cx| {
        seam::open_local_panel_no_inputs(app, panel_repo, cx)
    });
    cx.run_until_parked();
    // Select HEAD and let the render load its changed files.
    kagi.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    settle(cx, window);
    // #829: a cache miss is read off the UI thread. Read both files once so
    // the opens below are cache hits that install synchronously, which the
    // highlight timing below relies on.
    for index in [0, 1] {
        kagi.update(cx, |app, cx| app.open_main_diff_commit(index, cx));
        cx.run_until_parked();
    }
    let commit_file = |cx: &mut VisualTestAppContext, index: usize| {
        kagi.update(cx, |app, cx| app.open_main_diff_commit(index, cx));
        main_view(cx, &kagi).expect("the commit file opens")
    };

    // Another file replaces the one whose highlight is out.
    let first = commit_file(cx, 0);
    frame(cx, window);
    let second = commit_file(cx, 1);
    assert_ne!(first.title, second.title);
    frame(cx, window);
    let before = counts();
    cx.run_until_parked();
    let shown = main_view(cx, &kagi).unwrap();
    assert_eq!(shown.title, second.title, "the newer file stays on screen");
    assert!(has_spans(&shown), "the newer file gets its own spans");
    assert_eq!(
        counts().1,
        before.1 + 1,
        "the older file's spans were dropped"
    );

    // The theme changes while a highlight is out. The active theme moves
    // without a redraw, so the old request is the only one out when it lands:
    // it must be dropped, not shown. The next frame asks under the new theme.
    let (old, old_key) = (theme::theme().slug.to_string(), theme::theme().key());
    let target = other_theme();
    commit_file(cx, 0);
    frame(cx, window);
    let pane = cx
        .read(|app| kagi.read(app).ui().main_diff.clone())
        .unwrap();
    let seen = record(cx, &pane, |pane, _| Some(pane.view.highlighted));
    assert!(theme::set_active(target));
    let before = counts();
    cx.run_until_parked();
    assert!(
        !seen.0.borrow().contains(&Some(old_key)),
        "{old}'s late spans were shown after the switch: {:?}",
        seen.0.borrow()
    );
    drop((seen, pane));
    assert_eq!(counts().1, before.1 + 1, "{old}'s late spans were dropped");
    settle(cx, window);
    let shown = main_view(cx, &kagi).unwrap();
    assert_eq!(
        shown.highlighted,
        Some(key_of(target)),
        "{target}'s spans land"
    );
    assert!(theme::set_active(&old));

    // A read that is still out when a newer diff is installed, or the pane is
    // closed, never lands.
    open_wip(cx, &kagi, 0);
    commit_file(cx, 1);
    cx.run_until_parked();
    let shown = main_view(cx, &kagi).unwrap();
    assert!(
        matches!(shown.source, MainDiffSource::Commit { file_index: 1, .. }),
        "the superseded WIP read replaced the commit diff"
    );
    open_wip(cx, &kagi, 0);
    kagi.update(cx, |app, _| app.close_main_diff());
    cx.run_until_parked();
    assert!(
        main_view(cx, &kagi).is_none(),
        "a read that lands after the close reopened the diff"
    );

    unmount(cx, kagi, window);
    eprintln!("[gui-e2e] PASS diff_highlight_stale");
}

/// #829: a commit file's diff on a content-cache miss is read off the UI
/// thread: the call returns before any rows change, and a read superseded by
/// another open (here: another commit's file) never lands or fills the cache.
pub fn scenario_commit_diff_off_thread(cx: &mut VisualTestAppContext) {
    let fixture = rust_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (kagi, window) = mount(cx, &repo);
    kagi.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    settle(cx, window);

    // A miss installs nothing during the call; the text lands with the read.
    kagi.update(cx, |app, cx| {
        app.open_main_diff_commit(0, cx);
        assert!(
            app.ui().main_diff.is_none(),
            "a cache-miss commit diff was read on the UI thread"
        );
    });
    cx.run_until_parked();
    let first = main_view(cx, &kagi).expect("the commit file lands");
    assert!(matches!(
        first.source,
        MainDiffSource::Commit {
            row_index: 0,
            file_index: 0,
            ..
        }
    ));

    // With a diff open, a miss leaves the shown rows alone until it lands.
    kagi.update(cx, |app, cx| app.open_main_diff_commit(1, cx));
    let during = main_view(cx, &kagi).unwrap();
    assert_eq!(
        rows_id(&during),
        rows_id(&first),
        "the shown rows changed before the read landed"
    );
    cx.run_until_parked();
    let second = main_view(cx, &kagi).unwrap();
    assert_ne!(second.title, first.title, "the second file landed");

    // Another commit's file is still being read when the selection moves
    // back and a cached file is opened: the older read must not land.
    kagi.update(cx, |app, cx| {
        app.select(1);
        cx.notify();
    });
    settle(cx, window);
    kagi.update(cx, |app, cx| app.open_main_diff_commit(0, cx));
    kagi.update(cx, |app, cx| {
        app.select(0);
        app.open_main_diff_commit(0, cx);
    });
    cx.run_until_parked();
    let shown = main_view(cx, &kagi).unwrap();
    assert!(
        matches!(
            shown.source,
            MainDiffSource::Commit {
                row_index: 0,
                file_index: 0,
                ..
            }
        ),
        "the superseded read of the other commit landed"
    );
    assert_eq!(shown.title, first.title);
    assert!(
        cx.read(|app| !kagi
            .read(app)
            .ui()
            .diff_caches
            .file_content
            .contains_key(&(1, 0))),
        "the superseded read filled the cache"
    );

    // A reload that renumbers the rows lands while a miss is still out: the
    // read is held (`hold_next_main_diff_read_for_e2e`) across the reload's
    // publish and its pane sweep. A commit's diff is immutable, so the read
    // is carried through the sweep and lands on its commit's new row; the
    // row key it was read under is stale, so it fills no cache.
    kagi.update(cx, |app, cx| {
        app.select(1);
        cx.notify();
    });
    settle(cx, window);
    let commit = cx.read(|app| kagi.read(app).view().details[1].full_sha.to_string());
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_main_diff_read_for_e2e(hold);
    kagi.update(cx, |app, cx| app.open_main_diff_commit(0, cx));
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "renumber"]);
    kagi.update(cx, |app, cx| app.reload_external(cx));
    settle(cx, window);
    settle(cx, window);
    assert_eq!(
        cx.read(|app| kagi.read(app).view().details[2].full_sha.to_string()),
        commit,
        "the reload moved the commit to row 2"
    );
    release.send(());
    settle(cx, window);
    let shown = main_view(cx, &kagi).expect("the pane sweep dropped the read still out");
    let MainDiffSource::Commit {
        row_index,
        file_index,
        commit: Some(landed),
    } = shown.source
    else {
        panic!("the carried read did not land as a commit diff");
    };
    assert_eq!(landed.0, commit, "another commit's diff is shown");
    assert_eq!(
        (row_index, file_index),
        (2, 0),
        "the carried read was not re-anchored to its commit's new row"
    );
    cx.read(|app| {
        let cache = &kagi.read(app).ui().diff_caches.file_content;
        assert!(
            !cache.contains_key(&(1, 0)) && !cache.contains_key(&(2, 0)),
            "a read keyed by the old rows filled the renumbered cache"
        );
    });

    unmount(cx, kagi, window);
    eprintln!("[gui-e2e] PASS commit_diff_off_thread");
}
