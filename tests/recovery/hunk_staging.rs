//! #842 (Refs #357): Stage / Unstage hunk on the Commit Panel diff.
//!
//! A committed 20-line file edited at lines 2 and 18 shows two hunks in its
//! unstaged diff. Clicking the first hunk header's measured "Stage hunk"
//! puts exactly that edit in the index: the panel lists the file on both
//! sides and the re-read diff shows the one hunk left. The staged diff's
//! "Unstage hunk" takes it back out and the pane closes, the staged side being
//! empty. Split view draws the same button on the full-width header row. A
//! held `index.lock` fails through #490's staging failure delivery.

use std::path::Path;

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::commit_panel::CommitPanelFileRef;
use kagi::ui::diff_view::DiffRow;
use kagi::ui::{e2e, theme, FooterStatus, KagiApp};

use crate::macos::{git, mount, unmount};
use crate::recovery_operations::{press_enter, wait_idle};

fn text(edit: impl Fn(usize) -> Option<&'static str>) -> String {
    (1..=20)
        .map(|n| match edit(n) {
            Some(t) => format!("{t}\n"),
            None => format!("line {n}\n"),
        })
        .collect()
}

fn index_text(repo: &Path) -> String {
    let out = std::process::Command::new("git")
        .args(["show", ":f.txt"])
        .current_dir(repo)
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap()
}

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

/// Row indices of the open main diff's hunk headers.
fn headers(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> Vec<usize> {
    cx.read(|cx| {
        let Some(pane) = app.read(cx).ui().main_diff.clone() else {
            return Vec::new();
        };
        let rows = pane.read(cx).view.rows.clone();
        rows.iter()
            .enumerate()
            .filter(|(_, r)| matches!(r, DiffRow::HunkHeader(_)))
            .map(|(i, _)| i)
            .collect()
    })
}

fn panel_counts(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> (usize, usize) {
    cx.read(|cx| {
        let panel = app.read(cx).ui().commit_panel.clone().expect("panel");
        let state = &panel.read(cx).state;
        (state.unstaged.len(), state.staged.len())
    })
}

fn open(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, file: CommitPanelFileRef) {
    app.update(cx, |app, cx| app.open_main_diff_wip(file, cx));
    cx.run_until_parked();
}

/// Click the hunk button on header row `row`.
fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, row: usize) {
    let id = format!("main-diff-hunk-stage-{row}");
    e2e::clear_control_bounds(window.window_id(), &id);
    for _ in 0..2 {
        draw(cx, window);
    }
    let bounds = e2e::control_bounds(window.window_id(), &id)
        .unwrap_or_else(|| panic!("{id} was not laid out"));
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

pub fn scenario_hunk_staging(cx: &mut VisualTestAppContext) {
    let split_before = theme::diff_split();
    theme::set_diff_split(false);
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().canonicalize().unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("f.txt"), text(|_| None)).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let both = text(|n| match n {
        2 => Some("TWO"),
        18 => Some("EIGHTEEN"),
        _ => None,
    });
    let first_only = text(|n| (n == 2).then_some("TWO"));
    std::fs::write(repo.join("f.txt"), &both).unwrap();

    let (app, window) = mount(cx, &repo);
    let panel_repo = repo.clone();
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, panel_repo, cx)
    });
    cx.run_until_parked();
    assert_eq!(panel_counts(cx, &app), (1, 0));

    // Stage the first of the two unstaged hunks.
    open(cx, &app, CommitPanelFileRef::Unstaged { index: 0 });
    let shown = headers(cx, &app);
    assert_eq!(shown.len(), 2, "two edits, two hunks");
    click(cx, window, shown[0]);
    wait_idle(cx, &app);
    assert_eq!(
        index_text(&repo),
        first_only,
        "exactly the first hunk staged"
    );
    assert_eq!(
        panel_counts(cx, &app),
        (1, 1),
        "the file is listed as unstaged and staged"
    );
    assert_eq!(
        headers(cx, &app).len(),
        1,
        "the re-read unstaged diff shows the hunk left"
    );
    assert_eq!(std::fs::read_to_string(repo.join("f.txt")).unwrap(), both);

    // Unstage it from the staged diff, in split view: the pane closes, the
    // staged side being empty again.
    theme::set_diff_split(true);
    open(cx, &app, CommitPanelFileRef::Staged { index: 0 });
    let staged = headers(cx, &app);
    assert_eq!(staged.len(), 1);
    click(cx, window, staged[0]);
    wait_idle(cx, &app);
    assert_eq!(index_text(&repo), text(|_| None), "the index is HEAD again");
    assert_eq!(panel_counts(cx, &app), (1, 0));
    assert!(
        cx.read(|cx| app.read(cx).ui().main_diff.is_none()),
        "the empty staged side closes the pane"
    );
    theme::set_diff_split(false);

    // #490: a held index.lock fails through the staging failure delivery and
    // leaves the index alone.
    open(cx, &app, CommitPanelFileRef::Unstaged { index: 0 });
    let shown = headers(cx, &app);
    let lock = repo.join(".git/index.lock");
    std::fs::write(&lock, "fixture lock").unwrap();
    click(cx, window, shown[0]);
    wait_idle(cx, &app);
    std::fs::remove_file(&lock).unwrap();
    cx.read(|cx| {
        let FooterStatus::Failed(footer) = &app.read(cx).status_footer else {
            panic!("missing staging failure footer")
        };
        assert!(footer.contains("lock"), "{footer}");
        assert!(footer.contains("f.txt"), "{footer}");
    });
    assert_eq!(
        index_text(&repo),
        text(|_| None),
        "the failure wrote nothing"
    );

    theme::set_diff_split(split_before);
    if cx.read(|cx| e2e::app_notice_message(app.read(cx)).is_some()) {
        press_enter(cx, &app, window);
    }
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS hunk_staging");
}
