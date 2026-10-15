//! Moving non-branch rows share the same semantic boundary, including no-action rows.
use super::{drawn, pointer, support, POPUP};
use crate::macos::{build_fixture, git, unmount};
use gpui::{point, px, AnyWindowHandle, Entity, MouseButton, VisualTestAppContext};
use kagi::ui::{build_tab_view, e2e, CompareTarget, KagiApp};
use kagi_ui_core::{i18n::Lang, theme};
use std::{
    path::Path,
    time::{Duration, Instant},
};

pub(super) fn mount(
    cx: &mut VisualTestAppContext,
    repo: &Path,
    compact: bool,
    pane: usize,
) -> (Entity<KagiApp>, AnyWindowHandle, Instant) {
    theme::set_reduce_motion(true);
    let (app, window) =
        support::mount_dimensions(cx, repo, if compact { (940., 660.) } else { (1440., 900.) });
    support::appearance(cx, &app, window, Lang::En, "apple-light", 1., pane);
    let t = Instant::now();
    drawn(cx, &app, window, t);
    if compact {
        let rail = ["local", "remote", "worktrees", "tags", "stashes"][pane];
        super::click(cx, &app, window, &format!("compact-rail-{rail}"));
        drawn(cx, &app, window, t);
    }
    (app, window, t)
}

fn overlap(window: AnyWindowHandle, target: &str, covered: &str) -> gpui::Point<gpui::Pixels> {
    let target = e2e::control_bounds(window.window_id(), target).expect("actual target control");
    let covered = e2e::control_bounds(window.window_id(), covered).expect("actual covered control");
    let top = target.top().max(covered.top());
    let bottom = target.bottom().min(covered.bottom());
    assert!(
        bottom - top > px(2.),
        "37ms actual body overlap: target={target:?} covered={covered:?}"
    );
    point(target.left() + px(32.), (top + bottom) * 0.5)
}

pub(super) fn focus(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    pane: usize,
    key: &str,
) {
    cx.simulate_keystrokes(window, "f19");
    support::focus(cx, app, window, pane, key);
}

pub(super) fn root_focus(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    label: &str,
    failures: &mut Vec<String>,
) {
    if support::focused(cx, app, window).is_some() {
        failures.push(format!(
            "{label}: pointer stole covered row's keyboard/AX focus"
        ));
    }
}

// Reuse the paged-tag scenario's real backend snapshot seam, not manufactured rows.
fn page(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, repo: &Path) {
    let snapshot = kagi_git::Backend::open(repo).unwrap().snapshot(2).unwrap();
    app.update(cx, |app, cx| {
        app.ui_mut().unwrap().commit_limit = 2;
        app.amend_tab_view(
            app.active_session().unwrap(),
            build_tab_view(&snapshot, "pointer fixture"),
        );
        let head = kagi_git::CommitId(support::output(repo, &["rev-parse", "HEAD"]));
        app.select(app.view().commit_row_index[&head]);
        cx.notify();
    });
}

fn history(repo: &Path) {
    for message in ["newer loaded pointer target", "current pointer baseline"] {
        git(repo, &["commit", "--allow-empty", "-qm", message]);
    }
}

