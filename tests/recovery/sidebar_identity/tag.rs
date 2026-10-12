//! Loaded, paged and unreadable tag navigation (#1304).
use std::path::{Path, PathBuf};

use gpui::{AnyWindowHandle, Entity, Modifiers, MouseButton, VisualTestAppContext};
use kagi::ui::{build_tab_view, e2e, KagiApp, ToastKind, COMMIT_PAGE_STEP};
use kagi_ui_core::i18n::Lang;

use super::support::*;
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, unmount};
use crate::recovery_layout::contained;

const TAGS: usize = 3;
const TAG: &str = "old-release-長い名前";

/// Corrupt-object fixtures cannot use history-reading Git probes. Record raw
/// repository bytes instead, exempting only index stat-cache bytes while still
/// comparing its staged entries.
fn raw_repo_files(repo: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn collect(root: &Path, at: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in std::fs::read_dir(at).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                collect(root, &path, files);
            } else if path != root.join(".git/index") {
                files.push((
                    path.strip_prefix(root).unwrap().to_owned(),
                    std::fs::read(path).unwrap(),
                ));
            }
        }
    }
    let mut files = Vec::new();
    collect(repo, repo, &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

/// The seam lives only here: install a real backend snapshot at a small limit,
/// equivalent to initial paging without manufacturing commits or sidebar rows.
fn truncate_history(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    repo: &Path,
) -> kagi_git::RepoSnapshot {
    let snapshot = kagi_git::Backend::open(repo).unwrap().snapshot(2).unwrap();
    install_history_page(cx, app, repo, &snapshot);
    snapshot
}

fn install_history_page(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    repo: &Path,
    snapshot: &kagi_git::RepoSnapshot,
) {
    app.update(cx, |app, cx| {
        app.ui_mut().unwrap().commit_limit = 2;
        app.amend_tab_view(
            app.active_session().unwrap(),
            build_tab_view(snapshot, "fixture"),
        );
        let tip = app
            .view()
            .commit_row_index
            .get(&kagi_git::CommitId(output(repo, &["rev-parse", "HEAD"])))
            .copied();
        app.select(tip.expect("tip loaded"));
        cx.notify();
    });
}

fn reason(language: Lang) -> &'static str {
    match language {
        Lang::En => "Commit not loaded; load more history.",
        Lang::Ja => "commit が未読込みです。履歴を追加で読み込んでください。",
    }
}

fn activate_tag(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    keyboard: bool,
) {
    if keyboard {
        focus(cx, app, window, TAGS, &format!("tag:{TAG}"));
        keys(cx, window, "enter");
    } else {
        let at = bounds(window, &format!("sidebar-tag-{TAG}")).center();
        cx.simulate_mouse_down(window, at, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(window, at, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
    }
}

fn assert_tag_feedback(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    language: Lang,
    selection: Option<usize>,
    generation: u64,
) {
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            app.ui().selected,
            selection,
            "missing target does not jump or auto-jump"
        );
        assert_eq!(
            app.ui().commit_limit,
            2 + COMMIT_PAGE_STEP,
            "activation invokes existing pagination"
        );
        assert_eq!(
            app.ui().load_more_gen,
            generation + 1,
            "one pagination request"
        );
        let toast = app
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .last()
            .cloned()
            .expect("missing target feedback");
        assert_eq!(toast.kind, ToastKind::Info);
        assert_eq!(toast.message.as_ref(), reason(language));
    });
}

fn tag_metadata(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    language: Lang,
    loaded: bool,
) {
    let id = format!("sidebar-tag-{TAG}");
    let tokens = kagi_ui_core::theme::theme();
    let color = if loaded {
        tokens.color_tag
    } else {
        tokens.text_muted
    };
    let paint = e2e::control_paint(window.window_id(), &id).expect("tag actually painted");
    assert_eq!(paint.text_color, gpui::rgb(color).into());
    let tooltip = hover(cx, app, window, &id, bounds(window, &id).center());
    let expected = if loaded {
        TAG.to_owned()
    } else {
        format!("{TAG}\n{}", reason(language))
    };
    assert_eq!(
        tooltip, expected,
        "actual tooltip follows target availability"
    );
    let description = if loaded { None } else { Some(reason(language)) };
    assert_eq!(
        e2e::sidebar_labels::recorded_description(&id).as_deref(),
        description
    );
}

fn rendered_toast(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    kind: ToastKind,
    text: &str,
    dimensions: (f32, f32),
) {
    let id = cx.read(|cx| {
        app.read(cx)
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .iter()
            .rev()
            .find(|toast| toast.kind == kind && toast.message.contains(text))
            .expect("specific feedback toast")
            .id
    });
    let label = bounds(window, &format!("toast-message-{id}"));
    assert!(label.size.width > gpui::px(0.) && label.size.height > gpui::px(0.));
    let viewport = gpui::Bounds::new(
        gpui::point(gpui::px(0.), gpui::px(0.)),
        gpui::size(gpui::px(dimensions.0), gpui::px(dimensions.1)),
    );
    contained(viewport, label, "drawn tag feedback");
}

fn loaded_tag(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    language: Lang,
    target: &kagi_git::CommitId,
    keyboard: bool,
) {
    let row = app
        .read_with(cx, |app, _| {
            app.view().commit_row_index.get(target).copied()
        })
        .expect("paging really loads target");
    tag_metadata(cx, app, window, language, true);
    let generation = app.read_with(cx, |app, _| app.ui().load_more_gen);
    activate_tag(cx, app, window, keyboard);
    paint(cx, app, window);
    assert_eq!(
        app.read_with(cx, |app, _| app.ui().selected),
        Some(row),
        "loaded target jumps"
    );
    assert_eq!(
        app.read_with(cx, |app, _| app.ui().load_more_gen),
        generation
    );
}

