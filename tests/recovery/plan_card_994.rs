//! #994: the renderer's headings, collapsed shell commands, and AX recovery.
use gpui::{AnyWindowHandle, Bounds, Entity, Pixels, VisualTestAppContext};
use kagi::ui::{dialog_a11y, e2e, KagiApp};
use kagi_domain::plan_note::ShellKind;
use kagi_git::CommitId;
use kagi_ui_core::i18n::{self, Lang, Msg};

use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};

#[path = "../support/git_fixture.rs"]
mod git_fixture;

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear()
    })
    .unwrap();
}

fn bounds(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    id: &str,
) -> Option<Bounds<Pixels>> {
    e2e::clear_control_bounds(window.window_id(), id);
    paint(cx, window);
    e2e::control_bounds(window.window_id(), id)
}

fn ready_push(
    cx: &mut VisualTestAppContext,
) -> (
    tempfile::TempDir,
    tempfile::TempDir,
    Entity<KagiApp>,
    AnyWindowHandle,
) {
    let fixture = build_fixture();
    let remote = tempfile::tempdir().unwrap();
    let repo = fixture.path();
    git(
        repo,
        &["init", "--bare", "-q", remote.path().to_str().unwrap()],
    );
    git(
        repo,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(repo, &["commit", "-q", "--allow-empty", "-m", "ahead"]);
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_push_modal(cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).push_modal().unwrap().plan.blockers.is_empty()));
    (fixture, remote, app, window)
}

fn assert_heading(cx: &mut VisualTestAppContext, window: AnyWindowHandle, title: &str) {
    dialog_a11y::clear_recorded_a11y();
    let icon = bounds(cx, window, "plan-heading-icon").expect("inline heading icon");
    let heading = bounds(cx, window, "plan-heading-title").expect("short heading");
    let chip = bounds(cx, window, "plan-heading-chip-0").expect("target chip");
    let copy = bounds(cx, window, "plan-card-copy").expect("Copy all button");
    let card = bounds(cx, window, "modal-card").expect("plan card");
    assert!(
        icon.size.width <= gpui::px(20.) && icon.size.height <= gpui::px(20.),
        "{icon:?}"
    );
    assert!(
        heading.size.height < gpui::px(32.),
        "heading stays on one line: {heading:?}"
    );
    for part in [icon, heading, chip, copy] {
        assert!(
            part.left() >= card.left() && part.right() <= card.right(),
            "heading part within card: {part:?} / {card:?}"
        );
    }
    assert!(
        icon.right() <= heading.left()
            && heading.right() <= chip.left()
            && chip.right() <= copy.left(),
        "heading order"
    );
    assert_eq!(
        dialog_a11y::recorded_dialog("plan-card").unwrap().label,
        title
    );
}

pub fn scenario_plan_card_heading(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let (fixture, _remote, app, window) = ready_push(cx);
    let repo = fixture.path();
    git(repo, &["branch", "heading-target", "HEAD~1"]);
    let before = repo_fingerprint(repo);
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        let push_title =
            cx.read(|cx| i18n::plan_title_text(&app.read(cx).push_modal().unwrap().plan.title));
        assert_heading(cx, window, &push_title);
        app.update(cx, |app, _| app.clear_push_modal());
        app.update(cx, |app, _| app.open_plan_modal("heading-target"));
        cx.run_until_parked();
        let checkout_title =
            cx.read(|cx| i18n::plan_title_text(&app.read(cx).plan_modal().unwrap().plan.title));
        assert_heading(cx, window, &checkout_title);
        app.update(cx, |app, _| app.clear_plan_modal());
        app.update(cx, |app, cx| app.open_push_modal(cx));
        cx.run_until_parked();
    }
    assert_eq!(repo_fingerprint(repo), before);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS plan_card_heading: inline 18px icon, short title, typed chip, full AX name in EN/JA");
}