fn tags(cx: &mut VisualTestAppContext, compact: bool, disabled: bool, failures: &mut Vec<String>) {
    let fixture = build_fixture();
    let repo = fixture.path();
    history(repo);
    for name in ["a", "e", "z"] {
        git(repo, &["tag", name, "HEAD~2"]);
    }
    let target = kagi_git::CommitId(support::output(repo, &["rev-parse", "z"]));
    let (app, window, t) = mount(cx, repo, compact, 3);
    if disabled {
        page(cx, &app, repo);
        drawn(cx, &app, window, t);
    }
    let baseline = app.read_with(cx, |app, _| app.ui().selected);
    let expected = app.read_with(cx, |app, _| {
        app.view().commit_row_index.get(&target).copied()
    });
    assert_eq!(
        expected.is_none(),
        disabled,
        "target availability is real history paging"
    );
    let covered_tip = kagi_git::CommitId(support::output(repo, &["rev-parse", "HEAD~1"]));
    assert_ne!(
        baseline,
        app.read_with(cx, |app, _| app
            .view()
            .commit_row_index
            .get(&covered_tip)
            .copied()),
        "covered action cannot be a no-op"
    );
    for name in ["b", "d", "f"] {
        git(repo, &["tag", name, "HEAD~1"]);
    }
    theme::set_reduce_motion(false);
    app.update(cx, |app, cx| app.reload(cx));
    drawn(cx, &app, window, t);
    let at = t + Duration::from_millis(37);
    drawn(cx, &app, window, at);
    let hit = overlap(window, "sidebar-tag-z", "sidebar-tag-f");
    let witness = support::Witness::read(repo);
    focus(cx, &app, window, 3, "tag:f");
    pointer(cx, window, hit, MouseButton::Right);
    assert_eq!(
        app.read_with(cx, |app, _| app
            .tag_menu
            .as_ref()
            .map(|menu| menu.name.as_str().to_owned())),
        Some("z".into()),
        "moving tag's actual menu"
    );
    let label = format!("compact={compact}/tag disabled={disabled}");
    root_focus(cx, &app, window, &label, failures);
    app.update(cx, |app, cx| {
        app.tag_menu = None;
        cx.notify();
    });
    drawn(cx, &app, window, at);
    focus(cx, &app, window, 3, "tag:f");
    let generation = app.read_with(cx, |app, _| app.ui().load_more_gen);
    pointer(cx, window, hit, MouseButton::Left);
    let actual = app.read_with(cx, |app, _| app.ui().selected);
    let expected = if disabled { baseline } else { expected };
    if actual != expected {
        failures.push(format!(
            "{label}: selected {actual:?}, expected {expected:?}"
        ));
    }
    if disabled {
        assert_eq!(
            app.read_with(cx, |app, _| app.ui().load_more_gen),
            generation + 1,
            "disabled tag retains exactly its existing pagination feedback"
        );
    }
    root_focus(cx, &app, window, &label, failures);
    drawn(cx, &app, window, at);
    if compact {
        let open = e2e::control_bounds(window.window_id(), POPUP).is_some();
        if open != disabled {
            failures.push(format!("{label}: popup open={open}, expected {disabled}"));
        }
    }
    drawn(cx, &app, window, t + Duration::from_millis(500));
    assert_eq!(
        support::Witness::read(repo),
        witness,
        "tag menus/navigation/paging remain read-only"
    );
    unmount(cx, app, window);
}

fn disabled_remote(cx: &mut VisualTestAppContext, compact: bool, failures: &mut Vec<String>) {
    let fixture = build_fixture();
    let repo = fixture.path();
    history(repo);
    for name in ["aaa/a", "aaa/c", "aaa/e"] {
        git(
            repo,
            &[
                "update-ref",
                &format!("refs/remotes/origin/{name}"),
                "HEAD~1",
            ],
        );
    }
    git(repo, &["update-ref", "refs/remotes/origin/aaa/z", "HEAD~2"]);
    let (app, window, t) = mount(cx, repo, compact, 1);
    page(cx, &app, repo);
    drawn(cx, &app, window, t);
    super::click(cx, &app, window, "sidebar-group-remote:origin:aaa");
    drawn(cx, &app, window, t);
    let baseline = app.read_with(cx, |app, _| app.ui().selected);
    let target = kagi_git::CommitId(support::output(repo, &["rev-parse", "HEAD~2"]));
    assert!(!app.read_with(cx, |app, _| app
        .view()
        .commit_row_index
        .contains_key(&target)));
    theme::set_reduce_motion(false);
    super::click(cx, &app, window, "sidebar-group-remote:origin:aaa");
    let at = t + Duration::from_millis(37);
    drawn(cx, &app, window, at);
    let hit = overlap(
        window,
        "sidebar-remote-origin/aaa/z",
        "sidebar-remote-origin/aaa/e",
    );
    let witness = support::Witness::read(repo);
    focus(cx, &app, window, 1, "remote:origin/aaa/e");
    pointer(cx, window, hit, MouseButton::Right);
    assert_eq!(
        app.read_with(cx, |app, _| app
            .branch_menu
            .as_ref()
            .map(|menu| menu.name.as_str().to_owned())),
        Some("origin/aaa/z".into()),
        "disabled remote retains its own context target"
    );
    let label = format!("compact={compact}/disabled remote");
    root_focus(cx, &app, window, &label, failures);
    app.update(cx, |app, cx| {
        app.branch_menu = None;
        cx.notify();
    });
    drawn(cx, &app, window, at);
    focus(cx, &app, window, 1, "remote:origin/aaa/e");
    pointer(cx, window, hit, MouseButton::Left);
    let actual = app.read_with(cx, |app, _| app.ui().selected);
    if actual != baseline {
        failures.push(format!(
            "{label}: no-action selected {actual:?}, expected {baseline:?}"
        ));
    }
    root_focus(cx, &app, window, &label, failures);
    drawn(cx, &app, window, at);
    if compact && e2e::control_bounds(window.window_id(), POPUP).is_none() {
        failures.push(format!("{label}: no-action row dismissed popup"));
    }
    assert_eq!(support::Witness::read(repo), witness);
    unmount(cx, app, window);
}

