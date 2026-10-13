//! #1197: Home's recent heading stays inside its sidebar, with an ellipsis
//! instead of a hard clip and the navigator's quiet section typography.
use crate::macos::{build_fixture, mount, unmount};
use crate::pr_fields_focus::OfflineGh;
use gpui::{FontWeight, VisualTestAppContext};
use kagi::ui::{
    e2e, home,
    i18n::{self, Lang, Msg},
    theme,
};

pub fn scenario_home_heading(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved =
        crate::gui_isolation::SavedKeys::keep(&["recent_repos", "lang", "theme", "ui_zoom"]);
    let previous = (i18n::lang(), theme::theme().slug.to_string(), theme::zoom());
    let _gh = OfflineGh::install();
    let fixture = build_fixture();
    kagi::ui::tabs::record_recent_repo(fixture.path());
    let (app, window) = mount(cx, fixture.path());
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx));
    })
    .unwrap();
    check_narrow(cx, &app, window);
    for preset in ["apple-light", "apple-dark"] {
        theme::set_active(preset);
        for lang in [Lang::Ja, Lang::En] {
            i18n::set_lang(lang);
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                // Preserve production geometry, cover the requested 300px width,
                // then stress a narrower pane to force actual JA truncation.
                for width in [None, Some(300.), Some(220.)] {
                    home::set_sidebar_width_for_e2e(width);
                    app.update(cx, |_, cx| cx.notify());
                    cx.update_window(window, |_, window, cx| {
                        window.refresh();
                        window.draw(cx).clear();
                    })
                    .unwrap();
                    let sidebar =
                        e2e::control_bounds(window.window_id(), "home-sidebar-bounds").unwrap();
                    let heading =
                        e2e::control_paint(window.window_id(), "home-recent-heading").unwrap();
                    assert!(
                        heading.bounds.left() >= sidebar.left()
                            && heading.bounds.right() <= sidebar.right(),
                        "heading bounds must stay within the sidebar: {heading:?} / {sidebar:?}"
                    );
                    let line =
                        e2e::control_paint(window.window_id(), "home-recent-heading-line").unwrap();
                    assert!(
                        (f32::from(line.bounds.size.height) - 12. * 0.9 * zoom).abs() <= 1.,
                        "heading line keeps the section-header line height: {line:?}"
                    );
                    assert!(
                        line.bounds.top() >= line.mask.top() + gpui::px(1.)
                            && line.bounds.bottom() + gpui::px(1.) <= line.mask.bottom(),
                        "heading line box and descender clearance must fit its paint-stage mask: {line:?}"
                    );
                    let natural_width = cx
                        .update_window(window, |_, window, _| {
                            let text = Msg::HomeRecent.t();
                            let mut style = window.text_style();
                            style.font_family = theme::UI_FONT.into();
                            style.font_weight = heading.font_weight;
                            window
                                .text_system()
                                .shape_line(
                                    text.into(),
                                    heading.font_size,
                                    &[style.to_run(text.len())],
                                    None,
                                )
                                .width
                        })
                        .unwrap();
                    let available = line.bounds.size.width;
                    assert!(
                        heading.ellipsis,
                        "Home heading must use native text ellipsis, not overflow clipping"
                    );
                    assert_eq!(heading.font_weight, FontWeight::SEMIBOLD);
                    assert!((f32::from(heading.font_size) - 12. * 0.9 * zoom).abs() < 0.1);
                    assert_eq!(
                        heading.text_color,
                        gpui::rgb(theme::theme().text_sub).into()
                    );
                    eprintln!("[gui-e2e] home_heading {preset} {lang:?} zoom={zoom} width={width:?}: natural={natural_width:?}, available={available:?}, ellipsis={}", heading.ellipsis);
                    if lang == Lang::Ja && zoom == 1.67 && width == Some(220.) {
                        assert!(
                            natural_width > available,
                            "JA at 167% must exercise truncation, not merely configure it"
                        );
                    }
                }
            }
        }
    }
    home::set_sidebar_width_for_e2e(None);
    i18n::set_lang(previous.0);
    theme::set_active(&previous.1);
    theme::set_zoom(previous.2);
    unmount(cx, app, window);
}

