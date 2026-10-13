//! Native PR detail fixture, focus traversal and renderer probes.
use super::{APPLY, COPY, DRAFT, HUNK};
use crate::macos::{build_fixture, git, unmount};
use crate::pr_fields_focus::OfflineGh;
use crate::pr_viewed::{pr_at, push_pr_head, wait_loaded};
use crate::recovery_layout::{contained, draw, GlobalSettings};
use gpui::{
    point, px, size, AnyWindowHandle, ElementId, Entity, FocusHandle, Modifiers, Role,
    VisualTestAppContext,
};
use kagi::ui::{
    button_style::ButtonRole,
    e2e::{self, pr_detail},
    i18n::{Lang, Msg},
    KagiApp,
};
use kagi_ui_core::{i18n, theme};
use std::{cell::RefCell, rc::Rc};

pub(crate) struct Cell {
    pub(crate) app: Entity<KagiApp>,
    pub(crate) win: AnyWindowHandle,
    pub(crate) dimensions: (f32, f32),
}
impl Cell {
    pub(crate) fn paint(&self, cx: &mut VisualTestAppContext) {
        pr_detail::clear_controls();
        for _ in 0..3 {
            cx.run_until_parked();
            draw(cx, self.win, self.dimensions);
        }
    }

    pub(crate) fn bounds(&self, id: &str) -> gpui::Bounds<gpui::Pixels> {
        e2e::control_bounds(self.win.window_id(), id).unwrap_or_else(|| panic!("missing {id}"))
    }

    pub(super) fn button(&self, id: &str, msg: Msg) {
        let control =
            pr_detail::control(id).unwrap_or_else(|| panic!("missing AX assignment {id}"));
        assert_eq!(control.role, Some(Role::Button), "{id} role");
        assert_eq!(
            control.name.as_deref(),
            Some(msg.t().as_ref()),
            "{id} localized name"
        );
    }

    fn focus(&self, cx: &mut VisualTestAppContext, id: &str) -> FocusHandle {
        let element_id = match id {
            COPY => ElementId::from(("pr-convo-hunk-copy", 7000usize)),
            APPLY => ElementId::from(("pr-convo-apply-suggestion", 7000usize)),
            _ => ElementId::Name(id.to_owned().into()),
        };
        cx.update_window(self.win, |_, window, cx| {
            let root = self.app.read(cx).root_focus.clone().unwrap();
            root.focus(window, cx);
        })
        .unwrap();
        for _ in 0..160 {
            cx.update_window(self.win, |_, window, cx| {
                window.focus_next(cx);
            })
            .unwrap();
            pr_detail::clear_key_focus();
            crate::keyboard_nav::keys(cx, self.win, "f19");
            if pr_detail::key_focus() == Some(element_id.clone()) {
                return cx
                    .update_window(self.win, |_, window, cx| window.focused(cx).unwrap())
                    .unwrap();
            }
        }
        panic!("{id} must be a real Tab stop");
    }

    pub(super) fn activate(
        &self,
        cx: &mut VisualTestAppContext,
        id: &str,
        geometry: &str,
        key: &str,
    ) {
        if matches!(id, COPY | APPLY) {
            self.reveal_review(cx, id);
        }
        let before = self.bounds(geometry);
        assert!(
            before.size.height >= px(20.) && before.size.width >= px(20.),
            "{id} hit target: {before:?}"
        );
        cx.simulate_mouse_move(self.win, before.center(), None, Modifiers::none());
        self.paint(cx);
        assert_eq!(self.bounds(geometry), before, "{id} hover changes geometry");
        self.focus(cx, id);
        crate::keyboard_nav::keys(cx, self.win, "f19");
        self.paint(cx);
        assert_eq!(self.bounds(geometry), before, "{id} focus changes geometry");
        let count = pr_detail::activation_count(id);
        crate::keyboard_nav::keys(cx, self.win, key);
        self.paint(cx);
        assert_eq!(
            pr_detail::activation_count(id),
            count + 1,
            "{id} {key} invokes once"
        );
    }

