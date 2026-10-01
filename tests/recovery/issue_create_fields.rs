//! #866: the New Issue composer picks labels and assignees through the field
//! picker, shows who it posts as, and sends the picks with `gh issue create`.
//! A label the repository no longer has is refused before `gh issue create`
//! runs, and the composer keeps everything that was typed and picked.

use std::path::Path;

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::modals::{FieldTarget, PrField};
use kagi::ui::{e2e, KagiApp};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};

use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use crate::pr_fields_focus::OfflineGh;
use crate::recovery_operations::wait_idle;

const BODY: &str = "Export fails on large repositories\n";

/// Labels come from `labels.json`, so a test can delete one between the pick
/// and the Create; the viewer login from `login.txt`, absent until the test
/// writes it. `issue create` records its argv and consumes the body.
fn install_gh(dir: &Path) -> OfflineGh {
    let labels = dir.join("labels.json");
    let login = dir.join("login.txt");
    let argv = dir.join("argv.txt");
    OfflineGh::with_script(&format!(
        r#"#!/bin/sh
case "$1 $2" in
  'label list') cat '{labels}' ;;
  'api user') cat '{login}' ;;
  'api '*) echo octocat; echo hubot ;;
  'issue create') printf '%s\n' "$@" > '{argv}'; cat > /dev/null
    echo https://example.invalid/example/fixture/issues/9 ;;
  *) echo 'no GitHub repository here' >&2; exit 1 ;;
esac
"#,
        labels = labels.display(),
        login = login.display(),
        argv = argv.display(),
    ))
}

fn write_labels(dir: &Path, names: &[&str]) {
    let json: Vec<String> = names
        .iter()
        .map(|name| format!(r#"{{"name":"{name}","color":"d73a4a"}}"#))
        .collect();
    std::fs::write(dir.join("labels.json"), format!("[{}]", json.join(","))).unwrap();
}

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("draw Issues window");
}

fn drawn(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) -> bool {
    e2e::clear_control_bounds(window.window_id(), id);
    paint(cx, window);
    e2e::control_bounds(window.window_id(), id).is_some()
}

fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    e2e::clear_control_bounds(window.window_id(), id);
    paint(cx, window);
    let bounds = e2e::control_bounds(window.window_id(), id)
        .unwrap_or_else(|| panic!("{id} was not laid out"));
    cx.simulate_mouse_move(window, bounds.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

/// #904 review: open a field picker with the keyboard only. Focus starts on
/// the title; `hops` steps of the window's tab order (what Tab does — the
/// body is a code editor that keeps Tab for indentation) land on the field's
/// Button, and a real `key` press — down, then up, which is when GPUI turns
/// it into a click — activates it. Escape closes the picker.
fn keyboard_open(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &Entity<KagiApp>,
    hops: usize,
    key: &str,
    field: PrField,
) {
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_issue_title_for_e2e(window, cx));
        for _ in 0..hops {
            window.focus_next(cx);
        }
    })
    .unwrap();
    // Keyboard activation is wired for the element focused when it painted.
    paint(cx, window);
    let keystroke = gpui::Keystroke::parse(key).unwrap();
    cx.dispatch_keystroke(window, keystroke.clone());
    cx.simulate_event(window, gpui::KeyUpEvent { keystroke });
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app
            .read(cx)
            .pr_fields_modal()
            .unwrap_or_else(|| panic!("{key} on the {field:?} entry opened no picker"));
        assert_eq!(modal.target, FieldTarget::NewIssue);
        assert_eq!(modal.field, field);
    });
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).pr_fields_modal().is_none()));
}

/// Open one field's picker from the composer, wait for the repository's
/// list, toggle `values`, and Apply.
fn pick(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &Entity<KagiApp>,
    open: &str,
    field: PrField,
    values: &[&str],
) {
    click(cx, window, open);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        let ready = cx.read(|cx| {
            let modal = app
                .read(cx)
                .pr_fields_modal()
                .unwrap_or_else(|| panic!("{open} opened no picker"));
            assert_eq!(modal.target, FieldTarget::NewIssue);
            assert_eq!(modal.field, field);
            assert!(modal.error.is_none(), "{:?}", modal.error);
            modal.candidates.is_some()
        });
        if ready {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the candidate read never landed"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    for value in values {
        app.update(cx, |app, cx| app.pr_fields_toggle((*value).into(), cx));
    }
    click(cx, window, "pr-fields-confirm");
    assert!(cx.read(|cx| app.read(cx).pr_fields_modal().is_none()));
}

/// Newest first.
fn issue_creates(repo: &Path) -> Vec<OpOutcome> {
    read_oplog_tail_for_repo(repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "issue-create")
        .map(|entry| entry.outcome)
        .collect()
}

fn fields(app: &Entity<KagiApp>, cx: &mut VisualTestAppContext) -> (Vec<String>, Vec<String>) {
    cx.read(|cx| {
        let fields = app.read(cx).issue_create_fields_for_e2e();
        (fields.labels, fields.assignees)
    })
}