fn failed_read_state(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    language: Lang,
    target: &kagi_git::CommitId,
    generation: u64,
) {
    cx.read(|cx| {
        let app = app.read(cx);
        let toasts = app.toast_stack.as_ref().unwrap().read(cx).toasts();
        assert!(toasts.iter().any(
            |toast| toast.kind == ToastKind::Info && toast.message.as_ref() == reason(language)
        ));
        assert_eq!(app.ui().commit_limit, 2 + COMMIT_PAGE_STEP);
        assert_eq!(app.ui().load_more_gen, generation + 1);
        assert!(!app.view().commit_row_index.contains_key(target));
        assert!(app.tag_menu.is_none() && !e2e::active_modal_present(app));
    });
}

fn tag_cell(
    cx: &mut VisualTestAppContext,
    language: Lang,
    slug: &str,
    zoom: f32,
    dimensions: (f32, f32),
) {
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["tag", TAG, "HEAD~1"]);
    for index in 0..5 {
        git(
            repo,
            &[
                "commit",
                "--allow-empty",
                "-qm",
                &format!("new history {index}"),
            ],
        );
    }
    let target = kagi_git::CommitId(output(repo, &["rev-parse", TAG]));
    let (app, window) = mount_dimensions(cx, repo, dimensions);
    appearance(cx, &app, window, language, slug, zoom, TAGS);
    let before = Witness::read(repo);
    assert!(kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).is_empty());
    for keyboard in [false, true] {
        truncate_history(cx, &app, repo);
        paint(cx, &app, window);
        assert!(app.read_with(cx, |app, _| !app
            .view()
            .commit_row_index
            .contains_key(&target)));
        tag_metadata(cx, &app, window, language, false);
        let (selection, generation) =
            app.read_with(cx, |app, _| (app.ui().selected, app.ui().load_more_gen));
        activate_tag(cx, &app, window, keyboard);
        assert_tag_feedback(cx, &app, language, selection, generation);
        cx.run_until_parked();
        paint(cx, &app, window);
        rendered_toast(
            cx,
            &app,
            window,
            ToastKind::Info,
            reason(language),
            dimensions,
        );
        assert_eq!(app.read_with(cx, |app, _| app.ui().selected), selection);
        loaded_tag(cx, &app, window, language, &target, keyboard);
        assert_eq!(
            Witness::read(repo),
            before,
            "tag feedback, paging and jump never write"
        );
    }
    unmount(cx, app, window);
}

fn tag_read_failure(
    cx: &mut VisualTestAppContext,
    language: Lang,
    slug: &str,
    zoom: f32,
    dimensions: (f32, f32),
) {
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["tag", TAG, "HEAD~1"]);
    for index in 0..5 {
        git(
            repo,
            &[
                "commit",
                "--allow-empty",
                "-qm",
                &format!("failure history {index}"),
            ],
        );
    }
    let target = kagi_git::CommitId(output(repo, &["rev-parse", TAG]));
    let (app, window) = mount_dimensions(cx, repo, dimensions);
    appearance(cx, &app, window, language, slug, zoom, TAGS);
    let snapshot = truncate_history(cx, &app, repo);
    paint(cx, &app, window);
    // Sabotage a real cached tag's referenced object, never fabricate a UI row.
    let object = repo
        .join(".git/objects")
        .join(&target.0[..2])
        .join(&target.0[2..]);
    std::fs::remove_file(object).expect("fixture target is a real loose object");
    assert!(
        !std::process::Command::new("git")
            .current_dir(repo)
            .args(["cat-file", "-e", &target.0])
            .output()
            .unwrap()
            .status
            .success(),
        "fixture target really is unreadable"
    );
    let bytes = raw_repo_files(repo);
    let index = output(repo, &["ls-files", "--stage"]);
    let receipts = kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len();
    assert_eq!(receipts, 0);
    for keyboard in [false, true] {
        install_history_page(cx, &app, repo, &snapshot);
        paint(cx, &app, window);
        tag_metadata(cx, &app, window, language, false);
        let generation = app.read_with(cx, |app, _| app.ui().load_more_gen);
        activate_tag(cx, &app, window, keyboard);
        cx.run_until_parked();
        paint(cx, &app, window);
        failed_read_state(cx, &app, language, &target, generation);
        rendered_toast(
            cx,
            &app,
            window,
            ToastKind::Info,
            reason(language),
            dimensions,
        );
        assert_eq!(
            raw_repo_files(repo),
            bytes,
            "read failure never writes repository bytes"
        );
        assert_eq!(output(repo, &["ls-files", "--stage"]), index);
        assert_eq!(
            kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len(),
            receipts
        );
    }
    unmount(cx, app, window);
}

/// Each registered runner cell owns one appearance/viewport and both fixture
/// legs, keeping KEEP_GOING deadlines bounded without reducing the matrix.
pub fn scenario_tag_unloaded_target_cell(
    cx: &mut VisualTestAppContext,
    language: Lang,
    slug: &str,
    zoom: f32,
    dimensions: (f32, f32),
) {
    let _settings = Settings::capture();
    tag_cell(cx, language, slug, zoom, dimensions);
    tag_read_failure(cx, language, slug, zoom, dimensions);
}

pub fn scenario_tag_unloaded_target(cx: &mut VisualTestAppContext) {
    scenario_tag_unloaded_target_cell(cx, Lang::En, "apple-light", 1., (940., 660.));
}
