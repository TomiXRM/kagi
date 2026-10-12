//! #1381: computed file previews and pinned actions in small, zoomed dialogs.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{
    dialog_a11y, e2e,
    i18n::{self, Lang},
    theme, KagiApp,
};
use kagi_git::CommitId;

#[path = "../support/git_fixture.rs"]
mod git_fixture;

struct Restore {
    zoom: f32,
    lang: Lang,
    saved: [(&'static str, Option<String>); 2],
}
impl Drop for Restore {
    fn drop(&mut self) {
        theme::set_zoom(self.zoom);
        i18n::set_lang(self.lang);
        for (key, value) in &self.saved {
            kagi::ui::settings::write_setting(key, value.as_deref());
        }
    }
}

fn full_path(index: usize) -> String {
    format!("preview-{index:03}-{}.txt", "long-path-segment-".repeat(12))
}

pub fn scenario_plan_preview_files(cx: &mut VisualTestAppContext) {
    let _restore = Restore {
        zoom: theme::zoom(),
        lang: i18n::lang(),
        saved: ["lang", "ui_zoom"].map(|key| (key, kagi::ui::settings::read_setting(key))),
    };
    for count in [1, 25, 300] {
        let fixture = build_fixture();
        let repo = fixture.path();
        git(repo, &["checkout", "-q", "-b", "preview", "main"]);
        for index in 0..count {
            std::fs::write(repo.join(full_path(index)), "preview\n").unwrap();
        }
        git(repo, &["add", "."]);
        git(repo, &["commit", "-q", "-m", "preview files"]);
        let commit = git_fixture::git_output(repo, &["rev-parse", "HEAD"]);
        for operation in ["merge", "cherry-pick", "revert"] {
            let branch = if operation == "revert" {
                "preview"
            } else {
                "main"
            };
            git(repo, &["checkout", "-q", branch]);
            let (app, window) = mount(cx, repo);
            crate::pr_detail_adaptive::resize(cx, window, (940., 660.));
            theme::set_zoom(1.67);
            for lang in [Lang::En, Lang::Ja] {
                i18n::set_lang(lang);
                run_card(cx, &app, window, operation, &commit, count);
            }
            unmount(cx, app, window);
        }
    }
    eprintln!("[gui-e2e] PASS plan_preview_files");
}

fn run_card(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    operation: &str,
    commit: &str,
    count: usize,
) {
    dialog_a11y::clear_recorded_a11y();
    for id in [
        "plan-preview-files",
        "plan-preview-more",
        "plan-preview-file-25",
    ] {
        e2e::clear_control_bounds(window.window_id(), id);
    }
    app.update(cx, |app, cx| match operation {
        "merge" => app.open_merge_modal("preview".into(), None, cx),
        "cherry-pick" => app.open_cherry_pick_modal(CommitId(commit.to_owned())),
        _ => app.open_revert_modal(CommitId(commit.to_owned())),
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        // The existing backend caps some previews at 200. Exercise a 300-entry
        // display payload without changing that read-only computation.
        if count == 300 {
            expand_stress_payload(app, operation, count);
        }
        let plan = match operation {
            "merge" => &app.merge_modal().unwrap().plan,
            "cherry-pick" => &app.cherry_pick_modal().unwrap().plan,
            _ => &app.revert_modal().unwrap().plan,
        };
        assert_eq!(
            plan.preview_files.len(),
            count,
            "computed {operation} preview"
        );
        cx.notify();
    });
    let viewport = cx
        .update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
            gpui::Bounds::new(
                gpui::point(gpui::px(0.), gpui::px(0.)),
                window.viewport_size(),
            )
        })
        .unwrap();
    assert!(
        e2e::control_bounds(window.window_id(), "plan-preview-files").is_some(),
        "{operation}/{count}: Files to change section missing"
    );
    let footer = e2e::control_bounds(window.window_id(), "modal-footer").expect("footer probe");
    assert!(
        footer.top() >= viewport.top() && footer.bottom() <= viewport.bottom(),
        "{operation}/{count}: footer outside window: {footer:?} / {viewport:?}"
    );
    assert_preview(window, count);
    app.update(cx, |app, cx| {
        match operation {
            "merge" => app.clear_merge_modal(),
            "cherry-pick" => app.clear_cherry_pick_modal(),
            _ => app.clear_revert_modal(),
        }
        cx.notify();
    });
}

fn assert_preview(window: AnyWindowHandle, count: usize) {
    let expected = kagi_ui_core::i18n::Msg::ModalFilesToChange
        .t()
        .replace("{}", &count.to_string());
    assert_eq!(
        dialog_a11y::recorded_note("plan-preview-files").unwrap().1,
        expected
    );
    assert!(
        e2e::control_bounds(window.window_id(), "plan-preview-file-25").is_none(),
        "bounded list"
    );
    assert_eq!(
        e2e::control_bounds(window.window_id(), "plan-preview-more").is_some(),
        count > 25
    );
    if count > 25 {
        let more = kagi_ui_core::i18n::Msg::ModalFilesMore
            .t()
            .replace("{}", &(count - 25).to_string());
        assert_eq!(
            dialog_a11y::recorded_note("plan-preview-more").unwrap().1,
            more
        );
    }
    assert_eq!(
        dialog_a11y::recorded_note("plan-preview-path-0").unwrap().1,
        full_path(0)
    );
    assert!(
        e2e::control_paint(window.window_id(), "plan-preview-path-0")
            .unwrap()
            .middle_ellipsis
    );
}

fn expand_stress_payload(app: &mut KagiApp, operation: &str, count: usize) {
    macro_rules! expand {
        ($get:ident, $set:ident) => {{
            let mut modal = app.$get().unwrap().clone();
            let mut plan = (*modal.plan).clone();
            if plan.preview_files.len() == count {
                return;
            }
            let template = plan
                .preview_files
                .first()
                .expect("real computed file")
                .clone();
            plan.preview_files = (0..count)
                .map(|index| {
                    let mut file = template.clone();
                    file.path = full_path(index).into();
                    file
                })
                .collect();
            modal.plan = std::sync::Arc::new(plan);
            app.$set(modal);
        }};
    }
    match operation {
        "merge" => expand!(merge_modal, set_merge_modal),
        "cherry-pick" => expand!(cherry_pick_modal, set_cherry_pick_modal),
        _ => expand!(revert_modal, set_revert_modal),
    }
}