fn push(repo: &Path, content: &str) -> String {
    std::fs::write(repo.join("README.md"), content).unwrap();
    git(repo, &["stash", "push", "-qm", content]);
    support::output(repo, &["rev-parse", "refs/stash"])
}
fn stashes(cx: &mut VisualTestAppContext, compact: bool, failures: &mut Vec<String>) {
    let fixture = build_fixture();
    let repo = fixture.path();
    push(repo, "old a");
    push(repo, "old b");
    let expected = push(repo, "old c");
    let (app, window, t) = mount(cx, repo, compact, 4);
    let covered = push(repo, "new a");
    push(repo, "new b");
    push(repo, "new c");
    theme::set_reduce_motion(false);
    app.update(cx, |app, cx| app.reload(cx));
    drawn(cx, &app, window, t);
    let at = t + Duration::from_millis(37);
    drawn(cx, &app, window, at);
    let hit = overlap(window, "sidebar-stash-3", "sidebar-stash-2");
    let witness = support::Witness::read(repo);
    focus(cx, &app, window, 4, &format!("stash:{covered}"));
    pointer(cx, window, hit, MouseButton::Right);
    assert_eq!(
        app.read_with(cx, |app, _| app.stash_menu.as_ref().map(|menu| menu.index)),
        Some(3),
        "moving stash actual menu index"
    );
    let label = format!("compact={compact}/stash");
    root_focus(cx, &app, window, &label, failures);
    app.update(cx, |app, cx| {
        app.stash_menu = None;
        cx.notify();
    });
    drawn(cx, &app, window, at);
    focus(cx, &app, window, 4, &format!("stash:{covered}"));
    pointer(cx, window, hit, MouseButton::Left);
    let actual = cx.read(|cx| {
        app.read(cx)
            .ui()
            .compare_view
            .as_ref()
            .map(|compare| compare.read(cx).view().target.clone())
    });
    let wanted = Some(CompareTarget::Commit(kagi_git::CommitId(expected)));
    if actual != wanted {
        failures.push(format!("{label}: peek {actual:?}, expected {wanted:?}"));
    }
    root_focus(cx, &app, window, &label, failures);
    drawn(cx, &app, window, at);
    if compact && e2e::control_bounds(window.window_id(), POPUP).is_some() {
        failures.push(format!("{label}: selected stash did not dismiss popup"));
    }
    app.update(cx, |app, cx| {
        app.close_compare_view();
        cx.notify();
    });
    assert_eq!(
        support::Witness::read(repo),
        witness,
        "stash pointer peeks cannot consume or modify stashes"
    );
    unmount(cx, app, window);
}

