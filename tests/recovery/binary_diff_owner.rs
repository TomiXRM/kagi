//! #1075: prepared image bytes must belong to the consumer that requested them.

use crate::diff_highlight::{assert_image_bytes, commit_image_index, main_view, png_bytes, settle};
use crate::evidence_support::deferred;
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{Entity, VisualTestAppContext};
use kagi::ui::commit_panel::CommitPanelFileRef;
use kagi::ui::diff_view::MainDiffSource;
use kagi::ui::{e2e as seam, KagiApp};
use std::path::Path;
use std::sync::Arc;

fn image_fixture(before: &[u8], after: &[u8]) -> tempfile::TempDir {
    let fixture = build_fixture();
    let repo = fixture.path();
    std::fs::write(repo.join("changed.png"), before).unwrap();
    git(repo, &["add", "changed.png"]);
    git(repo, &["commit", "-q", "-m", "image owner base"]);
    std::fs::write(repo.join("changed.png"), after).unwrap();
    git(repo, &["add", "changed.png"]);
    git(repo, &["commit", "-q", "-m", "image owner change"]);
    fixture
}

fn compare_image_index(cx: &mut VisualTestAppContext, kagi: &Entity<KagiApp>) -> usize {
    cx.read(|cx| {
        kagi.read(cx)
            .ui()
            .compare_view
            .as_ref()
            .unwrap()
            .read(cx)
            .view()
            .files
            .iter()
            .position(|file| file.path == Path::new("changed.png"))
            .unwrap()
    })
}

fn panel_image_ref(
    cx: &mut VisualTestAppContext,
    kagi: &Entity<KagiApp>,
    path: &str,
    staged: bool,
) -> CommitPanelFileRef {
    cx.read(|cx| {
        let state = &kagi
            .read(cx)
            .ui()
            .commit_panel
            .as_ref()
            .unwrap()
            .read(cx)
            .state;
        let files = if staged {
            &state.staged
        } else {
            &state.unstaged
        };
        let index = files
            .iter()
            .position(|file| file.path == Path::new(path))
            .unwrap();
        if staged {
            CommitPanelFileRef::Staged { index }
        } else {
            CommitPanelFileRef::Unstaged { index }
        }
    })
}