fn check_narrow(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<kagi::ui::KagiApp>,
    window: gpui::AnyWindowHandle,
) {
    use kagi_git::github_repos::{OwnerRepos, RepoList, RepoListing};
    crate::home_github::wait_for(cx, app, "offline Home settles", crate::home_github::settled);
    app.update(cx, |app, cx| {
        app.home_github.repos = kagi::ui::home_github::GithubRepos::Loaded {
            sections: vec![OwnerRepos {
                owner: None,
                list: Ok(RepoList {
                    truncated: false,
                    repos: vec![RepoListing {
                        name_with_owner: "acme/gpui-component".into(),
                        host: "github.com".into(),
                        is_private: true,
                        is_fork: true,
                        description: "Long repository description that yields to its name".into(),
                        updated_at: "2026-10-11T00:00:00Z".into(),
                    }],
                }),
            }],
            orgs_error: None,
        };
        cx.notify();
    });
    theme::set_active("apple-dark");
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        for (width, zoom) in [(1392., 1.), (940., 1.67), (760., 1.67), (1392., 1.)] {
            theme::set_zoom(zoom);
            let wanted = gpui::size(gpui::px(width), gpui::px(660.));
            cx.update_window(window, |_, window, _| window.resize(wanted))
                .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                crate::macos::drain_native_events();
                cx.run_until_parked();
                let actual = cx
                    .update_window(window, |_, window, cx| {
                        window.refresh();
                        window.draw(cx).clear();
                        window.viewport_size()
                    })
                    .unwrap();
                if actual == wanted {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "native Home resize timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let viewport = gpui::Bounds::new(gpui::point(gpui::px(0.), gpui::px(0.)), wanted);
            for id in [
                "home-open-folder",
                "home-remote",
                "home-pane-repos",
                "home-pane-prs",
                "home-pane-issues",
                "home-github-refresh",
                "home-search",
                "home-recent-0",
            ] {
                let bounds = e2e::control_bounds(window.window_id(), id)
                    .unwrap_or_else(|| panic!("missing {id}"));
                if width >= 940. {
                    crate::recovery_layout::contained(viewport, bounds, id);
                }
            }
            let action_id = format!("home-gh-acme/gpui-component:{}", Msg::HomeGithubClone.t());
            let action = e2e::control_bounds(window.window_id(), &action_id).unwrap();
            // #1212 window containment uses the first repository row
            // (home-recent-0 above), not virtual-list overdraw. The restored
            // titlebar and toolbar can leave this GitHub row below the fold.
            // Its action must still fit the row, independently of clipping.
            if zoom > 1. {
                let row =
                    e2e::control_bounds(window.window_id(), "home-gh-acme/gpui-component").unwrap();
                crate::recovery_layout::contained(row, action, &action_id);
                let name = e2e::control_bounds(
                    window.window_id(),
                    "home-gh-acme/gpui-component-name-column",
                )
                .unwrap();
                assert!(
                    name.size.width >= row.size.width * 0.4,
                    "name priority: {name:?} / {row:?}"
                );
                eprintln!("[gui-e2e] home_narrow {lang:?}: row={row:?} name={name:?}");
            }
            if zoom == 1. {
                let sidebar =
                    e2e::control_bounds(window.window_id(), "home-sidebar-bounds").unwrap();
                let column =
                    e2e::control_bounds(window.window_id(), "home-content-column").unwrap();
                eprintln!(
                    "[gui-e2e] home_narrow wide {lang:?}: sidebar={sidebar:?} column={column:?}"
                );
                // Recorded on the unchanged native 1392px/100% layout:
                // the sidebar probe excludes its literal 1px right border.
                assert_eq!(sidebar.size.width, gpui::px(269.));
                assert_eq!(column.size.width, gpui::px(648.));
            }
        }
    }
}