fn worktrees(cx: &mut VisualTestAppContext, compact: bool, failures: &mut Vec<String>) {
    let _ports = crate::gui_isolation::PortStore::keep();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let linked = tempfile::tempdir().unwrap();
    let root = linked.path().canonicalize().unwrap();
    for name in ["a-kept", "z"] {
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                root.join(name).to_str().unwrap(),
                "HEAD",
            ],
        );
    }
    let target = root.join("z");
    let settings = kagi::ui::settings::Settings::load();
    let (start, end) = settings.worktree_port_range();
    let port_number = kagi_git::worktree_ports::assign_block(
        &target,
        kagi_domain::worktree_ports::PortRange { start, end },
        settings.worktree_ports_per_worktree(),
    )
    .unwrap();
    let (app, window, t) = mount(cx, &repo, compact, 2);
    for name in ["b-new", "d-new", "f-new"] {
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                root.join(name).to_str().unwrap(),
                "HEAD",
            ],
        );
    }
    theme::set_reduce_motion(false);
    app.update(cx, |app, cx| app.reload(cx));
    drawn(cx, &app, window, t);
    let at = t + Duration::from_millis(37);
    drawn(cx, &app, window, at);
    let hit = overlap(window, "sidebar-worktree-z", "sidebar-worktree-f-new");
    let witness = support::Witness::read(&repo);
    let owner = app.read_with(cx, |app, _| app.active_session());
    let key = format!("worktree:{}", root.join("f-new").display());
    focus(cx, &app, window, 2, &key);
    pointer(cx, window, hit, MouseButton::Right);
    assert_eq!(
        app.read_with(cx, |app, _| app
            .worktree_menu
            .as_ref()
            .and_then(|menu| menu.path.clone())),
        Some(target.clone()),
        "moving worktree actual menu path"
    );
    let label = format!("compact={compact}/worktree");
    root_focus(cx, &app, window, &label, failures);
    app.update(cx, |app, cx| {
        app.worktree_menu = None;
        cx.notify();
    });
    drawn(cx, &app, window, at);
    // A real nested link must still consume before the shared real-row boundary.
    let port = e2e::control_bounds(window.window_id(), "sidebar-worktree-port-z")
        .expect("real child localhost link");
    let paint = e2e::control_paint(window.window_id(), "sidebar-worktree-port-z").unwrap();
    assert!(
        port.center().x >= paint.mask.left()
            && port.center().x < paint.mask.right()
            && port.center().y >= paint.mask.top()
            && port.center().y < paint.mask.bottom(),
        "child link point must be visibly inside its actual clip: {paint:?}"
    );
    e2e::take_opened_urls();
    focus(cx, &app, window, 2, &key);
    pointer(cx, window, port.center(), MouseButton::Left);
    assert_eq!(
        e2e::take_opened_urls(),
        [format!("http://localhost:{port_number}")],
        "real child link opens exactly its own URL"
    );
    assert_eq!(
        app.read_with(cx, |app, _| app.active_session()),
        owner,
        "child link does not select repository"
    );
    drawn(cx, &app, window, at);
    if compact {
        assert!(
            e2e::control_bounds(window.window_id(), POPUP).is_some(),
            "child link does not dismiss popover"
        );
    }
    focus(cx, &app, window, 2, &key);
    pointer(cx, window, hit, MouseButton::Left);
    let actual = app.read_with(cx, |app, _| app.repo_path.clone());
    let wanted = if compact { target } else { repo.clone() };
    if actual != Some(wanted.clone()) {
        failures.push(format!(
            "{label}: repository {actual:?}, expected {wanted:?}"
        ));
    }
    if compact && app.read_with(cx, |app, _| app.active_session()) == owner {
        failures.push(format!(
            "{label}: compact click did not select another repository"
        ));
    }
    root_focus(cx, &app, window, &label, failures);
    drawn(cx, &app, window, at);
    if compact && e2e::control_bounds(window.window_id(), POPUP).is_some() {
        failures.push(format!(
            "{label}: selected worktree did not dismiss popover"
        ));
    }
    assert_eq!(
        support::Witness::read(&repo),
        witness,
        "worktree navigation does not mutate repository"
    );
    unmount(cx, app, window);
}

pub(super) fn other_rows(cx: &mut VisualTestAppContext, failures: &mut Vec<String>) {
    for compact in [false, true] {
        tags(cx, compact, false, failures);
        tags(cx, compact, true, failures);
        disabled_remote(cx, compact, failures);
        stashes(cx, compact, failures);
        worktrees(cx, compact, failures);
    }
}