    pub(super) fn reveal_review(
        &self,
        cx: &mut VisualTestAppContext,
        id: &str,
    ) -> gpui::Bounds<gpui::Pixels> {
        for _ in 0..32 {
            e2e::clear_control_bounds(self.win.window_id(), id);
            self.paint(cx);
            let viewport = cx.read(|cx| {
                let mode = self.app.read(cx).pr_mode().unwrap();
                mode.tabs[mode.active.unwrap()].feed_list.viewport_bounds()
            });
            let bounds = e2e::control_bounds(self.win.window_id(), id);
            if let Some(bounds) = bounds {
                if bounds.top() >= viewport.top() && bounds.bottom() <= viewport.bottom() {
                    contained(viewport, bounds, id);
                    return bounds;
                }
            }
            let delta = bounds
                .map(|bounds| f32::from(viewport.center().y - bounds.center().y))
                .unwrap_or(-f32::from(viewport.size.height) / 2.);
            cx.simulate_event(
                self.win,
                gpui::ScrollWheelEvent {
                    position: viewport.center(),
                    delta: gpui::ScrollDelta::Pixels(point(px(0.), px(delta))),
                    touch_phase: gpui::TouchPhase::Moved,
                    ..Default::default()
                },
            );
        }
        panic!("{id} never becomes reachable in the real PR feed viewport");
    }

    pub(super) fn type_draft(&self, cx: &mut VisualTestAppContext) {
        cx.update_window(self.win, |_, window, cx| {
            let input = self.app.read(cx).pr_comment_input.clone().unwrap();
            input.update(cx, |input, cx| input.replace(DRAFT.to_owned(), window, cx));
        })
        .unwrap();
        self.paint(cx);
    }

    pub(super) fn draft(&self, cx: &mut VisualTestAppContext) -> (gpui::EntityId, String) {
        cx.read(|cx| {
            let input = self.app.read(cx).pr_comment_input.as_ref().unwrap();
            (input.entity_id(), input.read(cx).value().to_string())
        })
    }
}
pub(super) fn matrix(
    cx: &mut VisualTestAppContext,
    check: impl Fn(&mut VisualTestAppContext, &Cell, &std::path::Path),
) {
    matrix_langs(cx, &[Lang::En, Lang::Ja], check);
}
pub(super) fn matrix_langs(
    cx: &mut VisualTestAppContext,
    languages: &[Lang],
    check: impl Fn(&mut VisualTestAppContext, &Cell, &std::path::Path),
) {
    matrix_commit_count(cx, languages, 1, check);
}

