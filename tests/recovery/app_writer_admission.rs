//! Real editor event → host reservation while a linked remove is in pre_remove.
use crate::macos::{build_fixture, git, mount};
use gpui::VisualTestAppContext;
use kagi::ui::i18n::Msg;
use std::path::PathBuf;
use std::time::{Duration, Instant};

struct ReleaseOnDrop(PathBuf);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.0, b"release");
    }
}

pub fn scenario_editor_save_during_remove(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let controls = tempfile::tempdir().unwrap();
    let control = controls.path().canonicalize().unwrap();
    let entered = control.join("entered");
    let release = control.join("release");
    let script = control.join("slow-pre-remove.sh");
    // Bounded externally by the test deadline; Drop always releases the child
    // even when an assertion fails. No shell quoting is needed in argv paths.
    std::fs::write(
        &script,
        "printf ready > \"$1\"\nwhile [ ! -e \"$2\" ]; do sleep 0.02; done\n",
    )
    .unwrap();
    let linked = control.join("linked");
    git(
        &repo,
        &["worktree", "add", "-qb", "linked", linked.to_str().unwrap()],
    );
    std::fs::create_dir(linked.join(".kagi")).unwrap();
    std::fs::write(
        linked.join(".kagi/worktree.toml"),
        format!(
            "[[pre_remove]]\ntype = \"command\"\nrun = \"/bin/sh {} {} {}\"\n",
            script.display(),
            entered.display(),
            release.display(),
        ),
    )
    .unwrap();
    git(&linked, &["add", ".kagi/worktree.toml"]);
    git(&linked, &["commit", "-qm", "slow remove fixture"]);
    let _release_on_drop = ReleaseOnDrop(release.clone());
    let before = std::fs::read(repo.join("README.md")).unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx.read(|cx| app.read(cx).editor_workspace.clone()).unwrap();
    editor.update(cx, |view, cx| view.open_tab("README.md".into(), cx));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        if cx.read(|cx| editor.read(cx).editor.is_some() && editor.read(cx).content.is_some()) {
            break;
        }
        assert!(Instant::now() < deadline, "editor did not load");
        std::thread::sleep(Duration::from_millis(2));
    }
    cx.update_window(window, |_, window, cx| {
        let input = editor.read(cx).editor.clone().unwrap();
        input.update(cx, |input, cx| {
            input.set_value("must remain unsaved\n", window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    assert!(cx.read(|cx| editor.read(cx).dirty));
    app.update(cx, |app, cx| {
        app.open_remove_worktree_modal("linked".into(), true, cx)
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| app.read(cx).remove_worktree_modal().is_some()) {
            break;
        }
        assert!(Instant::now() < deadline, "remove plan did not arrive");
        std::thread::sleep(Duration::from_millis(2));
    }
    app.update(cx, |app, cx| app.confirm_remove_worktree(cx));
    let deadline = Instant::now() + Duration::from_secs(15);
    while !entered.exists() {
        cx.run_until_parked();
        assert!(Instant::now() < deadline, "slow pre_remove was not entered");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    app.update(cx, |app, cx| app.save_editor_file(cx));
    cx.run_until_parked();
    assert_eq!(std::fs::read(repo.join("README.md")).unwrap(), before);
    assert!(
        cx.read(|cx| editor.read(cx).dirty),
        "Busy must retain the buffer"
    );
    assert!(cx.read(|cx| app
        .read(cx)
        .toast_stack
        .as_ref()
        .unwrap()
        .read(cx)
        .toasts()
        .iter()
        .any(|toast| toast.message.as_ref() == Msg::OpInProgress.t())));
    std::fs::write(&release, b"release").unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while cx.read(|cx| app.read(cx).app_sessions.has_leases()) {
        cx.run_until_parked();
        assert!(Instant::now() < deadline, "remove did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(std::fs::read(repo.join("README.md")).unwrap(), before);
    eprintln!("[gui-e2e] PASS editor save during slow remove → Busy → bytes unchanged");
}