pub fn scenario_binary_diff_owner_transitions(cx: &mut VisualTestAppContext) {
    let before = png_bytes([255, 0, 0, 255]);
    let after = png_bytes([0, 255, 0, 255]);
    let workdir = png_bytes([0, 0, 255, 255]);
    let fixture = image_fixture(&before, &after);
    let repo = fixture.path().canonicalize().unwrap();
    let (kagi, window) = mount(cx, &repo);
    kagi.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    settle(cx, window);
    let index = commit_image_index(cx, &kagi, "changed.png");

    // The OID is A again, but selection closed the consumer twice. The old
    // cache-miss request must neither reopen the pane nor fill its row key.
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_main_diff_read_for_e2e(hold);
    kagi.update(cx, |app, cx| {
        app.open_main_diff_commit(index, cx);
        app.select(1);
        app.select(0);
        cx.notify();
    });
    release.send(());
    cx.run_until_parked();
    assert!(
        main_view(cx, &kagi).is_none(),
        "selection ABA revived an old image request"
    );
    cx.read(|cx| {
        assert!(!kagi
            .read(cx)
            .ui()
            .diff_caches
            .file_content
            .contains_key(&(0, index)));
    });

    // Both requests are the same immutable file. Byte equality alone could
    // miss the late install, so also retain the newer prepared image identity.
    kagi.update(cx, |app, cx| app.open_main_diff_commit(index, cx));
    cx.run_until_parked();
    assert_image_bytes(&main_view(cx, &kagi).unwrap(), Some(&before), Some(&after));
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_main_diff_read_for_e2e(hold);
    kagi.update(cx, |app, cx| {
        app.open_main_diff_commit(index, cx);
        app.select(1);
        app.select(0);
        app.open_main_diff_commit(index, cx);
    });
    cx.run_until_parked();
    let newer = main_view(cx, &kagi).unwrap().images.unwrap().new.unwrap();
    release.send(());
    cx.run_until_parked();
    let shown = main_view(cx, &kagi).unwrap();
    assert_image_bytes(&shown, Some(&before), Some(&after));
    assert!(Arc::ptr_eq(
        &newer,
        shown.images.as_ref().unwrap().new.as_ref().unwrap()
    ));

    // A new read on the next tab visit wins even over a cached image request
    // from the old visit of this same session and commit.
    let other = build_fixture();
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_main_diff_read_for_e2e(hold);
    kagi.update(cx, |app, cx| {
        app.open_main_diff_commit(index, cx);
        assert!(app.open_repository(other.path().canonicalize().unwrap(), cx));
        app.switch_repo(0, cx);
    });
    cx.run_until_parked();
    settle(cx, window);
    settle(cx, window);
    let index = commit_image_index(cx, &kagi, "changed.png");
    kagi.update(cx, |app, cx| app.open_main_diff_commit(index, cx));
    cx.run_until_parked();
    let revisited = main_view(cx, &kagi).unwrap().images.unwrap().new.unwrap();
    release.send(());
    cx.run_until_parked();
    let shown = main_view(cx, &kagi).unwrap();
    assert_image_bytes(&shown, Some(&before), Some(&after));
    assert!(Arc::ptr_eq(
        &revisited,
        shown.images.as_ref().unwrap().new.as_ref().unwrap()
    ));

    // The Compare entity survives A → working tree → A. Matching base/target
    // and path is insufficient: this read belongs to its earlier revision.
    std::fs::write(repo.join("changed.png"), &workdir).unwrap();
    let head = cx.read(|cx| kagi.read(cx).view().rows[0].id.clone());
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_main_diff_read_for_e2e(hold);
    kagi.update(cx, |app, cx| {
        app.open_main_diff_commit(index, cx);
        app.open_compare_with_working_tree(head, Some(cx));
    });
    release.send(());
    cx.run_until_parked();
    assert!(
        main_view(cx, &kagi).is_none(),
        "Compare inherited a pending commit image"
    );

    let base = kagi.update(cx, |app, cx| {
        let base = app.view().rows[1].id.clone();
        app.open_compare_with_head(base.clone(), Some(cx));
        base
    });
    let index = compare_image_index(cx, &kagi);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_main_diff_read_for_e2e(hold);
    kagi.update(cx, |app, cx| {
        app.open_main_diff_compare(index, cx);
        app.open_compare_with_working_tree(base.clone(), Some(cx));
        app.open_compare_with_head(base, Some(cx));
    });
    release.send(());
    cx.run_until_parked();
    assert!(
        main_view(cx, &kagi).is_none(),
        "Compare ABA revived its earlier revision"
    );
    let index = compare_image_index(cx, &kagi);
    kagi.update(cx, |app, cx| app.open_main_diff_compare(index, cx));
    cx.run_until_parked();
    assert_image_bytes(&main_view(cx, &kagi).unwrap(), Some(&before), Some(&after));

    // Stash peek is the fixed-commit Compare consumer, not HEAD/workdir. A
    // subsequent worktree change must not become its prepared target side.
    git(&repo, &["stash", "push", "-q", "-m", "image owner peek"]);
    kagi.update(cx, |app, cx| app.open_stash_peek(0, cx));
    let index = compare_image_index(cx, &kagi);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_main_diff_read_for_e2e(hold);
    kagi.update(cx, |app, cx| app.open_main_diff_compare(index, cx));
    std::fs::write(repo.join("changed.png"), &before).unwrap();
    release.send(());
    cx.run_until_parked();
    assert_image_bytes(&main_view(cx, &kagi).unwrap(), Some(&after), Some(&workdir));

    unmount(cx, kagi, window);
    eprintln!("[gui-e2e] PASS binary_diff_owner_transitions");
}

