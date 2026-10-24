//! Stash visible/AX identity and read-only activation (#1303).
use std::path::Path;

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::list_a11y::recorded_list;
use kagi::ui::sidebar::SidebarRow;
use kagi::ui::{CompareTarget, KagiApp};
use kagi_ui_core::i18n::Lang;

use super::support::*;
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, mount, unmount};

const STASHES: usize = 4;
const MESSAGE: &str = "same parked work 同じ退避メッセージ";

fn stash_labels() -> Vec<(String, (usize, Option<bool>, usize, usize))> {
    let tree = recorded_list("sidebar-stashes").expect("stash Tree drawn");
    assert_eq!(tree.role, Some(gpui::Role::Tree));
    tree.rows
        .iter()
        .filter_map(|(ix, (label, _))| {
            (tree.tree[ix].0 == 2).then(|| (label.clone(), tree.tree[ix]))
        })
        .collect()
}

fn stash_key(app: &KagiApp, oid: &str) -> String {
    app.sidebar
        .rows
        .iter()
        .find_map(|row| match row {
            SidebarRow::Stash { target, .. } if target.0 == oid => {
                Some(format!("stash:{}", target.0))
            }
            _ => None,
        })
        .expect("stash identity in rows")
}

fn push_stash(repo: &Path, content: &str) -> String {
    std::fs::write(repo.join("README.md"), content).unwrap();
    git(repo, &["stash", "push", "-qm", MESSAGE]);
    output(repo, &["rev-parse", "refs/stash"])
}

fn assert_stash_labels(count: usize) {
    let labels = stash_labels();
    assert_eq!(labels.len(), count, "all same-message stashes are drawn");
    let distinct: std::collections::HashSet<_> = labels.iter().map(|(label, _)| label).collect();
    assert_eq!(
        distinct.len(),
        count,
        "AX distinguishes every equal message by index"
    );
    for (index, (label, spec)) in labels.iter().enumerate() {
        assert!(
            label.contains(&format!("stash@{{{index}}}")),
            "index missing: {label}"
        );
        assert!(label.contains(MESSAGE), "message missing: {label}");
        assert_eq!(*spec, (2, None, index + 1, count));
    }
}

fn assert_peek(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, oid: &str) {
    cx.read(|cx| {
        let app = app.read(cx);
        let compare = app
            .ui()
            .compare_view
            .as_ref()
            .expect("Enter opens Compare")
            .read(cx)
            .view();
        assert_eq!(
            compare.target,
            CompareTarget::Commit(kagi_git::CommitId(oid.to_owned()))
        );
        assert_eq!(compare.title.as_ref(), "stash@{3}");
        assert!(compare
            .files
            .iter()
            .any(|file| file.path.to_string_lossy() == "README.md"));
        assert!(app.stash_drop_modal().is_none() && app.stash_apply_modal().is_none());
        assert!(
            app.pop_modal().is_none(),
            "peek must never offer consumption"
        );
    });
}

fn menu_peek(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    key: &str,
    oid: &str,
) {
    app.update(cx, |app, cx| {
        app.close_compare_view();
        cx.notify();
    });
    focus(cx, app, window, STASHES, key);
    keys(cx, window, "shift-f10");
    paint(cx, app, window);
    app.read_with(cx, |app, _| {
        let menu = app
            .stash_menu
            .as_ref()
            .expect("focused stash opens its context menu");
        assert_eq!(menu.index, 3, "menu follows focused OID at its new index");
        assert!(menu.message.contains(MESSAGE));
    });
    let (item, enabled) = cx
        .update_window(window, |_, window, cx| {
            app.read(cx).menu_keys_for_e2e(window)
        })
        .unwrap();
    assert_eq!(
        item,
        enabled.first().copied(),
        "Peek is the focused first command"
    );
    assert_eq!(item, Some(0));
    keys(cx, window, "enter");
    paint(cx, app, window);
    assert!(app.read_with(cx, |app, _| app.stash_menu.is_none()));
    assert_peek(cx, app, oid);
    assert_eq!(focused(cx, app, window), Some((STASHES, key.to_owned())));
}

fn stash_cell(cx: &mut VisualTestAppContext, language: Lang, slug: &str, zoom: f32) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let original = push_stash(repo, "first distinct stash content\n");
    let newer = push_stash(repo, "second distinct stash content\n");
    assert_ne!(original, newer);
    let third = push_stash(repo, "third distinct stash content\n");
    assert_ne!(third, original);
    assert_ne!(third, newer);
    let (app, window) = mount(cx, repo);
    appearance(cx, &app, window, language, slug, zoom, STASHES);
    assert_stash_labels(3);
    let key = app.read_with(cx, |app, _| stash_key(app, &original));
    focus(cx, &app, window, STASHES, &key);
    push_stash(repo, "fourth distinct stash content\n");
    app.update(cx, |app, cx| app.reload(cx));
    cx.run_until_parked();
    paint(cx, &app, window);
    assert_stash_labels(4);
    assert_eq!(
        focused(cx, &app, window),
        Some((STASHES, key.clone())),
        "focus follows OID after index shift"
    );
    let before = Witness::read(repo);
    keys(cx, window, "enter");
    paint(cx, &app, window);
    assert_peek(cx, &app, &original);
    assert_eq!(focused(cx, &app, window), Some((STASHES, key.clone())));
    assert_eq!(
        Witness::read(repo),
        before,
        "Enter peek never consumes or writes"
    );
    keys(cx, window, "space");
    paint(cx, &app, window);
    assert_peek(cx, &app, &original);
    assert_eq!(focused(cx, &app, window), Some((STASHES, key.clone())));
    assert_eq!(
        Witness::read(repo),
        before,
        "Space peek never consumes or writes"
    );
    menu_peek(cx, &app, window, &key, &original);
    assert_eq!(
        Witness::read(repo),
        before,
        "context Peek never consumes or writes"
    );
    assert_eq!(output(repo, &["stash", "list"]).lines().count(), 4);
    unmount(cx, app, window);
}

pub fn scenario_stash_row_identity(cx: &mut VisualTestAppContext) {
    let _settings = Settings::capture();
    for language in [Lang::En, Lang::Ja] {
        for slug in ["apple-light", "apple-dark"] {
            for zoom in [1., 1.67] {
                stash_cell(cx, language, slug, zoom);
            }
        }
    }
}
