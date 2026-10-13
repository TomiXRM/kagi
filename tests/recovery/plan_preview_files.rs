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
        let plan = match operation {
            "merge" => &app.merge_modal().unwrap().plan,
            "cherry-pick" => &app.cherry_pick_modal().unwrap().plan,
            _ => &app.revert_modal().unwrap().plan,
        };
        assert_eq!(
            plan.preview_files.len(),
            if operation == "merge" {
                count.min(kagi_git::ops::PREVIEW_FILES_CAP)
            } else {
                count
            },
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
        footer.top() >= viewport.top()
            && footer.bottom() <= viewport.bottom()
            && footer.left() >= viewport.left()
            && footer.right() <= viewport.right(),
        "{operation}/{count}: footer outside window: {footer:?} / {viewport:?}"
    );
    let cap = kagi_git::ops::PREVIEW_FILES_CAP;
    let capped = operation == "merge" && count >= cap;
    assert_preview(window, if capped { cap } else { count }, capped);
    app.update(cx, |app, cx| {
        match operation {
            "merge" => app.clear_merge_modal(),
            "cherry-pick" => app.clear_cherry_pick_modal(),
            _ => app.clear_revert_modal(),
        }
        cx.notify();
    });
}

fn assert_preview(window: AnyWindowHandle, count: usize, capped: bool) {
    let display_count = if capped {
        format!("{count}+")
    } else {
        count.to_string()
    };
    let expected = kagi_ui_core::i18n::Msg::ModalFilesToChange
        .t()
        .replace("{}", &display_count);
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
        let more = if capped {
            match i18n::lang() {
                Lang::En => "…and more files".to_owned(),
                Lang::Ja => "…ほかにもファイルがあります".to_owned(),
            }
        } else {
            kagi_ui_core::i18n::Msg::ModalFilesMore
                .t()
                .replace("{}", &(count - 25).to_string())
        };
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

pub fn scenario_plan_preview_files_empty(cx: &mut VisualTestAppContext) {
    let _restore = Restore {
        zoom: theme::zoom(),
        lang: i18n::lang(),
        saved: ["lang", "ui_zoom"].map(|key| (key, kagi::ui::settings::read_setting(key))),
    };
    let fixture = build_fixture();
    let repo = fixture.path();
    std::fs::write(repo.join("preview-anchor.txt"), "original\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "preview anchor"]);
    git(repo, &["branch", "target", "main"]);
    git(repo, &["checkout", "-q", "-b", "preview"]);
    std::fs::write(repo.join("preview-change.txt"), "change\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "preview change"]);
    let commit = git_fixture::git_output(repo, &["rev-parse", "HEAD"]);
    for operation in ["merge-into", "merge", "cherry-pick", "revert"] {
        std::fs::write(repo.join("preview-anchor.txt"), "original\n").unwrap();
        git(repo, &["checkout", "-q", "main"]);
        if operation != "merge-into" {
            std::fs::write(repo.join("preview-anchor.txt"), "dirty\n").unwrap();
        }
        let (app, window) = mount(cx, repo);
        crate::pr_detail_adaptive::resize(cx, window, (940., 660.));
        theme::set_zoom(1.67);
        for lang in [Lang::En, Lang::Ja] {
            i18n::set_lang(lang);
            run_empty_card(cx, &app, window, operation, &commit, lang);
        }
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS plan_preview_files_empty");
}

fn run_empty_card(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    operation: &str,
    commit: &str,
    lang: Lang,
) {
    e2e::clear_control_bounds(window.window_id(), "plan-preview-files");
    app.update(cx, |app, cx| match operation {
        "merge-into" => app.open_merge_into_modal("preview".into(), "target".into(), cx),
        "merge" => app.open_merge_modal("preview".into(), None, cx),
        "cherry-pick" => app.open_cherry_pick_modal(CommitId(commit.to_owned())),
        _ => app.open_revert_modal(CommitId(commit.to_owned())),
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        let plan = match operation {
            "merge-into" | "merge" => &app.merge_modal().unwrap().plan,
            "cherry-pick" => &app.cherry_pick_modal().unwrap().plan,
            _ => &app.revert_modal().unwrap().plan,
        };
        assert!(
            plan.preview_files.is_empty(),
            "{operation}: uncomputed preview fixture"
        );
        assert_eq!(plan.blockers.is_empty(), operation == "merge-into");
        cx.notify();
    });
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    assert!(
        e2e::control_bounds(window.window_id(), "plan-preview-files").is_none(),
        "{operation}/{lang:?}: uncomputed preview must not claim zero files"
    );
    app.update(cx, |app, cx| {
        match operation {
            "merge-into" | "merge" => app.clear_merge_modal(),
            "cherry-pick" => app.clear_cherry_pick_modal(),
            _ => app.clear_revert_modal(),
        }
        cx.notify();
    });
}
