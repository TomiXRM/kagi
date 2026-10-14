//! Batch staging publishes session status immediately, without waiting for a watcher.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::e2e;
use std::path::Path;

fn git_value(repo: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

pub fn scenario_batch_staging_refresh(cx: &mut VisualTestAppContext) {
    for count in [17, 200] {
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        for i in 0..count {
            std::fs::write(repo.join(format!("batch-{i:03}.txt")), "base\n").unwrap();
        }
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "batch base"]);
        for i in 0..count {
            std::fs::write(repo.join(format!("batch-{i:03}.txt")), "changed\n").unwrap();
        }
        let head = git_value(&repo, &["rev-parse", "HEAD"]);
        let unstaged_tree = git_value(&repo, &["write-tree"]);
        git(&repo, &["add", "."]);
        let staged_tree = git_value(&repo, &["write-tree"]);
        git(&repo, &["reset", "-q"]);
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| {
            e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
        });
        // #1570 opens with a placeholder; settle only that initial read.
        // Assertions after each batch write below must remain synchronous.
        cx.run_until_parked();
        let panel = cx.read(|cx| app.read(cx).ui().commit_panel.clone().unwrap());
        for staged in [true, false] {
            app.update(cx, |app, cx| {
                let owner = app.active_session().unwrap();
                if staged {
                    app.do_stage_all(owner, cx);
                } else {
                    app.do_unstage_all(owner, cx);
                }
            });
            // No watcher and no parked background tasks: the write's own
            // refresh must publish status before control returns to the UI.
            cx.read(|cx| {
                let state = &panel.read(cx).state;
                assert_eq!(state.staged.len(), if staged { count } else { 0 });
                assert_eq!(state.unstaged.len(), if staged { 0 } else { count });
                let summary = &app.read(cx).view().status_summary;
                assert_eq!(
                    summary.staged,
                    state.staged.len(),
                    "batch must publish staged status immediately"
                );
                assert_eq!(
                    summary.unstaged,
                    state.unstaged.len(),
                    "batch must publish unstaged status immediately"
                );
                assert!(summary.is_dirty);
            });
            assert_eq!(git_value(&repo, &["rev-parse", "HEAD"]), head);
            assert_eq!(
                git_value(&repo, &["write-tree"]),
                if staged { &staged_tree } else { &unstaged_tree }.as_str()
            );
            for i in 0..count {
                assert_eq!(
                    std::fs::read(repo.join(format!("batch-{i:03}.txt"))).unwrap(),
                    b"changed\n"
                );
            }
        }
        drop(panel);
        unmount(cx, app, window);
    }
}