pub fn scenario_binary_diff_worktree_owner(cx: &mut VisualTestAppContext) {
    let before = png_bytes([255, 0, 0, 255]);
    let head = png_bytes([0, 255, 0, 255]);
    let staged = png_bytes([0, 0, 255, 255]);
    let tab_workdir = png_bytes([255, 0, 255, 255]);
    let fixture = image_fixture(&before, &head);
    let repo = fixture.path().canonicalize().unwrap();
    let foreign_fixture = tempfile::tempdir().unwrap();
    let foreign = foreign_fixture.path().canonicalize().unwrap();
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "image-owner",
            foreign.to_str().unwrap(),
            "HEAD",
        ],
    );
    std::fs::write(repo.join("changed.png"), &tab_workdir).unwrap();
    std::fs::write(foreign.join("changed.png"), &staged).unwrap();
    git(&foreign, &["add", "changed.png"]);
    std::fs::write(foreign.join("changed.png"), &before).unwrap();
    let (kagi, window) = mount(cx, &repo);
    kagi.update(cx, |app, cx| {
        seam::open_worktree_panel_no_inputs(app, foreign.clone(), "image-owner", 0, cx)
    });
    let file = panel_image_ref(cx, &kagi, "changed.png", true);
    kagi.update(cx, |app, cx| app.open_main_diff_wip(file, cx));
    cx.run_until_parked();
    assert_image_bytes(&main_view(cx, &kagi).unwrap(), Some(&head), Some(&staged));
    let file = panel_image_ref(cx, &kagi, "changed.png", false);
    kagi.update(cx, |app, cx| app.open_main_diff_wip(file, cx));
    cx.run_until_parked();
    assert_image_bytes(&main_view(cx, &kagi).unwrap(), Some(&staged), Some(&before));

    // The panel goes foreign → local → foreign while the same path/side is
    // still being read. The returning panel must not inherit the old request.
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_main_diff_read_for_e2e(hold);
    let file = panel_image_ref(cx, &kagi, "changed.png", false);
    kagi.update(cx, |app, cx| {
        app.open_main_diff_wip(file, cx);
        seam::open_local_panel_no_inputs(app, repo.clone(), cx);
        seam::open_worktree_panel_no_inputs(app, foreign.clone(), "image-owner", 0, cx);
    });
    release.send(());
    cx.run_until_parked();
    assert!(
        main_view(cx, &kagi).is_none(),
        "replaced panel inherited stale image bytes"
    );

    // A staged addition and its unstaged deletion keep exactly one side.
    std::fs::write(foreign.join("added.png"), &staged).unwrap();
    git(&foreign, &["add", "added.png"]);
    kagi.update(cx, |app, cx| {
        seam::open_worktree_panel_no_inputs(app, foreign.clone(), "image-owner", 0, cx)
    });
    let file = panel_image_ref(cx, &kagi, "added.png", true);
    kagi.update(cx, |app, cx| app.open_main_diff_wip(file, cx));
    cx.run_until_parked();
    assert_image_bytes(&main_view(cx, &kagi).unwrap(), None, Some(&staged));
    std::fs::remove_file(foreign.join("added.png")).unwrap();
    kagi.update(cx, |app, cx| {
        seam::open_worktree_panel_no_inputs(app, foreign.clone(), "image-owner", 0, cx)
    });
    let file = panel_image_ref(cx, &kagi, "added.png", false);
    kagi.update(cx, |app, cx| app.open_main_diff_wip(file, cx));
    cx.run_until_parked();
    assert_image_bytes(&main_view(cx, &kagi).unwrap(), Some(&staged), None);

    // A current foreign refresh keeps its own index/workdir pairing. A held
    // earlier open cannot replace the refresh, even with equal resulting bytes.
    let file = panel_image_ref(cx, &kagi, "changed.png", false);
    kagi.update(cx, |app, cx| app.open_main_diff_wip(file, cx));
    cx.run_until_parked();
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_main_diff_read_for_e2e(hold);
    let file = panel_image_ref(cx, &kagi, "changed.png", false);
    kagi.update(cx, |app, cx| app.open_main_diff_wip(file, cx));
    std::fs::write(foreign.join("changed.png"), &head).unwrap();
    kagi.update(cx, |app, cx| app.reload_external(cx));
    settle(cx, window);
    settle(cx, window);
    let refreshed = main_view(cx, &kagi).unwrap();
    assert_image_bytes(&refreshed, Some(&staged), Some(&head));
    let refreshed_image = refreshed.images.unwrap().new.unwrap();
    release.send(());
    cx.run_until_parked();
    let shown = main_view(cx, &kagi).unwrap();
    assert_image_bytes(&shown, Some(&staged), Some(&head));
    assert!(Arc::ptr_eq(
        &refreshed_image,
        shown.images.as_ref().unwrap().new.as_ref().unwrap()
    ));

    // When the ordinary panel becomes entirely clean it may disappear before
    // the refresh's Nothing result. That result still owns the retained diff.
    kagi.update(cx, |app, cx| {
        seam::open_local_panel_no_inputs(app, repo.clone(), cx)
    });
    let file = panel_image_ref(cx, &kagi, "changed.png", false);
    kagi.update(cx, |app, cx| app.open_main_diff_wip(file, cx));
    cx.run_until_parked();
    assert_image_bytes(
        &main_view(cx, &kagi).unwrap(),
        Some(&head),
        Some(&tab_workdir),
    );
    std::fs::write(repo.join("changed.png"), &head).unwrap();
    kagi.update(cx, |app, cx| app.reload_external(cx));
    settle(cx, window);
    settle(cx, window);
    assert!(
        main_view(cx, &kagi).is_none(),
        "cleaned file retained its old image diff"
    );
    cx.read(|cx| assert!(kagi.read(cx).ui().commit_panel.is_none()));

    unmount(cx, kagi, window);
    eprintln!("[gui-e2e] PASS binary_diff_worktree_owner");
}