pub(crate) fn matrix_commit_count(
    cx: &mut VisualTestAppContext,
    languages: &[Lang],
    count: usize,
    check: impl Fn(&mut VisualTestAppContext, &Cell, &std::path::Path),
) {
    let _restore = GlobalSettings::capture();
    let _saved_theme = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    let original_theme = theme::theme().slug.to_string();
    let _gh = OfflineGh::install();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let remote_dir = tempfile::tempdir().unwrap();
    let remote = remote_dir.path().join("repo.git");
    git(remote_dir.path(), &["init", "-q", "--bare", "repo.git"]);
    git(
        &repo,
        &[
            "push",
            "-q",
            remote.to_str().unwrap(),
            "main:refs/heads/main",
        ],
    );
    let url = "https://github.com/example/repo.git";
    git(&repo, &["remote", "add", "origin", url]);
    git(
        &repo,
        &[
            "config",
            &format!("url.file://{}.insteadOf", remote.display()),
            url,
        ],
    );
    let mut head = push_pr_head(&repo, &remote, "main", &[("s.txt", "one\ntwo\nthree\n")]);
    for ix in 1..count {
        let text = format!("one\ntwo\nthree\n{ix}\n");
        head = push_pr_head(&repo, &remote, &head, &[("s.txt", &text)]);
    }
    // Apply plans must be allowed to open against the actual head blob.
    git(&repo, &["checkout", "-q", "-b", "feature", &head]);
    for &lang in languages {
        i18n::set_lang(lang);
        for slug in ["apple-light", "apple-dark"] {
            for zoom in [1., 1.667] {
                theme::set_zoom(zoom);
                for dimensions in [(940., 660.), (1392., 883.)] {
                    let state = e2e::app_state(&repo).unwrap();
                    let captured = Rc::new(RefCell::new(None));
                    let out = captured.clone();
                    let win = crate::macos::open_offscreen(
                        cx,
                        size(px(dimensions.0), px(dimensions.1)),
                        move |w, cx| e2e::mount_root(state, w, cx, &out),
                    );
                    let app = captured.borrow().clone().unwrap();
                    drop(captured);
                    app.update(cx, |a, cx| a.set_theme(slug, cx));
                    e2e::queue_github_pr_conversation(gpui::Task::ready((
                        Ok((vec![], vec![])),
                        Ok(vec![kagi_domain::review_thread::ReviewThread {
                            path: "s.txt".into(),
                            line: Some(2),
                            original_line: Some(2),
                            comments: vec![kagi_domain::github::ReviewComment {
                                author: "reviewer".into(),
                                body: "shout it\n\n```suggestion\nTWO\n```\n".into(),
                                diff_hunk: HUNK.into(),
                                created_at: "2026-10-01T00:00:00Z".into(),
                                ..Default::default()
                            }],
                            ..Default::default()
                        }]),
                    )));
                    app.update(cx, |a, cx| a.pr_mode_open(&pr_at(&head), cx));
                    wait_loaded(cx, &app, &head);
                    let cell = Cell {
                        app,
                        win: win.into(),
                        dimensions,
                    };
                    cell.paint(cx);
                    check(cx, &cell, &repo);
                    cell.app
                        .update(cx, |a, cx| a.set_theme(&original_theme, cx));
                    unmount(cx, cell.app, cell.win);
                }
            }
        }
    }
}
pub(super) fn fill(cx: &mut VisualTestAppContext, cell: &Cell, id: &str) -> [u8; 4] {
    let bounds = cell.bounds(id);
    cx.update_window(cell.win, |_, window, _| {
        let image = window.render_to_image().expect("native submit paint");
        let scale = window.scale_factor();
        image
            .get_pixel(
                ((f32::from(bounds.left()) + 8.) * scale) as u32,
                (f32::from(bounds.center().y) * scale) as u32,
            )
            .0
    })
    .unwrap()
}
pub(super) fn settle_plan(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while cx.read(|cx| app.read(cx).planning.is_some()) {
        cx.run_until_parked();
        assert!(
            std::time::Instant::now() < deadline,
            "PR merge plan never settled"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    cx.run_until_parked();
}
pub(super) fn issue_undo(cx: &mut VisualTestAppContext, cell: &Cell, number: Option<u64>) {
    cx.update_window(cell.win, |_, window, cx| {
        cell.app
            .update(cx, |a, cx| a.focus_issue_body_for_e2e(number, window, cx))
    })
    .unwrap();
    let body = |cx: &mut VisualTestAppContext| {
        cx.read(|cx| {
            let app = cell.app.read(cx);
            match number {
                Some(number) => app.issue_reply_snapshot_for_e2e(number).body,
                None => app.issue_composer_snapshot_for_e2e().0.body,
            }
        })
    };
    crate::keyboard_nav::keys(cx, cell.win, "secondary-z");
    assert_eq!(
        body(cx),
        "",
        "mode/focus toggles preserve Issue undo history"
    );
    crate::keyboard_nav::keys(cx, cell.win, "secondary-shift-z");
    assert_eq!(body(cx), DRAFT, "Issue redo restores draft");
}
pub(super) fn disabled_submit(cx: &mut VisualTestAppContext, cell: &Cell, id: &str, msg: Msg) {
    cell.button(id, msg);
    let control = pr_detail::control(id).unwrap();
    assert!(control.disabled);
    assert_eq!(control.semantic, Some(ButtonRole::Secondary));
    let count = pr_detail::activation_count(id);
    cx.simulate_click(cell.win, cell.bounds(id).center(), Modifiers::none());
    cell.paint(cx);
    assert_eq!(
        pr_detail::activation_count(id),
        count,
        "disabled submit is inert"
    );
}
pub(super) fn enabled_submit(cx: &mut VisualTestAppContext, cell: &Cell, id: &str, msg: Msg) {
    cell.button(id, msg);
    assert_eq!(
        pr_detail::control(id).unwrap().semantic,
        Some(ButtonRole::Primary)
    );
    assert!(!pr_detail::control(id).unwrap().disabled);
    assert!(
        f32::from(cell.bounds(id).size.height - theme::scaled_px(36.)).abs() <= 0.5,
        "submit height preserves 36 reference px within native pixel rounding"
    );
    for key in ["enter", "space"] {
        cx.update_window(cell.win, |_, window, cx| {
            cell.app.update(cx, |a, cx| {
                if id == "issue-composer-submit" {
                    a.replace_issue_body_for_e2e(DRAFT, window, cx);
                } else {
                    a.replace_issue_reply_body_for_e2e(4, DRAFT, window, cx);
                }
            })
        })
        .unwrap();
        cell.paint(cx);
        cell.activate(cx, id, id, key);
        settle_write(cx, &cell.app);
        cell.paint(cx);
        let body = cx.read(|cx| {
            let app = cell.app.read(cx);
            if id == "issue-composer-submit" {
                app.issue_composer_snapshot_for_e2e().0.body
            } else {
                app.issue_reply_snapshot_for_e2e(4).body
            }
        });
        assert!(
            body.is_empty(),
            "successful offline Issue post consumes its own draft"
        );
    }
}
pub(super) fn settle_write(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while cx.read(|cx| app.read(cx).write_busy_op.is_some() || app.read(cx).planning.is_some()) {
        cx.run_until_parked();
        assert!(
            std::time::Instant::now() < deadline,
            "offline composer write never settled"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    cx.run_until_parked();
}
