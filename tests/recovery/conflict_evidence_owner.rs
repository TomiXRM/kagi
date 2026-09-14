//! #643 Wave 4 S2b: conflict detection evidence is owned by the tab session.
use crate::macos::{git, mount, unmount};
use gpui::VisualTestAppContext;
use std::path::{Path, PathBuf};

fn conflict_fixture(file: &str, main_text: &str, feature_text: &str) -> tempfile::TempDir {
    let fixture = tempfile::tempdir().expect("conflict fixture");
    let repo = fixture.path();
    git(repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join(file), "base\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "base"]);
    git(repo, &["checkout", "-qb", "feature"]);
    std::fs::write(repo.join(file), feature_text).unwrap();
    git(repo, &["commit", "-qam", "feature"]);
    git(repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join(file), main_text).unwrap();
    git(repo, &["commit", "-qam", "main"]);

    let status = std::process::Command::new("git")
        .args(["merge", "feature"])
        .current_dir(repo)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .expect("run conflicting merge");
    assert!(!status.success(), "fixture merge unexpectedly succeeded");
    fixture
}

fn assert_visible_conflict(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<kagi::ui::KagiApp>,
    expected_file: &Path,
    owner: kagi::app::SessionId,
    stage: &str,
) {
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            app.active_session(),
            Some(owner),
            "{stage}: wrong active owner"
        );
        assert!(
            app.view().operation.is_some(),
            "{stage}: the accepted read lost the in-progress operation"
        );
        let pane = app
            .ui()
            .conflict
            .as_ref()
            .unwrap_or_else(|| panic!("{stage}: conflict detector did not build a pane"));
        let pane_state = pane.read(cx);
        let mode = pane_state
            .mode
            .as_ref()
            .unwrap_or_else(|| panic!("{stage}: conflict pane has no mode"));
        let paths: Vec<PathBuf> = mode
            .session
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect();
        assert_eq!(
            paths,
            vec![expected_file.to_path_buf()],
            "{stage}: pane shows another owner's conflict evidence"
        );
    });
}

pub fn scenario_conflict_detector_owner_guard(cx: &mut VisualTestAppContext) {
    let fixture_a = conflict_fixture("alpha.txt", "alpha main\n", "alpha feature\n");
    let fixture_b = conflict_fixture("beta.txt", "beta main\n", "beta feature\n");
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo_a);
    app.update(cx, |app, cx| app.detect_conflict_mode_async(cx));
    cx.run_until_parked();
    let owner_a = cx.read(|cx| app.read(cx).active_session().expect("A owner"));
    assert_visible_conflict(
        cx,
        &app,
        Path::new("alpha.txt"),
        owner_a,
        "A initial detect",
    );

    let owner_b = app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx), "open B");
        assert!(
            app.ui().conflict.is_none(),
            "switch to B retained A's root conflict pane"
        );
        app.active_session().expect("B owner")
    });
    assert_ne!(owner_a, owner_b, "A and B must have distinct owners");
    cx.run_until_parked();
    assert_visible_conflict(cx, &app, Path::new("beta.txt"), owner_b, "B own detect");

    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        assert!(
            app.ui().conflict.is_none(),
            "return to A retained B's root conflict pane"
        );
    });
    cx.run_until_parked();
    assert_visible_conflict(cx, &app, Path::new("alpha.txt"), owner_a, "A return detect");

    let owner_a2 = app.update(cx, |app, cx| {
        let index = app
            .tabs
            .iter()
            .position(|tab| tab.session == owner_a)
            .expect("A tab remains open");
        app.close_tab(index, cx);
        assert!(app.open_repository(repo_a.clone(), cx), "reopen A");
        app.active_session().expect("reopened A owner")
    });
    assert_ne!(
        owner_a2, owner_a,
        "same-path reopen must create a new incarnation"
    );
    cx.run_until_parked();
    assert_visible_conflict(
        cx,
        &app,
        Path::new("alpha.txt"),
        owner_a2,
        "A new-incarnation detect",
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict detector guard follows its session owner");
}