/// A returning tab revalidates its retained foreign diff before the empty
/// panel is removed. The prepared Nothing still belongs to that diff.
pub fn scenario_binary_diff_linked_empty_revisit(cx: &mut VisualTestAppContext) {
    let before = png_bytes([255, 0, 0, 255]);
    let head = png_bytes([0, 255, 0, 255]);
    let foreign_workdir = png_bytes([0, 0, 255, 255]);
    let tab_workdir = png_bytes([255, 0, 255, 255]);
    let fixture = image_fixture(&before, &head);
    let repo = fixture.path().canonicalize().unwrap();
    let foreign_fixture = tempfile::tempdir().unwrap();
    let foreign = foreign_fixture.path().canonicalize().unwrap();
    assert_ne!(repo, foreign);
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "image-empty-revisit",
            foreign.to_str().unwrap(),
            "HEAD",
        ],
    );
    // The tab remains dirty with different bytes: a tab-repository fallback
    // would show another diff instead of closing the now-clean foreign file.
    std::fs::write(repo.join("changed.png"), &tab_workdir).unwrap();
    std::fs::write(foreign.join("changed.png"), &foreign_workdir).unwrap();
    let other = build_fixture();
    let other_repo = other.path().canonicalize().unwrap();
    let (kagi, window) = mount(cx, &repo);
    kagi.update(cx, |app, cx| {
        seam::open_worktree_panel_no_inputs(app, foreign.clone(), "image-empty-revisit", 0, cx);
    });
    let file = panel_image_ref(cx, &kagi, "changed.png", false);
    kagi.update(cx, |app, cx| app.open_main_diff_wip(file, cx));
    cx.run_until_parked();
    assert_image_bytes(
        &main_view(cx, &kagi).unwrap(),
        Some(&head),
        Some(&foreign_workdir),
    );
    let (owner, pane_id, panel_id, publish_gen, request) = cx.read(|cx| {
        let app = kagi.read(cx);
        let owner = app.active_session().unwrap();
        let pane = app.ui().main_diff.as_ref().unwrap();
        let panel = app.ui().commit_panel.as_ref().unwrap();
        assert_eq!(app.repo_path.as_ref(), Some(&repo));
        assert_eq!(panel.read(cx).owner, owner);
        assert_eq!(panel.read(cx).repo_path, foreign);
        assert!(panel.read(cx).state.commit_msg.is_empty());
        assert!(matches!(
            &pane.read(cx).view.source,
            MainDiffSource::Unstaged { path } if path == Path::new("changed.png")
        ));
        (
            owner,
            pane.entity_id(),
            panel.entity_id(),
            app.ui().view_publish_gen,
            app.ui().main_diff_req,
        )
    });

    let other_owner = kagi.update(cx, |app, cx| {
        assert!(app.open_repository(other_repo.clone(), cx));
        let other_owner = app.active_session().unwrap();
        assert_ne!(owner, other_owner);
        assert!(app.ui().main_diff.is_none());
        assert!(app.ui().commit_panel.is_none());
        other_owner
    });
    cx.run_until_parked();
    settle(cx, window);
    std::fs::write(foreign.join("changed.png"), &head).unwrap();
    cx.read(|cx| {
        let app = kagi.read(cx);
        assert_eq!(app.active_session(), Some(other_owner));
        assert_eq!(app.repo_path.as_ref(), Some(&other_repo));
        let retained = app.ui.get(&owner).unwrap();
        assert_eq!(retained.main_diff.as_ref().unwrap().entity_id(), pane_id);
        let panel = retained.commit_panel.as_ref().unwrap();
        assert_eq!(panel.entity_id(), panel_id);
        assert_eq!(panel.read(cx).repo_path, foreign);
        assert_eq!(panel.read(cx).owner, owner);
    });

    // Arm before activation: native rendering may revalidate during the
    // dispatcher park itself. The hold belongs to that real consumer read.
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_main_diff_read_for_e2e(hold);
    kagi.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        assert_eq!(app.active_session(), Some(owner));
        assert_eq!(app.repo_path.as_ref(), Some(&repo));
        assert_eq!(app.ui().main_diff.as_ref().unwrap().entity_id(), pane_id);
        assert_eq!(
            app.ui().commit_panel.as_ref().unwrap().entity_id(),
            panel_id
        );
    });
    cx.run_until_parked();
    settle(cx, window);
    cx.read(|cx| {
        let app = kagi.read(cx);
        assert!(app.ui().view_publish_gen > publish_gen);
        assert_eq!(app.active_session(), Some(owner));
        assert_eq!(app.repo_path.as_ref(), Some(&repo));
        assert!(
            app.ui().commit_panel.is_none(),
            "clean foreign panel was retained"
        );
        assert!(!app.ui().commit_panel_open);
        assert!(app.ui().main_diff_req > request);
        assert_eq!(app.ui().main_diff.as_ref().unwrap().entity_id(), pane_id);
        let other_ui = app.ui.get(&other_owner).unwrap();
        assert!(other_ui.main_diff.is_none());
        assert!(other_ui.commit_panel.is_none());
    });
    assert_image_bytes(
        &main_view(cx, &kagi).unwrap(),
        Some(&head),
        Some(&foreign_workdir),
    );
    assert_eq!(
        std::fs::read(repo.join("changed.png")).unwrap(),
        tab_workdir
    );
    assert_eq!(std::fs::read(foreign.join("changed.png")).unwrap(), head);

    release.send(());
    cx.run_until_parked();
    assert!(
        main_view(cx, &kagi).is_none(),
        "clean linked-worktree image diff survived retained-pane revalidation"
    );
    cx.read(|cx| {
        let app = kagi.read(cx);
        assert_eq!(app.active_session(), Some(owner));
        assert_eq!(app.repo_path.as_ref(), Some(&repo));
        assert!(app.ui().commit_panel.is_none());
        assert!(app.ui.get(&other_owner).unwrap().main_diff.is_none());
    });
    assert_eq!(
        std::fs::read(repo.join("changed.png")).unwrap(),
        tab_workdir
    );

    unmount(cx, kagi, window);
    eprintln!("[gui-e2e] PASS binary_diff_linked_empty_revisit");
}