pub fn scenario_plan_recovery_commands(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let (fixture, _remote, app, window) = ready_push(cx);
    let before = repo_fingerprint(fixture.path());
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        let (commands, prose) = cx.read(|cx| {
            let plan = &app.read(cx).push_modal().unwrap().plan;
            let rec = plan.recovery.as_ref().unwrap();
            (
                rec.commands_for(ShellKind::current()).join("\n"),
                i18n::plan_recovery_text(Some(rec)),
            )
        });
        assert!(!commands.is_empty());
        assert!(
            bounds(cx, window, "plan-recovery-body").is_none(),
            "disclosure starts closed in {lang:?}; state {:?}",
            e2e::recorded_plan_command_disclosure("plan-recovery")
        );
        assert!(
            bounds(cx, window, "plan-recovery-scroll").is_none(),
            "prose never occupies the body"
        );
        let row = bounds(cx, window, "plan-recovery").expect("Ready recovery disclosure");
        assert!(
            row.size.height < gpui::px(40.),
            "one collapsed row: {row:?}"
        );
        let (_, ax, expanded) = e2e::recorded_plan_command_disclosure("plan-recovery").unwrap();
        assert!(!expanded && ax.starts_with(Msg::ModalRecoveryCommands.t()));
        let copy = bounds(cx, window, "plan-recovery-copy").unwrap();
        cx.simulate_click(window, copy.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some(commands.clone())
        );
        assert!(
            bounds(cx, window, "plan-recovery-body").is_none(),
            "Copy does not expand"
        );
        let all = bounds(cx, window, "plan-card-copy").unwrap();
        cx.simulate_click(window, all.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        let copied = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap();
        assert!(copied.contains(&prose) && copied.contains(Msg::ModalRecoveryCommands.t()));
        for command in commands.lines() {
            assert!(copied.contains(command), "Copy all keeps {command}");
        }
        let row = bounds(cx, window, "plan-recovery").unwrap();
        cx.simulate_click(window, row.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        assert!(
            bounds(cx, window, "plan-recovery-body").is_some(),
            "expansion reveals commands"
        );
        let (_, _, expanded) = e2e::recorded_plan_command_disclosure("plan-recovery").unwrap();
        assert!(expanded);
        let open_row = bounds(cx, window, "plan-recovery").unwrap();
        cx.simulate_click(window, open_row.center(), gpui::Modifiers::none());
        cx.run_until_parked();
    }
    assert_eq!(repo_fingerprint(fixture.path()), before);
    unmount(cx, app, window);

    // A backend-supplied recovery command must not read as executable when
    // the corresponding plan is blocked.
    let blocked = build_fixture();
    let repo = blocked.path();
    let target = CommitId(git_fixture::git_output(repo, &["rev-parse", "HEAD~1"]));
    git(repo, &["checkout", "-q", "--detach", "HEAD"]);
    let before = repo_fingerprint(repo);
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_reset_current_modal(target, cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| !app
        .read(cx)
        .reset_current_modal()
        .unwrap()
        .plan
        .blockers
        .is_empty()));
    for id in ["plan-recovery", "plan-recovery-copy", "plan-recovery-body"] {
        assert!(
            bounds(cx, window, id).is_none(),
            "blocked plan must hide {id}"
        );
    }
    assert_eq!(repo_fingerprint(repo), before);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS plan_recovery_commands: Ready collapsed, exact shell copy and expansion, EN/JA");
}

pub fn scenario_plan_recovery_ax(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let (fixture, _remote, app, window) = ready_push(cx);
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        dialog_a11y::clear_recorded_a11y();
        paint(cx, window);
        let prose = cx.read(|cx| {
            i18n::plan_recovery_text(app.read(cx).push_modal().unwrap().plan.recovery.as_ref())
        });
        assert_eq!(
            dialog_a11y::recorded_dialog("plan-card")
                .unwrap()
                .description
                .as_deref(),
            Some(prose.as_str())
        );
        assert!(bounds(cx, window, "plan-recovery-scroll").is_none());
        let copy = bounds(cx, window, "plan-card-copy").unwrap();
        cx.simulate_click(window, copy.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        assert!(cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap()
            .contains(&prose));
    }
    unmount(cx, app, window);
    drop(fixture);
    eprintln!(
        "[gui-e2e] PASS plan_recovery_ax: EN/JA recovery prose in AX and Copy all, never card body"
    );
}

pub fn scenario_plan_equivalent_summary(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let (_fixture, _remote, app, window) = ready_push(cx);
    let command = cx.read(|cx| {
        app.read(cx)
            .push_modal()
            .unwrap()
            .plan
            .equivalent_command
            .clone()
            .unwrap()
    });
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        assert!(bounds(cx, window, "plan-equivalent-command").is_some());
        let (summary, name, expanded) =
            e2e::recorded_plan_command_disclosure("plan-equivalent-command").unwrap();
        assert_eq!(
            summary, command,
            "visible summary is the mono command, not a sentence"
        );
        assert_eq!(
            name,
            format!("{} {command}", Msg::ModalEquivalentCommand.t())
        );
        assert!(!expanded);
    }
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS plan_equivalent_summary: reusable disclosure shows bare command with localized equivalent AX name");
}