pub fn scenario_issue_create_fields(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let gh_dir = tempfile::tempdir().unwrap();
    write_labels(gh_dir.path(), &["bug", "docs", "gone"]);
    let gh = install_gh(gh_dir.path());
    let argv = gh_dir.path().join("argv.txt");
    let (app, window) = mount(cx, &repo);

    app.update(cx, |app, cx| {
        app.seed_issue_composer_for_e2e(cx);
        app.show_issues_mode(cx);
    });
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.insert_issue_body_for_e2e(BODY, window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();

    // The repository's host has no login yet: nobody is claimed as the
    // author, whatever the window-global login says.
    app.update(cx, |app, _| app.github_login = Some("someone-else".into()));
    assert!(drawn(cx, window, "issue-field-value-labels"));
    assert!(drawn(cx, window, "issue-field-value-assignees"));
    assert!(!drawn(cx, window, "issue-composer-posted-as"));

    // The host's own login arrives with a later Issues read.
    std::fs::write(gh_dir.path().join("login.txt"), "octocat\n").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while cx.read(|cx| app.read(cx).github_host_logins.get(&None).is_none()) {
        assert!(
            std::time::Instant::now() < deadline,
            "the host login was never read"
        );
        app.update(cx, |app, cx| app.refresh_github_issues(cx));
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        cx.read(|cx| app.read(cx).github_host_logins.get(&None).cloned()),
        Some("octocat".into())
    );
    assert!(
        drawn(cx, window, "issue-composer-posted-as"),
        "the composer says who the issue is posted as"
    );

    // Keyboard only: the entries are tab stops right after the body, and
    // Enter / Space open their pickers.
    keyboard_open(cx, window, &app, 2, "enter", PrField::Labels);
    keyboard_open(cx, window, &app, 3, "space", PrField::Assignees);

    pick(
        cx,
        window,
        &app,
        "issue-field-open-labels",
        PrField::Labels,
        &["bug", "gone"],
    );
    pick(
        cx,
        window,
        &app,
        "issue-field-open-assignees",
        PrField::Assignees,
        &["hubot"],
    );
    assert_eq!(
        fields(&app, cx),
        (vec!["bug".into(), "gone".into()], vec!["hubot".into()]),
        "Apply stores the picks in the composer"
    );
    assert!(
        issue_creates(&repo).is_empty(),
        "picking is not a write; nothing reaches the oplog before Create"
    );

    // The label is deleted on GitHub between the pick and the Create.
    write_labels(gh_dir.path(), &["bug", "docs"]);
    click(cx, window, "issue-composer-submit");
    wait_idle(cx, &app);
    assert!(
        !argv.exists(),
        "an unknown label must stop the write before gh issue create runs"
    );
    let outcomes = issue_creates(&repo);
    assert!(
        matches!(outcomes.as_slice(), [OpOutcome::Refused { blockers }]
            if blockers.iter().any(|b| b.contains("gone"))),
        "the refusal is the receipt: {outcomes:?}"
    );
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            state.toast_stack.as_ref().is_some_and(|stack| stack
                .read(cx)
                .toasts()
                .iter()
                .any(|toast| toast.message.contains("gone"))),
            "the toast names the label"
        );
        assert_eq!(
            state.issue_composer_snapshot_for_e2e().0.body,
            BODY,
            "a refusal keeps the body"
        );
    });
    assert_eq!(
        fields(&app, cx),
        (vec!["bug".into(), "gone".into()], vec!["hubot".into()]),
        "a refusal keeps the picks"
    );

    // Drop the deleted label and create: the picks travel as flags.
    pick(
        cx,
        window,
        &app,
        "issue-field-open-labels",
        PrField::Labels,
        &["gone"],
    );
    click(cx, window, "issue-composer-submit");
    wait_idle(cx, &app);
    let sent: Vec<String> = std::fs::read_to_string(&argv)
        .expect("gh issue create ran")
        .lines()
        .map(str::to_owned)
        .collect();
    let flags: Vec<&str> = sent
        .iter()
        .skip_while(|arg| *arg != "-")
        .skip(1)
        .map(String::as_str)
        .collect();
    assert_eq!(flags, ["--label", "bug", "--assignee", "hubot"], "{sent:?}");
    assert!(
        matches!(
            issue_creates(&repo).first(),
            Some(OpOutcome::Success { .. })
        ),
        "the create succeeded"
    );
    assert_eq!(
        fields(&app, cx),
        (Vec::new(), Vec::new()),
        "a created issue leaves an empty composer"
    );

    drop(gh);
    assert_eq!(repo_fingerprint(&repo), before, "repository mutated");
    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS issue_create_fields: picks reach gh issue create; an unknown label is refused before gh and keeps the composer"
    );
}
